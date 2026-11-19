use crate::analysis::{Analysis, NoteRef, Part, flatten};
use crate::check::expected_labels;
use crate::frac::{Frac, NoteValue};
use crate::model::{
    AccDisplay, Attr, Clef, El, ElKind, Event, GroupHead, Head, Lane, MarkKind, Meter, Note,
    PedalStyle, PitchMark, Score, Slurs, Staff, Tok, TokMark,
};
use crate::text::{parse, write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fix {
    No,
    Yes,
}

pub fn format(mut score: Score, fix: Fix) -> Result<String, String> {
    if fix == Fix::Yes {
        split_runs(&mut score);
        let labels = expected_labels(&score);
        for (bar, label) in score.bars.iter_mut().zip(labels) {
            bar.label = label;
        }
    }
    drop_defaults(&mut score);
    let first = write::write(&score)?;
    let again = parse::parse(&first);
    if let Some(d) = again.diags.first() {
        return Err(format!("internal: the formatted text does not parse: {d}"));
    }
    write::write(&again.score)
}

pub(crate) fn for_each_leaf_mut(
    score: &mut Score,
    mut f: impl FnMut(usize, usize, Lane, &mut Tok),
) {
    fn walk(
        els: &mut [El],
        n: &mut usize,
        bar: usize,
        lane: Lane,
        f: &mut impl FnMut(usize, usize, Lane, &mut Tok),
    ) {
        for e in els {
            match &mut e.kind {
                ElKind::Tok(Tok::Gap) => {}
                ElKind::Tok(t) => {
                    f(*n, bar, lane, t);
                    *n += 1;
                }
                ElKind::Group(_, c) => walk(c, n, bar, lane, f),
            }
        }
    }
    let mut n = 0;
    for (b, bar) in score.bars.iter_mut().enumerate() {
        let mut idx: Vec<usize> = (0..bar.lines.len())
            .filter(|i| bar.lines[*i].lane.is_voice())
            .collect();
        idx.sort_by_key(|i| bar.lines[*i].lane.order());
        for i in idx {
            let lane = bar.lines[i].lane;
            walk(&mut bar.lines[i].els, &mut n, b, lane, &mut f);
        }
    }
}

struct Edits {
    signs: std::collections::HashSet<NoteRef>,
    multi_voice: Vec<[bool; 2]>,
}

fn drop_defaults(score: &mut Score) {
    let edits = {
        let a = Analysis::new(score);
        let multi_voice = (0..score.bars.len())
            .map(|b| {
                [
                    a.voice_count(b, Staff::Upper) > 1,
                    a.voice_count(b, Staff::Lower) > 1,
                ]
            })
            .collect();
        Edits {
            signs: a.signs,
            multi_voice,
        }
    };
    for_each_leaf_mut(score, |leaf, bar, lane, tok| {
        let Tok::Event(ev) = tok else { return };
        let fix_notes = |notes: &mut [Note], part: Part| {
            for (i, n) in notes.iter_mut().enumerate() {
                let sign = edits.signs.contains(&NoteRef {
                    leaf,
                    part,
                    note: i,
                });
                if sign && n.acc == AccDisplay::Force {
                    n.acc = AccDisplay::Default;
                }
                if !sign {
                    n.marks.retain(|m| *m != PitchMark::Hide);
                }
            }
        };
        if let Some(g) = &mut ev.grace {
            for (gi, gn) in g.notes.iter_mut().enumerate() {
                fix_notes(&mut gn.notes, Part::Grace(gi));
            }
        }
        if let Head::Chord(notes) = &mut ev.head {
            fix_notes(notes, Part::Main);
        }
        if let Some(g) = &mut ev.after {
            for (gi, gn) in g.notes.iter_mut().enumerate() {
                fix_notes(&mut gn.notes, Part::After(gi));
            }
        }
        let Lane::Voice(staff, n) = lane else { return };
        if edits.multi_voice[bar][usize::from(staff.number() - 1)] {
            let default = if n % 2 == 1 {
                TokMark::StemUp
            } else {
                TokMark::StemDown
            };
            ev.marks.retain(|m| *m != default);
        }
    });
    drop_default_pedals(score);
    drop_default_clefs(score);
    drop_default_header(score);
}

fn drop_default_header(score: &mut Score) {
    if let Some(first) = score.bars.first_mut() {
        let (moved, kept): (Vec<Attr>, Vec<Attr>) = std::mem::take(&mut first.attrs)
            .into_iter()
            .partition(|a| matches!(a, Attr::Key(_) | Attr::Time(_)));
        first.attrs = kept;
        for a in moved {
            let same_kind = |h: &Attr| std::mem::discriminant(h) == std::mem::discriminant(&a);
            score.header.retain(|h| !same_kind(h));
            score.header.push(a);
        }
    }
    score.header.retain(|a| match a {
        Attr::Key(k) => k.fifths != 0 || k.mode.is_some(),
        Attr::Time(m) => *m != Meter::four_four(),
        Attr::PedalStyle(s) => *s != PedalStyle::Line,
        Attr::Program(p) => *p != 1,
        _ => true,
    });
}

fn drop_default_pedals(score: &mut Score) {
    let mut rule: Option<Vec<El>> = None;
    for bar in &mut score.bars {
        if bar.attrs.iter().any(|a| matches!(a, Attr::Time(_))) {
            rule = None;
        }
        if let Some(i) = bar.lines.iter().position(|l| l.lane == Lane::PedRule) {
            let els = &bar.lines[i].els;
            let stops = !flatten(els)
                .iter()
                .any(|(_, _, t)| matches!(t, Tok::Pedal(_) | Tok::Cont));
            let same = match &rule {
                Some(r) => r == els,
                None => stops,
            };
            if same {
                bar.lines.remove(i);
            } else {
                rule = (!stops).then(|| els.clone());
            }
        }
        let applies =
            !bar.layout.is_partial() && !bar.attrs.iter().any(|a| matches!(a, Attr::Cells(_)));
        if let (Some(r), true) = (&rule, applies) {
            if let Some(i) = bar
                .lines
                .iter()
                .position(|l| l.lane == Lane::Ped && l.els == *r)
            {
                bar.lines.remove(i);
            }
        }
        if rule.is_none() || !applies {
            let none = |l: &crate::model::Line| {
                l.lane == Lane::Ped
                    && flatten(&l.els)
                        .iter()
                        .all(|(_, _, t)| matches!(t, Tok::Empty | Tok::Gap))
            };
            bar.lines.retain(|l| !none(l));
        }
    }
}

fn drop_default_clefs(score: &mut Score) {
    let mut current = [Clef::Treble, Clef::Bass];
    for bar in &mut score.bars {
        for staff in [Staff::Upper, Staff::Lower] {
            let k = usize::from(staff.number() - 1);
            let Some(line) = bar
                .lines
                .iter_mut()
                .find(|l| l.lane == Lane::Mark(staff, MarkKind::Clef, 1))
            else {
                continue;
            };
            replace_clefs(&mut line.els, &mut current[k]);
        }
    }
}

fn replace_clefs(els: &mut [El], current: &mut Clef) {
    for e in els {
        match &mut e.kind {
            ElKind::Tok(t @ Tok::Clef(_)) => {
                let Tok::Clef(c) = t else { continue };
                if *c == *current {
                    *t = Tok::Empty;
                } else {
                    *current = *c;
                }
            }
            ElKind::Group(_, c) => replace_clefs(c, current),
            ElKind::Tok(_) => {}
        }
    }
}

fn split_runs(score: &mut Score) {
    for bar in &mut score.bars {
        let mut bounds = bar.layout.cell_starts();
        bounds.push(bar.layout.total());
        for line in bar.lines.iter_mut().filter(|l| l.lane.is_voice()) {
            split_els(&mut line.els, Frac::int(1), Some(&bounds));
        }
    }
}

fn split_els(els: &mut Vec<El>, factor: Frac, bounds: Option<&[Frac]>) {
    let mut out = Vec::with_capacity(els.len());
    for mut e in els.drain(..) {
        match &mut e.kind {
            ElKind::Group(head, children) => {
                let (a, b) = match *head {
                    GroupHead::Tremolo { .. } => (2, 1),
                    GroupHead::Tuplet { num, ratio, .. } => (
                        num,
                        ratio
                            .or_else(|| write::default_ratio(num, e.dur))
                            .unwrap_or(num),
                    ),
                };
                split_els(
                    children,
                    factor * Frac::new(i64::from(a), i64::from(b)),
                    None,
                );
                out.push(e);
            }
            ElKind::Tok(Tok::Event(ev))
                if ev.head != Head::BarRest && NoteValue::of(e.dur * factor).is_none() =>
            {
                let parts = pieces(e.start, e.dur, factor, bounds);
                let mut t = e.start;
                let last = parts.len() - 1;
                for (k, d) in parts.into_iter().enumerate() {
                    out.push(El {
                        start: t,
                        dur: d,
                        kind: ElKind::Tok(Tok::Event(Box::new(piece(ev, k, last)))),
                    });
                    t += d;
                }
            }
            ElKind::Tok(_) => out.push(e),
        }
    }
    *els = out;
}

// Splits a run into note values: at the latest cell boundary that works, else the largest plain value first.
fn pieces(start: Frac, dur: Frac, factor: Frac, bounds: Option<&[Frac]>) -> Vec<Frac> {
    if NoteValue::of(dur * factor).is_some() {
        return vec![dur];
    }
    let end = start + dur;
    if let Some(b) = bounds {
        for &t in b.iter().rev().filter(|t| **t > start && **t < end) {
            let first = t - start;
            if NoteValue::of(first * factor).is_some() {
                let mut v = vec![first];
                v.extend(pieces(t, end - t, factor, bounds));
                return v;
            }
        }
    }
    let written = dur * factor;
    let mut v = Frac::int(4);
    while v > written {
        v = v / 2;
    }
    let first = v / factor;
    let mut out = vec![first];
    out.extend(pieces(start + first, dur - first, factor, bounds));
    out
}

fn piece(ev: &Event, k: usize, last: usize) -> Event {
    let is_last = k == last;
    if k == 0 {
        let mut e = ev.clone();
        if last > 0 {
            e.after = None;
            e.slurs.ends.clear();
            if let Head::Chord(notes) = &mut e.head {
                for n in notes {
                    n.marks.retain(|m| {
                        !matches!(
                            m,
                            PitchMark::TieLater | PitchMark::LetRing | PitchMark::Gliss(_)
                        )
                    });
                }
            }
        }
        return e;
    }
    let head = match &ev.head {
        Head::Chord(notes) => Head::Chord(
            notes
                .iter()
                .map(|n| Note {
                    tied: true,
                    hidden: n.hidden,
                    pitch: n.pitch,
                    acc: AccDisplay::Default,
                    marks: n
                        .marks
                        .iter()
                        .copied()
                        .filter(|m| match m {
                            PitchMark::CrossStaff(_) | PitchMark::TieUp | PitchMark::TieDown => {
                                true
                            }
                            PitchMark::TieLater | PitchMark::LetRing | PitchMark::Gliss(_) => {
                                is_last
                            }
                            _ => false,
                        })
                        .collect(),
                })
                .collect(),
        ),
        other => other.clone(),
    };
    Event {
        grace: None,
        head,
        marks: Vec::new(),
        slurs: Slurs {
            ends: if is_last {
                ev.slurs.ends.clone()
            } else {
                Vec::new()
            },
            starts: Vec::new(),
        },
        after: if is_last { ev.after.clone() } else { None },
    }
}
