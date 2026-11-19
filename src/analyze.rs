use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::analysis::{Analysis, NoteRef, Part, PedalAction};
use crate::frac::Frac;
use crate::model::{Head, Ottava, Pitch, PitchMark, Score, Staff, TokMark};
use crate::text::write::write_line;

struct Key {
    pitch: Pitch,
    hand: Staff,
    start: Frac,
    end: Frac,
    tied: bool,
    velocity: u8,
}

impl Key {
    fn midi(&self) -> i32 {
        self.pitch.midi()
    }

    fn attacked_at(&self, t: Frac) -> bool {
        self.start == t && !self.tied
    }

    fn held_at(&self, t: Frac) -> bool {
        self.start <= t && t < self.end
    }
}

fn sounding(p: Pitch, ottava: Option<Ottava>) -> Pitch {
    let shift = match ottava {
        Some(Ottava::Up8) => 1,
        Some(Ottava::Down8) => -1,
        Some(Ottava::Up15) => 2,
        Some(Ottava::Down15) => -2,
        None => 0,
    };
    Pitch {
        octave: p.octave + shift,
        ..p
    }
}

fn keys(a: &Analysis) -> Vec<Key> {
    let mut out = Vec::new();
    for (id, leaf) in a
        .leaves
        .iter()
        .enumerate()
        .filter(|(_, l)| l.lane.is_voice())
    {
        let Some(ev) = leaf.event() else {
            continue;
        };
        let Head::Chord(notes) = &ev.head else {
            continue;
        };
        if ev.has(TokMark::Mute) {
            continue;
        }
        let Some(hand) = leaf.lane.staff() else {
            continue;
        };
        let ottava = a.ottava_at(hand, leaf.bar, leaf.start);
        for (note, n) in notes
            .iter()
            .enumerate()
            .filter(|(_, n)| !n.has(PitchMark::Mute))
        {
            let r = NoteRef {
                leaf: id,
                part: Part::Main,
                note,
            };
            out.push(Key {
                pitch: sounding(n.pitch, ottava),
                hand,
                start: a.abs(leaf.bar, leaf.start),
                end: a.abs(leaf.bar, leaf.end()),
                tied: n.tied,
                velocity: a.velocity(r).note.round().clamp(1.0, 127.0) as u8,
            });
        }
    }
    out
}

fn pedal_down(a: &Analysis) -> Vec<(Frac, Frac)> {
    let mut events: Vec<(Frac, PedalAction)> = a
        .pedals
        .iter()
        .map(|p| (a.abs(p.bar, p.at), p.action))
        .collect();
    // At one time a stop comes before a start, so a stop and a start at a barline are a change.
    events.sort_by_key(|(t, action)| (*t, *action != PedalAction::Stop));
    let mut out = Vec::new();
    let mut down: Option<Frac> = None;
    for (t, action) in events {
        if let Some(s) = down.take() {
            out.push((s, t));
        }
        if action != PedalAction::Stop {
            down = Some(t);
        }
    }
    if let Some(s) = down {
        let last = a.score.bars.len() - 1;
        out.push((s, a.abs(last, a.bar_end(last))));
    }
    out
}

// A key released while the pedal is down sounds until the pedal goes up or changes.
fn pedal_held(k: &Key, t: Frac, down: &[(Frac, Frac)]) -> bool {
    k.end <= t && down.iter().any(|(s, e)| *s < k.end && k.end < *e && t < *e)
}

fn position(score: &Score, bar: usize, t: Frac) -> String {
    let layout = &score.bars[bar].layout;
    let label = &score.bars[bar].label;
    if t >= layout.lead + layout.full {
        return format!("{label} end");
    }
    let ci = layout.cell_at(t);
    let offset = t - layout.cell_starts()[ci];
    if offset.is_zero() {
        format!("{label} {}", ci + 1)
    } else {
        format!("{label} {}+{}", ci + 1, offset / layout.cells[ci])
    }
}

// A time at the end of a bar is the start of the next bar.
fn bar_time(a: &Analysis, bar: usize, t: Frac) -> (usize, Frac) {
    let next = bar + 1;
    if t < a.bar_end(bar) || next == a.score.bars.len() {
        return (bar, t);
    }
    (next, a.score.bars[next].layout.lead + t - a.bar_end(bar))
}

fn release(k: &Key, down: &[(Frac, Frac)]) -> Frac {
    down.iter()
        .find(|(s, e)| *s < k.end && k.end < *e)
        .map_or(k.end, |(_, e)| *e)
}

