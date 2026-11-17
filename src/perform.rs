use std::collections::{HashMap, HashSet};

use crate::accidental::key_alter;
use crate::analysis::{Analysis, NoteRef, Part, PedalAction, flatten};
use crate::frac::Frac;
use crate::model::{
    ArpKind, Articulation, Attr, GroupHead, Jump, Lane, Note, OrnamentKind, Pitch, PitchMark,
    Score, Sign, Staff, Tok, TokMark,
};

pub const TICKS_PER_QUARTER: f64 = 480.0;
const TICKS_PER_WHOLE: f64 = 4.0 * TICKS_PER_QUARTER;
// MuseScore plays a score with no metronome mark at this tempo.
const DEFAULT_QPM: f64 = 120.0;
const FERMATA_STRETCH: f64 = 2.0;
const GRACE_MS: f64 = 70.0;
const ORNAMENT_MS: f64 = 70.0;
const ARPEGGIO_MS: f64 = 35.0;
const PEDAL_CHANGE_MS: f64 = 60.0;
const SHORTEST_MS: f64 = 20.0;
// The release sound after the last note. sfizz stops at the end of the MIDI file, because
// with Salamander a signal of one sample value stays, and its wait for silence never ends.
const TAIL_MS: f64 = 4000.0;
// Two voices that play one key at about the same time strike it once.
const SAME_STRIKE: f64 = 10.0;

pub struct Performance {
    pub notes: Vec<Played>,
    // CC 64 values.
    pub pedal: Vec<(f64, u8)>,
    // Quarter notes per minute.
    pub tempo: Vec<(f64, f64)>,
    pub meters: Vec<(f64, u32, u32)>,
    pub program: u8,
    pub volume: Option<u8>,
    pub pan: Option<u8>,
    pub end: f64,
}

pub struct Played {
    pub on: f64,
    pub off: f64,
    pub key: u8,
    pub velocity: u8,
    pub staff: Staff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Humanize {
    Off,
    Seed(u64),
}

#[derive(Clone, Copy, PartialEq)]
enum Gate {
    Normal,
    Legato,
    Portato,
    Staccato,
    Staccatissimo,
}

#[derive(Clone, Copy)]
enum Figure {
    Ornament {
        kind: OrnamentKind,
        upper: i32,
        lower: i32,
    },
    Tremolo(u8),
}

#[derive(Clone, Copy)]
enum ArpGroup {
    Token(usize),
    CrossStaff,
}

#[derive(Clone)]
struct Raw {
    on: f64,
    end: f64,
    key: i32,
    vel: f64,
    lane: Lane,
    tied: bool,
    tie_later: bool,
    attack: f64,
    release: f64,
    gate: Gate,
    figure: Option<Figure>,
    arp: Option<(ArpKind, ArpGroup)>,
    downbeat: bool,
}

struct Instance {
    bar: usize,
    start: f64,
}

struct TempoMap {
    points: Vec<(f64, f64)>,
}

impl TempoMap {
    fn qpm(&self, t: f64) -> f64 {
        self.points
            .iter()
            .take_while(|(at, _)| *at <= t)
            .last()
            .map_or(DEFAULT_QPM, |(_, q)| *q)
    }

