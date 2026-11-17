use std::ops::RangeInclusive;

use crate::frac::Frac;
use crate::model::{
    Attr, Bar, BarLayout, El, ElKind, Event, Head, Key, Lane, Line, MarkKind, Note, Pitch, Score,
    Slurs, Staff, Tok,
};
use crate::perform::DetectedPhrase;

fn changes(bar: &Bar) -> Vec<String> {
    let attrs = bar
        .attrs
        .iter()
        .filter(|a| !matches!(a, Attr::Break(_)))
        .map(|a| a.to_string());
    let lines = bar
        .lines
        .iter()
        .filter(|l| matches!(l.lane, Lane::PedRule | Lane::Mark(_, MarkKind::Clef, _)))
        .map(|l| format!("a {} line", l.lane));
    attrs.chain(lines).collect()
}

fn rest_bar(meter_of: &Bar) -> Result<Bar, String> {
    let meter = meter_of.layout.meter.clone();
    let cells = meter.cells()?;
    let full: Frac = cells.iter().copied().sum();
    let rest = |staff| Line {
        lane: Lane::Voice(staff, 1),
        src: 0,
        els: vec![El {
            start: Frac::ZERO,
            dur: full,
            kind: ElKind::Tok(Tok::Event(Box::new(Event {
                grace: None,
                head: Head::BarRest,
                marks: Vec::new(),
                slurs: Slurs::default(),
                after: None,
            }))),
        }],
    };
    Ok(Bar {
        label: String::new(),
        src: 0,
        attrs: Vec::new(),
        lines: vec![rest(Staff::Upper), rest(Staff::Lower)],
        layout: BarLayout {
            meter,
            cells,
            lead: Frac::ZERO,
            trail: Frac::ZERO,
            full,
        },
    })
}

pub fn insert(score: &mut Score, at: usize, count: usize) -> Result<(), String> {
    let Some(meter_of) = score.bars.get(at.saturating_sub(1)).or(score.bars.last()) else {
        return Err(
            "the file has no bar to take the meter from; write the first bar by hand".into(),
        );
    };
    let bar = rest_bar(meter_of)?;
    score.bars.splice(at..at, std::iter::repeat_n(bar, count));
    Ok(())
}

// The return value names each change of the deleted bars that no longer applies to the bars after them.
pub fn delete(score: &mut Score, range: RangeInclusive<usize>) -> Vec<String> {
    let removed: Vec<Bar> = score.bars.drain(range).collect();
    removed
        .iter()
        .flat_map(|b| {
            changes(b)
                .into_iter()
                .map(move |c| format!("bar {}: {c}", b.label))
        })
        .collect()
}

pub fn copy(score: &mut Score, range: RangeInclusive<usize>, at: usize) -> Result<(), String> {
    let bars: Vec<Bar> = score.bars[range].to_vec();
    if let Some((b, c)) = bars
        .iter()
        .find_map(|b| changes(b).into_iter().next().map(|c| (b, c)))
    {
        return Err(format!(
            "bar {} has {c}, which also applies to the bars after the copy; copy the notes by hand",
            b.label
        ));
    }
    score.bars.splice(at..at, bars);
    Ok(())
}

struct Interval {
    steps: i32,
    semitones: i32,
}

// The fifths shift with the smallest key signature gives the spelling.
fn interval(semitones: i32, fifths: i32) -> (Interval, i32) {
    let base = (7 * semitones.rem_euclid(12)).rem_euclid(12);
    let shift = if (fifths + base).abs() <= (fifths + base - 12).abs() {
        base
    } else {
        base - 12
    };
    let steps = (4 * shift).rem_euclid(7) + 7 * semitones.div_euclid(12);
    (Interval { steps, semitones }, shift)
}

fn shift_pitch(p: Pitch, iv: &Interval) -> Result<Pitch, String> {
    let degree = i32::from(p.octave) * 7 + i32::from(p.step) + iv.steps;
    let octave = degree.div_euclid(7);
    let step = degree.rem_euclid(7);
    let natural = Pitch {
        step: step as u8,
        alter: 0,
        octave: octave as i8,
    };
    let alter = p.midi() + iv.semitones - natural.midi();
    if alter.abs() > 2 {
        return Err(format!("{p} cannot be spelled after the transposition"));
    }
    Ok(Pitch {
        alter: alter as i8,
        ..natural
    })
}

fn shift_notes(notes: &mut [Note], iv: &Interval) -> Result<usize, String> {
    for n in notes.iter_mut() {
        n.pitch = shift_pitch(n.pitch, iv)?;
    }
    Ok(notes.len())
}

fn shift_event(ev: &mut Event, iv: &Interval) -> Result<usize, String> {
    let mut count = 0;
    if let Head::Chord(notes) = &mut ev.head {
        count += shift_notes(notes, iv)?;
    }
    for grace in [&mut ev.grace, &mut ev.after].into_iter().flatten() {
        for g in &mut grace.notes {
            count += shift_notes(&mut g.notes, iv)?;
        }
    }
    Ok(count)
}