fn onsets(a: &Analysis, keys: &[Key], bars: &[usize]) -> Vec<(usize, Frac)> {
    let attacks = a
        .leaves
        .iter()
        .filter(|l| keys.iter().any(|k| k.attacked_at(a.abs(l.bar, l.start))))
        .map(|l| (l.bar, l.start));
    let pedals = a.pedals.iter().map(|p| bar_time(a, p.bar, p.at));
    let mut out: Vec<(usize, Frac)> = attacks
        .chain(pedals)
        .filter(|(bar, _)| bars.contains(bar))
        .collect();
    out.sort();
    out.dedup();
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PedalNotes {
    Hide,
    Show,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Velocities {
    Hide,
    Show,
}

fn hand_sounds(sounds: &BTreeMap<(Staff, i32), String>, hand: Staff) -> String {
    let notes: Vec<&str> = sounds
        .iter()
        .filter(|((h, _), _)| *h == hand)
        .map(|(_, n)| n.as_str())
        .collect();
    if notes.is_empty() {
        "-".to_string()
    } else {
        notes.join(" ")
    }
}

pub fn sound(
    a: &Analysis,
    bars: &[usize],
    pedal_notes: PedalNotes,
    velocities: Velocities,
) -> String {
    let keys = keys(a);
    let down = pedal_down(a);
    let mut out = String::from("lh | rh, pitches low to high; ~ sounds from before");
    out.push_str(match pedal_notes {
        PedalNotes::Show => ", * is held by the pedal\n",
        PedalNotes::Hide => "; --pedal adds the notes that the pedal holds\n",
    });
    for (bar, t) in onsets(a, &keys, bars) {
        let at = a.abs(bar, t);
        let mut sounds: BTreeMap<(Staff, i32), String> = BTreeMap::new();
        if pedal_notes == PedalNotes::Show {
            for k in keys.iter().filter(|k| pedal_held(k, at, &down)) {
                sounds.insert((k.hand, k.midi()), format!("{}*", k.pitch));
            }
        }
        for k in keys.iter().filter(|k| k.held_at(at)) {
            let mark = match (k.attacked_at(at), velocities) {
                (false, _) => "~".to_string(),
                (true, Velocities::Show) => format!("@{}", k.velocity),
                (true, Velocities::Hide) => String::new(),
            };
            sounds.insert((k.hand, k.midi()), format!("{}{mark}", k.pitch));
        }
        let actions: Vec<PedalAction> = a
            .pedals
            .iter()
            .filter(|p| a.abs(p.bar, p.at) == at)
            .map(|p| p.action)
            .collect();
        let has = |x| actions.contains(&x);
        let up_and_down = has(PedalAction::Stop) && has(PedalAction::Start);
        let pedal = if has(PedalAction::Change) || up_and_down {
            "  [pedal change]"
        } else if has(PedalAction::Start) {
            "  [pedal down]"
        } else if has(PedalAction::Stop) {
            "  [pedal up]"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "{:<8} lh {} | rh {}{pedal}",
            position(a.score, bar, t),
            hand_sounds(&sounds, Staff::Lower),
            hand_sounds(&sounds, Staff::Upper)
        );
    }
    out
}

const CHORDS: [(&str, &[i32]); 12] = [
    ("maj7", &[0, 4, 7, 11]),
    ("7", &[0, 4, 7, 10]),
    ("m7", &[0, 3, 7, 10]),
    ("m7b5", &[0, 3, 6, 10]),
    ("dim7", &[0, 3, 6, 9]),
    ("", &[0, 4, 7]),
    ("m", &[0, 3, 7]),
    ("dim", &[0, 3, 6]),
    ("aug", &[0, 4, 8]),
    ("sus4", &[0, 5, 7]),
    ("sus2", &[0, 2, 7]),
    ("(no3)", &[0, 7]),
];

fn pc_name(p: Pitch) -> String {
    let s = p.to_string();
    s.trim_end_matches(|c: char| c.is_ascii_digit() || c == '-')
        .to_string()
}

fn chord_name(pcs: &BTreeMap<i32, Pitch>, bass: i32) -> String {
    let names = |set: &mut dyn Iterator<Item = &i32>| -> Vec<String> {
        set.filter_map(|pc| pcs.get(pc).map(|p| pc_name(*p)))
            .collect()
    };
    if pcs.len() == 1 {
        return names(&mut pcs.keys()).join("");
    }
    let mut roots: Vec<i32> = vec![bass];
    roots.extend(pcs.keys().filter(|pc| **pc != bass));
    for (suffix, tones) in CHORDS {
        for root in &roots {
            let chord: Vec<i32> = tones.iter().map(|i| (root + i) % 12).collect();
            if !chord.iter().all(|pc| pcs.contains_key(pc)) {
                continue;
            }
            let mut name = format!("{}{suffix}", pc_name(pcs[root]));
            let extra = names(&mut pcs.keys().filter(|pc| !chord.contains(pc) && **pc != bass));
            if !extra.is_empty() {
                let _ = write!(name, " add {}", extra.join(" "));
            }
            if bass != *root {
                let _ = write!(name, "/{}", pc_name(pcs[&bass]));
            }
            return name;
        }
    }
    format!("{{{}}}", names(&mut pcs.keys()).join(" "))
}

fn chords(a: &Analysis, keys: &[Key], bars: &[usize], out: &mut String) {
    out.push_str("chords, one for each beat; the bass includes a note that the pedal holds:\n");
    let down = pedal_down(a);
    for &bar in bars {
        let layout = &a.score.bars[bar].layout;
        let mut cells = Vec::new();
        for (start, len) in layout.cell_starts().into_iter().zip(&layout.cells) {
            let (from, to) = (a.abs(bar, start), a.abs(bar, start + *len));
            let mut sounding: Vec<&Key> = keys
                .iter()
                .filter(|k| k.start < to && k.end > from)
                .collect();
            let pedal_bass = keys
                .iter()
                .filter(|k| k.start < to && release(k, &down) > from)
                .min_by_key(|k| k.midi());
            sounding.extend(pedal_bass);
            sounding.sort_by_key(|k| k.midi());
            let Some(bass) = sounding.first().map(|k| k.midi().rem_euclid(12)) else {
                cells.push("-".to_string());
                continue;
            };
            let mut pcs: BTreeMap<i32, Pitch> = BTreeMap::new();
            for k in &sounding {
                pcs.entry(k.midi().rem_euclid(12)).or_insert(k.pitch);
            }
            cells.push(chord_name(&pcs, bass));
        }
        let _ = writeln!(out, "{:<4} {}", a.score.bars[bar].label, cells.join(" | "));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dissonances {
    Strong,
    All,
}

fn interval_name(semitones: i32, which: Dissonances) -> Option<&'static str> {
    match (semitones, which) {
        (1, _) => Some("m2"),
        (11, _) => Some("M7"),
        (13, _) => Some("m9"),
        (2, Dissonances::All) => Some("M2"),
        (6, Dissonances::All) => Some("tritone"),
        (10, Dissonances::All) => Some("m7"),
        (14, Dissonances::All) => Some("M9"),
        _ => None,
    }
}

fn clashes(a: &Analysis, keys: &[Key], bars: &[usize], which: Dissonances, out: &mut String) {
    out.push_str(match which {
        Dissonances::Strong => {
            "dissonances between the hands (m2, M7, m9; --all adds M2, tritone, m7, M9):\n"
        }
        Dissonances::All => "dissonances between the hands, up to a ninth:\n",
    });
    let mut none = true;
    for (bar, t) in onsets(a, keys, bars) {
        let at = a.abs(bar, t);
        let held: Vec<&Key> = keys.iter().filter(|k| k.held_at(at)).collect();
        let hand = |s: Staff| held.iter().filter(move |k| k.hand == s);
        for r in hand(Staff::Upper) {
            for l in hand(Staff::Lower) {
                let new = r.attacked_at(at) || l.attacked_at(at);
                let Some(name) = interval_name((r.midi() - l.midi()).abs(), which) else {
                    continue;
                };
                if new {
                    none = false;
                    let _ = writeln!(
                        out,
                        "{:<8} rh {} / lh {}: {name}",
                        position(a.score, bar, t),
                        r.pitch,
                        l.pitch
                    );
                }
            }
        }
    }
    if none {
        out.push_str("none\n");
    }
}

fn hand_text(score: &Score, bar: usize, hand: Staff) -> String {
    let b = &score.bars[bar];
    let mut lines: Vec<_> = b
        .lines
        .iter()
        .filter(|l| l.lane.is_voice() && l.lane.staff() == Some(hand))
        .collect();
    lines.sort_by_key(|l| l.lane.order());
    let mut out = String::new();
    for l in lines {
        let text = write_line(l.lane, &l.els, &b.layout).unwrap_or_default();
        let _ = writeln!(out, "{} {text}", l.lane);
    }
    out
}

fn repeats(score: &Score, bars: &[usize], out: &mut String) {
    out.push_str("bars equal to an earlier bar:\n");
    let mut none = true;
    for &bar in bars {
        let label = &score.bars[bar].label;
        let same = |hand: Staff| {
            let text = hand_text(score, bar, hand);
            (0..bar).find(|b| hand_text(score, *b, hand) == text)
        };
        let lower = if score.one_staff() {
            same(Staff::Upper)
        } else {
            same(Staff::Lower)
        };
        match (same(Staff::Upper), lower) {
            (Some(r), Some(l)) if r == l => {
                let _ = writeln!(out, "{label} = {}", score.bars[r].label);
                none = false;
            }
            (r, l) => {
                for (hand, b) in [("rh", r), ("lh", l)] {
                    if let Some(b) = b {
                        let _ = writeln!(out, "{label} {hand} = {} {hand}", score.bars[b].label);
                        none = false;
                    }
                }
            }
        }
    }
    if none {
        out.push_str("none\n");
    }
}

pub fn analyze(a: &Analysis, bars: &[usize], which: Dissonances) -> String {
    let keys = keys(a);
    let mut out = String::new();
    chords(a, &keys, bars, &mut out);
    clashes(a, &keys, bars, which, &mut out);
    repeats(a.score, bars, &mut out);
    out
}