    fn ticks(&self, t: f64, ms: f64) -> f64 {
        ms * self.qpm(t) * TICKS_PER_QUARTER / 60_000.0
    }
}

fn ticks(f: Frac) -> f64 {
    f.num() as f64 * TICKS_PER_WHOLE / f.den() as f64
}

pub fn perform(a: &Analysis, humanize: Humanize) -> Performance {
    let order = instances(a.score);
    let mut by_bar: Vec<Vec<usize>> = vec![Vec::new(); a.score.bars.len()];
    for (id, leaf) in a.leaves.iter().enumerate() {
        by_bar[leaf.bar].push(id);
    }
    let tempo = tempo_map(a, &order, &by_bar);
    let legato = slurred_leaves(a);
    let mut raw = Vec::new();
    for inst in &order {
        let lead = a.score.bars[inst.bar].layout.lead;
        for &id in &by_bar[inst.bar] {
            let at = inst.start + ticks(a.leaves[id].start - lead);
            leaf_notes(a, id, at, &tempo, legato.contains(&id), &mut raw);
        }
    }
    let raw: Vec<Raw> = join_ties(raw)
        .into_iter()
        .flat_map(|r| play_figure(r, &tempo))
        .collect();
    let mut notes = timed(raw, &tempo, humanize);
    remove_overlaps(&mut notes);
    let (program, volume, pan) = channel_settings(a.score);
    let pedal = pedal(a, &order, &tempo);
    let last = notes
        .iter()
        .map(|n| n.off)
        .chain(pedal.iter().map(|p| p.0))
        .fold(0.0, f64::max);
    Performance {
        notes,
        pedal,
        end: last + tempo.ticks(last, TAIL_MS),
        tempo: tempo.points,
        meters: meters(a.score, &order),
        program,
        volume,
        pan,
    }
}

fn instances(score: &Score) -> Vec<Instance> {
    let mut t = 0.0;
    play_order(score)
        .into_iter()
        .map(|bar| {
            let start = t;
            t += ticks(score.bars[bar].layout.length());
            Instance { bar, start }
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum After {
    Repeats,
    Jumped { to_fine: bool, to_coda: bool },
}

// The bars in the order they sound: repeats, voltas and jumps played out.
// After a jump the repeats are not played again and each volta plays its last ending.
fn play_order(score: &Score) -> Vec<usize> {
    let bars = &score.bars;
    let has = |b: usize, f: &dyn Fn(&Attr) -> bool| bars[b].attrs.iter().any(f);
    let volta = |b: usize| {
        bars[b].attrs.iter().find_map(|a| match a {
            Attr::Volta { numbers, .. } => Some(numbers.clone()),
            _ => None,
        })
    };
    let repeat = |b: usize| {
        bars[b].attrs.iter().find_map(|a| match a {
            Attr::Repeat { start, end } => Some((*start, *end)),
            _ => None,
        })
    };
    let jump = |b: usize| {
        bars[b].attrs.iter().find_map(|a| match a {
            Attr::Jump(j) => Some(*j),
            _ => None,
        })
    };
    let sign = |s: Sign| (0..bars.len()).find(|&b| has(b, &|a| *a == Attr::Sign(s)));
    let last_ending = |b: usize| {
        let run_end = (b..bars.len())
            .take_while(|&x| volta(x).is_some())
            .last()
            .unwrap_or(b);
        let run_start = (0..=b)
            .rev()
            .take_while(|&x| volta(x).is_some())
            .last()
            .unwrap_or(b);
        let top = (run_start..=run_end)
            .filter_map(volta)
            .flatten()
            .max()
            .unwrap_or(0);
        volta(b).is_some_and(|n| n.contains(&top))
    };

    let mut order = Vec::new();
    let mut state = After::Repeats;
    let (mut b, mut pass, mut start, mut back) = (0, 1, 0, false);
    let limit = bars.len() * 64;
    while b < bars.len() && order.len() < limit {
        let (repeat_start, repeat_end) = repeat(b).unwrap_or((false, None));
        if repeat_start && !back {
            start = b;
            pass = 1;
        }
        back = false;
        if let Some(numbers) = volta(b) {
            let plays = match state {
                After::Repeats => numbers.contains(&pass),
                After::Jumped { .. } => last_ending(b),
            };
            if !plays {
                b += 1;
                continue;
            }
        }
        order.push(b);
        let j = jump(b);
        if let After::Jumped { to_fine, to_coda } = state {
            if to_fine && j == Some(Jump::Fine) {
                break;
            }
            if to_coda && j == Some(Jump::ToCoda) {
                match sign(Sign::Coda) {
                    Some(coda) => b = coda,
                    None => break,
                }
                state = After::Jumped {
                    to_fine: false,
                    to_coda: false,
                };
                continue;
            }
        }
        if state == After::Repeats {
            if let Some(times) = repeat_end {
                if pass < times {
                    pass += 1;
                    b = start;
                    back = true;
                    continue;
                }
                pass = 1;
                start = b + 1;
            }
            let target = match j {
                Some(Jump::Dc | Jump::DcFine | Jump::DcCoda) => Some(0),
                Some(Jump::Ds | Jump::DsFine | Jump::DsCoda) => sign(Sign::Segno),
                _ => None,
            };
            if let Some(t) = target {
                state = After::Jumped {
                    to_fine: matches!(j, Some(Jump::DcFine | Jump::DsFine)),
                    to_coda: matches!(j, Some(Jump::DcCoda | Jump::DsCoda)),
                };
                b = t;
                continue;
            }
        }
        b += 1;
    }
    order
}

fn tempo_map(a: &Analysis, order: &[Instance], by_bar: &[Vec<usize>]) -> TempoMap {
    let mut marks = Vec::new();
    let mut holds = Vec::new();
    for inst in order {
        let bar = &a.score.bars[inst.bar];
        let local = |t: Frac| inst.start + ticks(t - bar.layout.lead);
        for line in bar.lines.iter().filter(|l| l.lane == Lane::Tempo) {
            for (start, _, tok) in flatten(&line.els) {
                if let Tok::Tempo(t) = tok {
                    if let Some(q) = t.mark.as_ref().map(|m| m.quarters_per_minute()) {
                        marks.push((local(start), q));
                    }
                }
            }
        }
        for leaf in by_bar[inst.bar].iter().map(|&id| &a.leaves[id]) {
            let held = leaf.event().is_some_and(|ev| {
                ev.marks
                    .iter()
                    .any(|m| matches!(m, TokMark::Fermata { .. }))
            });
            if held {
                holds.push((local(leaf.start), local(leaf.end())));
            }
        }
    }
    let base = |t: f64| {
        marks
            .iter()
            .take_while(|(at, _)| *at <= t)
            .last()
            .map_or(DEFAULT_QPM, |(_, q)| *q)
    };
    let mut cuts: Vec<f64> = marks.iter().map(|m| m.0).collect();
    cuts.extend(holds.iter().flat_map(|h| [h.0, h.1]));
    cuts.push(0.0);
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let mut points: Vec<(f64, f64)> = Vec::new();
    for t in cuts {
        let held = holds.iter().any(|(s, e)| *s <= t && t < *e);
        let q = base(t) / if held { FERMATA_STRETCH } else { 1.0 };
        if points.last().is_none_or(|p| p.1 != q) {
            points.push((t, q));
        }
    }
    TempoMap { points }
}

// The leaves under a slur, except the last one: they play legato.
fn slurred_leaves(a: &Analysis) -> HashSet<usize> {
    let mut out = HashSet::new();
    for s in &a.slurs {
        let lane = a.leaves[s.start.leaf].lane;
        let Some(ids) = a.lane_leaves.get(&lane) else {
            continue;
        };
        out.extend(
            ids.iter()
                .copied()
                .filter(|&id| s.start.leaf <= id && id < s.end.leaf),
        );
    }
    out
}

fn gate(marks: &[TokMark], legato: bool) -> Gate {
    let has = |art| marks.contains(&TokMark::Art(art));
    if has(Articulation::Staccato) && has(Articulation::Tenuto) {
        Gate::Portato
    } else if has(Articulation::Staccatissimo) {
        Gate::Staccatissimo
    } else if has(Articulation::Staccato) {
        Gate::Staccato
    } else if legato || has(Articulation::Tenuto) {
        Gate::Legato
    } else {
        Gate::Normal
    }
}

fn neighbor(p: Pitch, dir: i8, alter: Option<i8>, a: &Analysis, bar: usize) -> i32 {
    let step = i16::from(p.step) + i16::from(dir);
    let octave = p.octave + step.div_euclid(7) as i8;
    let step = step.rem_euclid(7) as u8;
    let alter = alter.unwrap_or_else(|| key_alter(a.bars[bar].key, step));
    Pitch {
        step,
        alter,
        octave,
    }
    .midi()
}

fn figure(marks: &[TokMark], p: Pitch, a: &Analysis, bar: usize) -> Option<Figure> {
    marks.iter().find_map(|m| match *m {
        TokMark::Ornament { kind, above, below } => Some(Figure::Ornament {
            kind,
            upper: neighbor(p, 1, above, a, bar),
            lower: neighbor(p, -1, below.or(above), a, bar),
        }),
        TokMark::Tremolo(n) => Some(Figure::Tremolo(n)),
        _ => None,
    })
}

struct Slot {
    on: f64,
    len: f64,
    downbeat: bool,
}

fn raw_note(
    a: &Analysis,
    r: NoteRef,
    n: &Note,
    marks: &[TokMark],
    slot: &Slot,
    legato: bool,
) -> Option<Raw> {
    if a.muted(r) {
        return None;
    }
    let leaf = &a.leaves[r.leaf];
    let offset = |want: fn(&PitchMark) -> Option<f64>| n.marks.iter().find_map(want).unwrap_or(0.0);
    let main = r.part == Part::Main;
    Some(Raw {
        on: slot.on,
        end: slot.on + slot.len,
        key: n.pitch.midi(),
        vel: a.velocity(r).note,
        lane: leaf.lane,
        tied: n.tied,
        tie_later: n.has(PitchMark::TieLater),
        attack: offset(|m| match m {
            PitchMark::Attack(v) => Some(*v),
            _ => None,
        }),
        release: offset(|m| match m {
            PitchMark::Release(v) => Some(*v),
            _ => None,
        }),
        gate: gate(marks, legato),
        figure: if main {
            figure(marks, n.pitch, a, leaf.bar)
        } else {
            None
        },
        arp: marks.iter().find_map(|m| match *m {
            TokMark::Arp(kind) => Some((
                kind,
                match kind {
                    ArpKind::CrossStaff => ArpGroup::CrossStaff,
                    _ => ArpGroup::Token(r.leaf),
                },
            )),
            _ => None,
        }),
        downbeat: slot.downbeat && main,
    })
}

fn leaf_notes(
    a: &Analysis,
    id: usize,
    on: f64,
    tempo: &TempoMap,
    legato: bool,
    out: &mut Vec<Raw>,
) {
    let leaf = &a.leaves[id];
    let Some(ev) = leaf.event() else { return };
    let len = ticks(leaf.dur);
    let downbeat = leaf.start == a.score.bars[leaf.bar].layout.lead;
    if let Some(g) = leaf
        .groups
        .iter()
        .map(|&g| &a.groups[g])
        .find(|g| matches!(g.head, GroupHead::Tremolo { .. }))
    {
        if g.first_leaf == id {
            two_note_tremolo(a, g.first_leaf, g.last_leaf, on, out);
        }
        return;
    }
    let unit = tempo.ticks(on, GRACE_MS);
    let mut main = Slot { on, len, downbeat };
    if let Some(g) = &ev.grace {
        let n = g.notes.len() as f64;
        let each = if g.notes.len() == 1 && !g.slash {
            len / 2.0
        } else {
            unit.min(len / (2.0 * n))
        };
        for (gi, gn) in g.notes.iter().enumerate() {
            let slot = Slot {
                on: on + gi as f64 * each,
                len: each,
                downbeat: false,
            };
            for (ni, note) in gn.notes.iter().enumerate() {
                let r = NoteRef {
                    leaf: id,
                    part: Part::Grace(gi),
                    note: ni,
                };
                out.extend(raw_note(a, r, note, &gn.marks, &slot, true));
            }
        }
        main.on += n * each;
        main.len -= n * each;
    }
    let after = ev.after.as_ref().map(|g| {
        let n = g.notes.len() as f64;
        let each = unit.min(main.len / (2.0 * n));
        main.len -= n * each;
        (g, each)
    });
    for (ni, note) in ev.notes().iter().enumerate() {
        let r = NoteRef {
            leaf: id,
            part: Part::Main,
            note: ni,
        };
        out.extend(raw_note(a, r, note, &ev.marks, &main, legato));
    }
    if let Some((g, each)) = after {
        for (gi, gn) in g.notes.iter().enumerate() {
            let slot = Slot {
                on: main.on + main.len + gi as f64 * each,
                len: each,
                downbeat: false,
            };
            for (ni, note) in gn.notes.iter().enumerate() {
                let r = NoteRef {
                    leaf: id,
                    part: Part::After(gi),
                    note: ni,
                };
                out.extend(raw_note(a, r, note, &gn.marks, &slot, true));
            }
        }
    }
}

// The two chords alternate for the full length of the group.
fn two_note_tremolo(a: &Analysis, first: usize, last: usize, on: f64, out: &mut Vec<Raw>) {
    let total = ticks(a.leaves[last].end() - a.leaves[first].start);
    let strokes = a.leaves[first]
        .groups
        .iter()
        .find_map(|&g| match a.groups[g].head {
            GroupHead::Tremolo { strokes } => Some(strokes),
            GroupHead::Tuplet { .. } => None,
        })
        .unwrap_or(1);
    let step = stroke_len(strokes).min(total / 2.0);
    let count = (total / step).round().max(2.0) as usize;
    let step = total / count as f64;
    for k in 0..count {
        let id = if k % 2 == 0 { first } else { last };
        let Some(ev) = a.leaves[id].event() else {
            continue;
        };
        let slot = Slot {
            on: on + k as f64 * step,
            len: step,
            downbeat: k == 0,
        };
        for (ni, note) in ev.notes().iter().enumerate() {
            let r = NoteRef {
                leaf: id,
                part: Part::Main,
                note: ni,
            };
            if let Some(mut raw) = raw_note(a, r, note, &ev.marks, &slot, false) {
                raw.tied = false;
                raw.figure = None;
                out.push(raw);
            }
        }
    }
}

// 1 stroke: eighths, 2: sixteenths, 3: 32nds.
fn stroke_len(strokes: u8) -> f64 {
    TICKS_PER_WHOLE / f64::from(1u32 << (u32::from(strokes) + 2).min(8))
}

fn join_ties(mut raw: Vec<Raw>) -> Vec<Raw> {
    raw.sort_by(|x, y| x.on.total_cmp(&y.on));
    let mut out: Vec<Raw> = Vec::with_capacity(raw.len());
    let mut later: HashMap<(Lane, i32), usize> = HashMap::new();
    for r in raw {
        if r.tied {
            let target = later.remove(&(r.lane, r.key)).or_else(|| {
                out.iter()
                    .rposition(|p| p.key == r.key && (p.end - r.on).abs() < 0.5)
            });
            if let Some(i) = target {
                let p = &mut out[i];
                p.end = r.end;
                p.release = r.release;
                p.gate = r.gate;
                if r.tie_later {
                    later.insert((r.lane, r.key), i);
                }
                continue;
            }
        }
        if r.tie_later {
            later.insert((r.lane, r.key), out.len());
        }
        out.push(r);
    }
    out
}

fn play_figure(r: Raw, tempo: &TempoMap) -> Vec<Raw> {
    let Some(fig) = r.figure else { return vec![r] };
    let len = r.end - r.on;
    let fast = tempo.ticks(r.on, ORNAMENT_MS);
    let main = r.key;
    // Each part is a key and its length; a length of 0 takes the rest of the note.
    let parts: Vec<(i32, f64)> = match fig {
        Figure::Tremolo(strokes) => {
            let step = stroke_len(strokes).min(len);
            let n = (len / step).round().max(1.0) as usize;
            vec![(main, len / n as f64); n]
        }
        Figure::Ornament { kind, upper, lower } => {
            let f = fast.min(len / 4.0);
            let quick = |keys: &[i32]| -> Vec<(i32, f64)> {
                let mut v: Vec<(i32, f64)> = keys.iter().map(|k| (*k, f)).collect();
                v.push((main, 0.0));
                v
            };
            match kind {
                OrnamentKind::Trill | OrnamentKind::TrillLine => {
                    let n = ((len / fast).round() as usize).max(2);
                    (0..n)
                        .map(|i| (if i % 2 == 0 { main } else { upper }, len / n as f64))
                        .collect()
                }
                OrnamentKind::Mordent => quick(&[main, lower]),
                OrnamentKind::Pralltriller => quick(&[main, upper]),
                OrnamentKind::LongMordent => quick(&[main, lower, main, lower]),
                OrnamentKind::LongPralltriller => quick(&[main, upper, main, upper]),
                OrnamentKind::Turn => quick(&[upper, main, lower]),
                OrnamentKind::InvertedTurn => quick(&[lower, main, upper]),
                OrnamentKind::DelayedTurn | OrnamentKind::DelayedInvertedTurn => {
                    let (x, y) = if kind == OrnamentKind::DelayedTurn {
                        (upper, lower)
                    } else {
                        (lower, upper)
                    };
                    let f = fast.min(len / 8.0);
                    vec![(main, len - 4.0 * f), (x, f), (main, f), (y, f), (main, f)]
                }
            }
        }
    };
    let used: f64 = parts.iter().map(|p| p.1).sum();
    let last = parts.len() - 1;
    let mut t = r.on;
    parts
        .into_iter()
        .enumerate()
        .map(|(i, (key, l))| {
            let l = if l == 0.0 { len - used } else { l };
            let mut p = r.clone();
            p.on = t;
            p.end = t + l;
            p.key = key;
            p.figure = None;
            if i > 0 {
                p.attack = 0.0;
                p.downbeat = false;
                p.arp = None;
            }
            if i < last {
                p.release = 0.0;
                p.gate = Gate::Legato;
            }
            t += l;
            p
        })
        .collect()
}

fn sounding_len(g: Gate, len: f64) -> f64 {
    match g {
        Gate::Normal => len - len.min(TICKS_PER_QUARTER) * 0.05,
        Gate::Legato => len,
        Gate::Portato => len * 0.75,
        Gate::Staccato => len * 0.5,
        Gate::Staccatissimo => len * 0.25,
    }
}

struct Timed {
    raw: Raw,
    on: f64,
    off: f64,
    vel: f64,
}

fn timed(raw: Vec<Raw>, tempo: &TempoMap, humanize: Humanize) -> Vec<Played> {
    let mut notes: Vec<Timed> = raw
        .into_iter()
        .map(|r| Timed {
            on: r.on + r.attack,
            off: r.on + sounding_len(r.gate, r.end - r.on) + r.release,
            vel: r.vel,
            raw: r,
        })
        .collect();
    if let Humanize::Seed(seed) = humanize {
        humanize_notes(&mut notes, tempo, seed);
    }
    spread_arpeggios(&mut notes, tempo);
    notes
        .into_iter()
        .map(|n| {
            let shortest = tempo.ticks(n.on, SHORTEST_MS);
            Played {
                on: n.on.max(0.0),
                off: n.off.max(n.on + shortest),
                key: n.raw.key.clamp(0, 127) as u8,
                velocity: n.vel.round().clamp(1.0, 127.0) as u8,
                staff: Analysis::home_staff(n.raw.lane),
            }
        })
        .collect()
}

// SplitMix64: a small generator that gives the same numbers for a seed on each platform.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }

    fn normal(&mut self, sd: f64, limit: f64) -> f64 {
        let u = self.next().max(f64::MIN_POSITIVE);
        let v = self.next();
        let z = (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos();
        (z * sd).clamp(-limit, limit)
    }
}

// Pianists play the notes of a chord close together, lead with the melody,
// stress the downbeat a little and connect legato notes.
fn humanize_notes(notes: &mut [Timed], tempo: &TempoMap, seed: u64) {
    let mut rng = Rng(seed);
    let mut chords: HashMap<(Staff, i64), Vec<usize>> = HashMap::new();
    for (i, n) in notes.iter().enumerate() {
        let staff = Analysis::home_staff(n.raw.lane);
        chords
            .entry((staff, n.raw.on.round() as i64))
            .or_default()
            .push(i);
    }
    let mut keys: Vec<(Staff, i64)> = chords.keys().copied().collect();
    keys.sort();
    for key in keys {
        let ids = &chords[&key];
        let at = notes[ids[0]].raw.on;
        let ms = |v: f64| tempo.ticks(at, v);
        let shift = ms(rng.normal(6.0, 18.0));
        let loud = rng.normal(2.0, 5.0);
        let top = ids.iter().copied().max_by_key(|&i| notes[i].raw.key);
        for &i in ids {
            let n = &mut notes[i];
            let lead = if key.0 == Staff::Upper && ids.len() > 1 && Some(i) == top {
                ms(-12.0)
            } else {
                0.0
            };
            n.on += shift + lead + ms(rng.normal(2.0, 6.0));
            let connect = if n.raw.gate == Gate::Legato {
                ms(20.0)
            } else {
                0.0
            };
            n.off += shift + connect + ms(rng.normal(10.0, 25.0));
            let stress = if n.raw.downbeat { 2.0 } else { 0.0 };
            n.vel += loud + stress + rng.normal(1.5, 4.0);
        }
    }
}

fn spread_arpeggios(notes: &mut [Timed], tempo: &TempoMap) {
    let mut groups: HashMap<(i64, usize), Vec<usize>> = HashMap::new();
    for (i, n) in notes.iter().enumerate() {
        let Some((_, g)) = n.raw.arp else { continue };
        let id = match g {
            ArpGroup::Token(leaf) => leaf,
            ArpGroup::CrossStaff => usize::MAX,
        };
        groups
            .entry((n.raw.on.round() as i64, id))
            .or_default()
            .push(i);
    }
    for mut ids in groups.into_values() {
        ids.sort_by_key(|&i| notes[i].raw.key);
        let down = ids
            .iter()
            .any(|&i| matches!(notes[i].raw.arp, Some((ArpKind::Down, _))));
        if down {
            ids.reverse();
        }
        let step = tempo.ticks(notes[ids[0]].raw.on, ARPEGGIO_MS);
        for (k, &i) in ids.iter().enumerate() {
            notes[i].on += k as f64 * step;
        }
    }
}

// One key cannot sound twice: a note ends just before the next note on its key.
fn remove_overlaps(notes: &mut Vec<Played>) {
    notes.sort_by(|x, y| x.key.cmp(&y.key).then(x.on.total_cmp(&y.on)));
    let mut out: Vec<Played> = Vec::with_capacity(notes.len());
    for n in notes.drain(..) {
        if let Some(p) = out.last_mut().filter(|p| p.key == n.key) {
            if n.on - p.on < SAME_STRIKE {
                p.off = p.off.max(n.off);
                p.velocity = p.velocity.max(n.velocity);
                continue;
            }
            p.off = p.off.min(n.on - 1.0);
        }
        out.push(n);
    }
    out.sort_by(|x, y| x.on.total_cmp(&y.on).then(x.key.cmp(&y.key)));
    *notes = out;
}

fn pedal(a: &Analysis, order: &[Instance], tempo: &TempoMap) -> Vec<(f64, u8)> {
    let mut out = Vec::new();
    let mut down = false;
    for (k, inst) in order.iter().enumerate() {
        let lead = a.score.bars[inst.bar].layout.lead;
        for p in a.pedals.iter().filter(|p| p.bar == inst.bar) {
            let t = inst.start + ticks(p.at - lead);
            match p.action {
                PedalAction::Start => out.push((t, 127)),
                PedalAction::Stop => out.push((t, 0)),
                PedalAction::Change => {
                    out.push((t, 0));
                    out.push((t + tempo.ticks(t, PEDAL_CHANGE_MS), 127));
                }
            }
            down = p.action != PedalAction::Stop;
        }
        let next_is_next_bar = order.get(k + 1).is_some_and(|n| n.bar == inst.bar + 1);
        if down && !next_is_next_bar {
            out.push((
                inst.start + ticks(a.score.bars[inst.bar].layout.length()),
                0,
            ));
            down = false;
        }
    }
    out.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
    out
}

fn meters(score: &Score, order: &[Instance]) -> Vec<(f64, u32, u32)> {
    let mut out: Vec<(f64, u32, u32)> = Vec::new();
    for inst in order {
        let m = &score.bars[inst.bar].layout.meter;
        let sig = (m.numerator(), m.beat_type);
        if out.last().is_none_or(|l| (l.1, l.2) != sig) {
            out.push((inst.start, sig.0, sig.1));
        }
    }
    out
}

fn channel_settings(score: &Score) -> (u8, Option<u8>, Option<u8>) {
    let mut program = 0;
    let mut volume = None;
    let mut pan = None;
    for attr in &score.header {
        match attr {
            // MusicXML numbers the programs from 1, MIDI from 0.
            Attr::Program(p) => program = p.saturating_sub(1).min(127) as u8,
            // MusicXML: volume 0 to 100, pan -90 (left) to 90 (right).
            Attr::Volume(v) => volume = Some((v * 127.0 / 100.0).round().clamp(0.0, 127.0) as u8),
            Attr::Pan(v) => pan = Some((64.0 + v * 63.0 / 90.0).round().clamp(0.0, 127.0) as u8),
            _ => {}
        }
    }
    (program, volume, pan)
}