fn shift_key(k: &mut Key, shift: i32) -> Result<(), String> {
    let fifths = i32::from(k.fifths) + shift;
    if !(-7..=7).contains(&fifths) {
        return Err(format!(
            "a key signature would have {} sharps or flats",
            fifths.abs()
        ));
    }
    k.fifths = fifths as i8;
    Ok(())
}

// Writes the phrases that --phrase finds as marks, so that they can be corrected.
pub fn add_phrases(score: &mut Score) -> Result<Vec<DetectedPhrase>, String> {
    let has_marks = score
        .bars
        .iter()
        .any(|b| b.lines.iter().any(|l| l.lane == Lane::Phrase));
    if has_marks {
        return Err("the score has phrase marks; delete its phrase lines first".into());
    }
    let found = crate::perform::detected_phrases(&crate::analysis::Analysis::new(score));
    let mut by_bar: std::collections::BTreeMap<usize, Vec<&DetectedPhrase>> =
        std::collections::BTreeMap::new();
    for p in &found {
        by_bar.entry(p.bar).or_default().push(p);
    }
    for (b, marks) in by_bar {
        let layout = &score.bars[b].layout;
        let end = layout.total() - layout.trail;
        let ends = marks.iter().skip(1).map(|p| p.time).chain([end]);
        let els: Vec<El> = marks
            .iter()
            .zip(ends)
            .map(|(p, next)| El {
                start: p.time,
                dur: next - p.time,
                kind: ElKind::Tok(Tok::Phrase(p.mark)),
            })
            .collect();
        let els = crate::musicxml::import::with_x(els, layout);
        score.bars[b].lines.push(Line {
            lane: Lane::Phrase,
            src: 0,
            els,
        });
    }
    Ok(found)
}

// The return value is the number of transposed notes. A bar range keeps the key signatures.
pub fn transpose(
    score: &mut Score,
    semitones: i32,
    range: Option<RangeInclusive<usize>>,
) -> Result<usize, String> {
    let fifths = score
        .header
        .iter()
        .find_map(|a| match a {
            Attr::Key(k) => Some(i32::from(k.fifths)),
            _ => None,
        })
        .unwrap_or(0);
    let (iv, shift) = interval(semitones, fifths);
    let has_key = score.header.iter().any(|a| matches!(a, Attr::Key(_)));
    if range.is_none() && !has_key {
        score.header.push(Attr::Key(Key {
            fifths: 0,
            mode: None,
        }));
    }
    if range.is_none() {
        let bar_attrs = score.bars.iter_mut().flat_map(|b| b.attrs.iter_mut());
        for a in score.header.iter_mut().chain(bar_attrs) {
            if let Attr::Key(k) = a {
                shift_key(k, shift)?;
            }
        }
    }
    let mut count = 0;
    let mut failure = None;
    crate::fmt::for_each_leaf_mut(score, |_, bar, _, tok| {
        let in_range = range.as_ref().is_none_or(|r| r.contains(&bar));
        let Tok::Event(ev) = tok else {
            return;
        };
        if !in_range || failure.is_some() {
            return;
        }
        match shift_event(ev, &iv) {
            Ok(n) => count += n,
            Err(e) => failure = Some(e),
        }
    });
    match failure {
        Some(e) => Err(e),
        None => Ok(count),
    }
}

fn line_key(line: &str) -> Option<(&str, &str)> {
    let mut words = line.split_whitespace();
    Some((words.next()?, words.next()?))
}

// Each new line replaces the lines with its bar and lane, or adds that lane to the end of its bar.
pub fn set_lines(src: &str, labels: &[&str], new: &[String]) -> Result<String, String> {
    let mut lines: Vec<String> = src.lines().map(str::to_string).collect();
    for line in new {
        let Some((bar, lane)) = line_key(line).filter(|(_, lane)| lane.starts_with(|c| c != '|'))
        else {
            return Err(format!(
                "`{line}` is not a line of a bar: it starts with a bar and a lane"
            ));
        };
        if !labels.contains(&bar) {
            return Err(format!("there is no bar {bar}"));
        }
        let same = |l: &String| line_key(l) == Some((bar, lane));
        match lines.iter().position(same) {
            Some(at) => {
                lines[at] = line.clone();
                lines = lines
                    .into_iter()
                    .enumerate()
                    .filter(|(i, l)| *i <= at || !same(l))
                    .map(|(_, l)| l)
                    .collect();
            }
            None => {
                let last = lines
                    .iter()
                    .rposition(|l| line_key(l).is_some_and(|(b, _)| b == bar))
                    .ok_or_else(|| format!("bar {bar} has no lines"))?;
                lines.insert(last + 1, line.clone());
            }
        }
    }
    Ok(lines.join(
        "
",
    ) + "
")
}
