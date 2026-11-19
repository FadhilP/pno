use std::collections::{BTreeMap, HashMap, HashSet};

use crate::accidental::key_alter;
use crate::analysis::{Analysis, NoteRef, Part, PedalAction, flatten};
use crate::frac::Frac;
use crate::model::{
    ArpKind, Articulation, Attr, Bar, BarStyle, Event, GroupHead, Jump, Lane, MarkKind, Mode, Note,
    OrnamentKind, PhraseMark, Pitch, PitchMark, Score, Sign, Staff, TempoMark, Tok, TokMark,
};

pub const TICKS_PER_QUARTER: f64 = 480.0;
const TICKS_PER_WHOLE: f64 = 4.0 * TICKS_PER_QUARTER;
// MuseScore plays a score with no metronome mark at this tempo.
const DEFAULT_QPM: f64 = 120.0;
const FERMATA_STRETCH: f64 = 2.0;
// A breath mark, a caesura or a fermata on a barline holds the time before it: (ticks, stretch).
// The note before a breath mark or caesura ends where the hold starts, so a silence follows it.
const BREATH_HOLD: (f64, f64) = (TICKS_PER_QUARTER / 8.0, 2.0);
const CAESURA_HOLD: (f64, f64) = (TICKS_PER_QUARTER / 4.0, 4.0);
const BARLINE_HOLD: (f64, f64) = (TICKS_PER_QUARTER, 2.0);
// The tempo at the end of a "rit." or "accel.", as a part of the tempo before it.
const RIT: f64 = 0.75;
const ACCEL: f64 = 1.2;
const FINAL_RIT: f64 = 0.7;
// A tempo change is a curve of steps, one each 16th.
const CURVE_STEP: f64 = TICKS_PER_QUARTER / 4.0;
// With --phrase 1: the velocity change between the ends and the peak of a phrase, and how
// much slower the end of a phrase is.
const PHRASE_VEL: f64 = 6.0;
const PHRASE_TEMPO: f64 = 0.03;
const GRACE_MS: f64 = 70.0;
const ORNAMENT_MS: f64 = 70.0;
const ARPEGGIO_MS: f64 = 35.0;
const PEDAL_CHANGE_MS: f64 = 60.0;
const SHORTEST_MS: f64 = 20.0;
// The notes of an ornament after its first note are softer by this velocity.
const ORNAMENT_SOFTER: f64 = 10.0;
// With --humanize: the top note of `rh` at an onset with other notes comes this much earlier and louder.
const MELODY_LEAD_MS: f64 = 20.0;
const MELODY_VEL: f64 = 6.0;
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
    // The start of each played bar.
    pub bars: Vec<f64>,
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

// The release sound after the last note: only for audio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tail {
    None,
    Release,
}

pub struct Options {
    pub humanize: Humanize,
    pub shaping: Shaping,
    pub bars: Option<std::ops::RangeInclusive<usize>>,
    pub tail: Tail,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shaping {
    Off,
    // The amount: 1 is the default arch.
    Phrases(f64),
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

// The lanes of one staff roll together; a cross-staff roll is one line over the two staves.
#[derive(Clone, Copy)]
enum ArpGroup {
    Staff(Staff),
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
    // The part of the play order with no jump: a `^t` tie does not join notes of two parts.
    segment: usize,
}

struct Instance {
    bar: usize,
    start: f64,
}

// A time that a fermata or a pause holds longer: (from, to, stretch).
#[derive(Clone, Copy)]
struct Hold {
    from: f64,
    to: f64,
    stretch: f64,
}

#[derive(Clone)]
struct TempoMap {
    points: Vec<(f64, f64)>,
    holds: Vec<Hold>,
}

impl TempoMap {
    // The same tempo from `from`, which becomes 0.
    fn shifted(&self, from: f64) -> TempoMap {
        let first = (0.0, self.qpm(from));
        let later = self.points.iter().filter(|p| p.0 > from);
        TempoMap {
            points: std::iter::once(first)
                .chain(later.map(|&(t, q)| (t - from, q)))
                .collect(),
            holds: self
                .holds
                .iter()
                .filter(|h| h.to > from)
                .map(|h| Hold {
                    from: h.from - from,
                    to: h.to - from,
                    stretch: h.stretch,
                })
                .collect(),
        }
    }

    fn qpm(&self, t: f64) -> f64 {
        self.points
            .iter()
            .take_while(|(at, _)| *at <= t)
            .last()
            .or(self.points.first())
            .map_or(DEFAULT_QPM, |(_, q)| *q)
    }

    // Grace notes, rolls and ornaments keep their speed under a fermata.
    fn ticks(&self, t: f64, ms: f64) -> f64 {
        let stretch = self
            .holds
            .iter()
            .find(|h| h.from <= t && t < h.to)
            .map_or(1.0, |h| h.stretch);
        ms * self.qpm(t) * stretch * TICKS_PER_QUARTER / 60_000.0
    }
}

fn pause_hold(mark: PhraseMark) -> Option<(f64, f64)> {
    match mark {
        PhraseMark::Breath => Some(BREATH_HOLD),
        PhraseMark::Caesura => Some(CAESURA_HOLD),
        PhraseMark::Phrase | PhraseMark::Section => None,
    }
}

fn ticks(f: Frac) -> f64 {
    f.num() as f64 * TICKS_PER_WHOLE / f.den() as f64
}

fn leaves_by_bar(a: &Analysis) -> Vec<Vec<usize>> {
    let mut by_bar: Vec<Vec<usize>> = vec![Vec::new(); a.score.bars.len()];
    for (id, leaf) in a.leaves.iter().enumerate() {
        by_bar[leaf.bar].push(id);
    }
    by_bar
}

// The instances of the longest run of `bars` in the play order (the first of equal runs),
// from 0, and the time where the run starts.
fn window(
    all: &[Instance],
    bars: Option<&std::ops::RangeInclusive<usize>>,
) -> Result<(Vec<Instance>, f64), String> {
    let copy = |run: &[Instance], from: f64| -> Vec<Instance> {
        run.iter()
            .map(|i| Instance {
                bar: i.bar,
                start: i.start - from,
            })
            .collect()
    };
    let Some(range) = bars else {
        return Ok((copy(all, 0.0), 0.0));
    };
    let mut best: Option<(usize, usize)> = None;
    let mut k = 0;
    while k < all.len() {
        let len = all[k..]
            .iter()
            .take_while(|i| range.contains(&i.bar))
            .count();
        if len > best.map_or(0, |b| b.1) {
            best = Some((k, len));
        }
        k += len.max(1);
    }
    let (first, len) = best.ok_or("these bars are not played")?;
    let from = all[first].start;
    Ok((copy(&all[first..first + len], from), from))
}

pub fn perform(a: &Analysis, opts: &Options) -> Result<Performance, String> {
    let all = instances(a.score);
    let by_bar = leaves_by_bar(a);
    let arcs = match opts.shaping {
        Shaping::Off => None,
        Shaping::Phrases(amount) => Some(Arcs::new(a, &all, &by_bar, amount)),
    };
    let full_tempo = tempo_map(a, &all, &by_bar, opts.humanize, arcs.as_ref());
    let (order, from) = window(&all, opts.bars.as_ref())?;
    let tempo = full_tempo.shifted(from);
    let legato = slurred_leaves(a);
    let mut raw: Vec<Raw> = Vec::new();
    let mut segment = 0;
    for (k, inst) in order.iter().enumerate() {
        if k > 0 && order[k - 1].bar + 1 != inst.bar {
            segment += 1;
        }
        let lead = a.score.bars[inst.bar].layout.lead;
        let first = raw.len();
        for &id in &by_bar[inst.bar] {
            let at = inst.start + ticks(a.leaves[id].start - lead);
            let touch = if legato.contains(&id) {
                Touch::Legato
            } else {
                Touch::Plain
            };
            leaf_notes(a, id, at, &tempo, touch, &mut raw);
        }
        for r in &mut raw[first..] {
            r.segment = segment;
        }
    }
    if let Some(arcs) = &arcs {
        for r in &mut raw {
            r.vel += arcs.vel(r.on + from);
        }
    }
    let raw: Vec<Raw> = join_ties(raw)
        .into_iter()
        .flat_map(|r| play_figure(r, &tempo))
        .collect();
    let (mut notes, lead) = timed(raw, &tempo, opts.humanize);
    remove_overlaps(&mut notes);
    let (program, volume, pan) = channel_settings(a.score);
    let later = |t: f64| if t > 0.0 { t + lead } else { t };
    let pedal: Vec<(f64, u8)> = pedal(a, &order, &tempo)
        .into_iter()
        .map(|(t, v)| (t + lead, v))
        .collect();
    let last = notes
        .iter()
        .map(|n| n.off)
        .chain(pedal.iter().map(|p| p.0))
        .fold(0.0, f64::max);
    let tail = match opts.tail {
        Tail::None => 0.0,
        Tail::Release => tempo.ticks(last, TAIL_MS),
    };
    Ok(Performance {
        notes,
        pedal,
        end: last + tail,
        bars: order.iter().map(|i| i.start + lead).collect(),
        tempo: tempo.points.iter().map(|&(t, q)| (later(t), q)).collect(),
        meters: meters(a.score, &order)
            .into_iter()
            .map(|(t, n, d)| (later(t), n, d))
            .collect(),
        program,
        volume,
        pan,
    })
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
    Jumped(Until),
}

#[derive(Clone, Copy, PartialEq)]
enum Until {
    End,
    Fine,
    Coda,
}

fn volta(bar: &Bar) -> Option<&[u32]> {
    bar.attrs.iter().find_map(|a| match a {
        Attr::Volta { numbers, .. } => Some(numbers.as_slice()),
        _ => None,
    })
}

fn is_last_ending(bars: &[Bar], b: usize) -> bool {
    let in_volta = |x: &usize| volta(&bars[*x]).is_some();
    let run_end = (b..bars.len()).take_while(in_volta).last().unwrap_or(b);
    let run_start = (0..=b).rev().take_while(in_volta).last().unwrap_or(b);
    let top = (run_start..=run_end)
        .filter_map(|x| volta(&bars[x]))
        .flatten()
        .copied()
        .max()
        .unwrap_or(0);
    volta(&bars[b]).is_some_and(|n| n.contains(&top))
}

// The bars in the order they sound: repeats, voltas and jumps played out.
// After a jump the repeats are not played again and each volta plays its last ending.
fn play_order(score: &Score) -> Vec<usize> {
    let bars = &score.bars;
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
    let sign = |s: Sign| {
        bars.iter()
            .position(|bar| bar.attrs.contains(&Attr::Sign(s)))
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
        if let Some(numbers) = volta(&bars[b]) {
            let plays = match state {
                After::Repeats => numbers.contains(&pass),
                After::Jumped(_) => is_last_ending(bars, b),
            };
            if !plays {
                b += 1;
                continue;
            }
        }
        order.push(b);
        let j = jump(b);
        if let After::Jumped(until) = state {
            match (until, j) {
                (Until::Fine, Some(Jump::Fine)) => break,
                (Until::Coda, Some(Jump::ToCoda)) => {
                    match sign(Sign::Coda) {
                        Some(coda) => b = coda,
                        None => break,
                    }
                    state = After::Jumped(Until::End);
                    continue;
                }
                _ => {}
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
            let volta_ends = volta(&bars[b]).is_some() && bars.get(b + 1).and_then(volta).is_none();
            if volta_ends {
                pass = 1;
                start = b + 1;
            }
            let target = match j {
                Some(Jump::Dc | Jump::DcFine | Jump::DcCoda) => Some(0),
                Some(Jump::Ds | Jump::DsFine | Jump::DsCoda) => sign(Sign::Segno),
                _ => None,
            };
            if let Some(t) = target {
                let until = match j {
                    Some(Jump::DcFine | Jump::DsFine) => Until::Fine,
                    Some(Jump::DcCoda | Jump::DsCoda) => Until::Coda,
                    _ => Until::End,
                };
                state = After::Jumped(until);
                b = t;
                continue;
            }
        }
        b += 1;
    }
    order
}

fn tempo_map(
    a: &Analysis,
    order: &[Instance],
    by_bar: &[Vec<usize>],
    humanize: Humanize,
    arcs: Option<&Arcs>,
) -> TempoMap {
    let mut marks: Vec<(f64, f64)> = Vec::new();
    let mut holds: Vec<Hold> = Vec::new();
    let mut words: Vec<(f64, f64, TempoWord)> = Vec::new();
    for inst in order {
        let bar = &a.score.bars[inst.bar];
        let local = |t: Frac| inst.start + ticks(t - bar.layout.lead);
        let lines = bar
            .lines
            .iter()
            .filter(|l| matches!(l.lane, Lane::Tempo | Lane::Mark(_, MarkKind::Text, _)));
        for line in lines {
            for (start, _, tok) in flatten(&line.els) {
                let (text, ext) = match tok {
                    Tok::Tempo(t) => {
                        if let Some(q) = t.mark.as_ref().map(TempoMark::quarters_per_minute) {
                            marks.push((local(start), q));
                        }
                        (t.text.as_deref(), t.ext)
                    }
                    Tok::Text(t) => (Some(t.text.as_str()), t.ext),
                    _ => continue,
                };
                let Some(word) = text.and_then(tempo_word) else {
                    continue;
                };
                if let (TempoWord::First, Some(&(_, q))) = (word, marks.first()) {
                    marks.push((local(start), q));
                }
                let len = match a.spans.iter().find(|s| std::ptr::eq(s.tok, tok)) {
                    Some(s) if ext => a.abs(s.end.0, s.end.1) - a.abs(s.start.0, s.start.1),
                    _ => bar.layout.length(),
                };
                words.push((local(start), local(start) + ticks(len), word));
            }
        }
        holds.extend(bar_holds(a, inst, &by_bar[inst.bar]));
    }
    marks.sort_by(|x, y| x.0.total_cmp(&y.0));
    let base = |t: f64| {
        marks
            .iter()
            .take_while(|(at, _)| *at <= t)
            .last()
            .map_or(DEFAULT_QPM, |(_, q)| *q)
    };
    let ramps = tempo_ramps(a, order, by_bar, &marks, words, humanize);
    let mut cuts: Vec<f64> = marks.iter().map(|m| m.0).collect();
    cuts.extend(holds.iter().flat_map(|h| [h.from, h.to]));
    for r in &ramps {
        let steps = ((r.to - r.from) / CURVE_STEP).ceil() as u32;
        cuts.extend((0..=steps).map(|k| (r.from + f64::from(k) * CURVE_STEP).min(r.to)));
    }
    if let Some(arcs) = arcs {
        let steps = (arcs.length / CURVE_STEP).ceil() as u32;
        cuts.extend((0..steps).map(|k| f64::from(k) * CURVE_STEP));
    }
    cuts.push(0.0);
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let mut points: Vec<(f64, f64)> = Vec::new();
    for t in cuts {
        let stretch = holds
            .iter()
            .find(|h| h.from <= t && t < h.to)
            .map_or(1.0, |h| h.stretch);
        let arc = arcs.map_or(1.0, |p| p.tempo(t));
        let q = base(t) * ramp_factor(&ramps, t) * arc / stretch;
        if points.last().is_none_or(|p| p.1 != q) {
            points.push((t, q));
        }
    }
    TempoMap { points, holds }
}

fn bar_holds(a: &Analysis, inst: &Instance, leaves: &[usize]) -> Vec<Hold> {
    let bar = &a.score.bars[inst.bar];
    let local = |t: Frac| inst.start + ticks(t - bar.layout.lead);
    let mut holds = Vec::new();
    for &id in leaves {
        let leaf = &a.leaves[id];
        let held = leaf.event().is_some_and(|ev| {
            ev.marks
                .iter()
                .any(|m| matches!(m, TokMark::Fermata { .. }))
        });
        if held {
            holds.push(Hold {
                from: local(leaf.start),
                to: local(leaf.end()),
                stretch: FERMATA_STRETCH,
            });
        }
        if let Some((len, stretch)) = a.pauses.get(&id).and_then(|m| pause_hold(*m)) {
            let to = local(leaf.end());
            holds.push(Hold {
                from: to - len,
                to,
                stretch,
            });
        }
    }
    let barline_fermata = bar
        .attrs
        .iter()
        .any(|at| matches!(at, Attr::Barline { fermata: true, .. }));
    if barline_fermata {
        let to = inst.start + ticks(bar.layout.length());
        holds.push(Hold {
            from: to - BARLINE_HOLD.0,
            to,
            stretch: BARLINE_HOLD.1,
        });
    }
    holds
}

#[derive(Clone, Copy)]
enum TempoWord {
    Change(f64),
    Back,
    // "Tempo I": back to the first metronome mark.
    First,
}

fn tempo_word(text: &str) -> Option<TempoWord> {
    let t = text.to_lowercase();
    let words: Vec<&str> = t
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let pair = |x: &str, y: &[&str]| words.windows(2).any(|w| w[0] == x && y.contains(&w[1]));
    let has = |w: &str| words.contains(&w);
    let starts = |prefixes: &[&str]| {
        words
            .iter()
            .any(|w| prefixes.iter().any(|p| w.starts_with(p)))
    };
    if pair("tempo", &["i", "1", "primo"]) {
        return Some(TempoWord::First);
    }
    if pair("a", &["tempo"]) {
        return Some(TempoWord::Back);
    }
    let strength = if has("poco") {
        0.5
    } else if has("molto") {
        1.6
    } else {
        1.0
    };
    if has("rit") || starts(&["ritard", "riten", "rall", "allarg", "slent"]) {
        return Some(TempoWord::Change(1.0 - (1.0 - RIT) * strength));
    }
    if starts(&["accel", "string"]) {
        return Some(TempoWord::Change(1.0 + (ACCEL - 1.0) * strength));
    }
    None
}

// From `from` to `to` the tempo changes on a curve; `Back` returns to the written tempo.
struct Ramp {
    from: f64,
    to: f64,
    change: TempoWord,
}

// The tempo words, and with --humanize a final ritardando over the two bars before the last
// onset if no slower change is in effect there. A metronome mark or "a tempo" ends a change.
fn tempo_ramps(
    a: &Analysis,
    order: &[Instance],
    by_bar: &[Vec<usize>],
    marks: &[(f64, f64)],
    mut words: Vec<(f64, f64, TempoWord)>,
    humanize: Humanize,
) -> Vec<Ramp> {
    words.sort_by(|x, y| x.0.total_cmp(&y.0));
    words.dedup_by(|x, y| (x.0 - y.0).abs() < 1.0);
    let mut backs: Vec<f64> = marks.iter().map(|m| m.0).collect();
    backs.extend(
        words
            .iter()
            .filter(|w| matches!(w.2, TempoWord::Back | TempoWord::First))
            .map(|w| w.0),
    );
    let mut ramps: Vec<Ramp> = Vec::new();
    for (from, to, change) in words {
        if let TempoWord::Change(_) = change {
            let to = backs
                .iter()
                .copied()
                .filter(|b| *b > from)
                .fold(to, f64::min);
            ramps.push(Ramp { from, to, change });
        }
    }
    for &b in &backs {
        ramps.push(Ramp {
            from: b,
            to: b,
            change: TempoWord::Back,
        });
    }
    ramps.sort_by(|x, y| x.from.total_cmp(&y.from));
    let last = match humanize {
        Humanize::Off => None,
        Humanize::Seed(_) => last_onset(a, order, by_bar),
    };
    if let Some((last, bar)) = last {
        let two_bars = (last - 2.0 * ticks(a.score.bars[bar].layout.length())).max(0.0);
        let from = backs
            .iter()
            .copied()
            .filter(|b| *b < last)
            .fold(two_bars, f64::max);
        let slower_starts = ramps.iter().any(|r| {
            let slower = matches!(r.change, TempoWord::Change(f) if f < 1.0);
            slower && from <= r.from && r.from < last
        });
        if ramp_factor(&ramps, from) >= 1.0 && !slower_starts {
            ramps.push(Ramp {
                from,
                to: last,
                change: TempoWord::Change(FINAL_RIT),
            });
            ramps.sort_by(|x, y| x.from.total_cmp(&y.from));
        }
    }
    ramps
}

fn last_onset(a: &Analysis, order: &[Instance], by_bar: &[Vec<usize>]) -> Option<(f64, usize)> {
    order.iter().rev().find_map(|inst| {
        let lead = a.score.bars[inst.bar].layout.lead;
        by_bar[inst.bar]
            .iter()
            .map(|&id| &a.leaves[id])
            .filter(|l| {
                l.event()
                    .is_some_and(|ev| ev.notes().iter().any(|n| !n.tied))
            })
            .map(|l| inst.start + ticks(l.start - lead))
            .max_by(f64::total_cmp)
            .map(|t| (t, inst.bar))
    })
}

// Friberg and Sundberg's curve of a final ritardando; the other changes use it too.
// A slower change during a slower change does not multiply: the lower factor applies.
fn ramp_factor(ramps: &[Ramp], t: f64) -> f64 {
    let mut f: f64 = 1.0;
    for r in ramps.iter().take_while(|r| r.from <= t) {
        match r.change {
            TempoWord::Back | TempoWord::First => f = 1.0,
            TempoWord::Change(m) => {
                let x = if r.to > r.from {
                    ((t - r.from) / (r.to - r.from)).min(1.0)
                } else {
                    1.0
                };
                let g = (1.0 + (m.powi(3) - 1.0) * x).cbrt();
                let both_slower = f < 1.0 && m < 1.0;
                f = if both_slower { f.min(g) } else { f * g };
            }
        }
    }
    f
}

// The phrases of the score, in ticks of score time (repeats not played out).
struct Arcs {
    phrases: Vec<(f64, f64)>,
    // Empty if the score has no section start.
    sections: Vec<(f64, f64)>,
    // The start of each played bar: (performance tick, score tick).
    starts: Vec<(f64, f64)>,
    length: f64,
    amount: f64,
}

impl Arcs {
    fn new(a: &Analysis, order: &[Instance], by_bar: &[Vec<usize>], amount: f64) -> Arcs {
        let starts: Vec<(f64, f64)> = order
            .iter()
            .map(|i| (i.start, ticks(a.bars[i.bar].start)))
            .collect();
        let length = order.last().map_or(0.0, |i| {
            i.start + ticks(a.score.bars[i.bar].layout.length())
        });
        let mut marks = phrase_marks(a);
        if marks.is_empty() {
            marks = find_phrases(a, by_bar)
                .into_iter()
                .map(|p| (p.at, p.mark))
                .collect();
        }
        let sections: Vec<f64> = marks
            .iter()
            .filter(|m| m.1 == PhraseMark::Section)
            .map(|m| m.0)
            .collect();
        let total = score_length(a);
        Arcs {
            phrases: between(marks.iter().map(|m| m.0).collect(), total),
            sections: if sections.is_empty() {
                Vec::new()
            } else {
                between(sections, total)
            },
            starts,
            length,
            amount,
        }
    }

    // Where `t` is in its phrase and its section, from 0 to 1.
    fn places(&self, t: f64) -> (Option<f64>, Option<f64>) {
        let k = self.starts.partition_point(|s| s.0 <= t).saturating_sub(1);
        let Some((perf, score)) = self.starts.get(k) else {
            return (None, None);
        };
        let pos = score + (t - perf);
        (place(&self.phrases, pos), place(&self.sections, pos))
    }

    // A section arc adds half of its value to the phrase arc, as in Todd's model.
    fn vel(&self, t: f64) -> f64 {
        let (phrase, section) = self.places(t);
        let v = phrase.map_or(0.0, arc) + 0.5 * section.map_or(0.0, arc);
        PHRASE_VEL * self.amount * v
    }

    fn tempo(&self, t: f64) -> f64 {
        let (phrase, section) = self.places(t);
        let v = phrase.map_or(0.0, tempo_arc) + 0.5 * section.map_or(0.0, tempo_arc);
        1.0 + PHRASE_TEMPO * self.amount * v
    }
}

fn place(spans: &[(f64, f64)], pos: f64) -> Option<f64> {
    let (from, to) = spans.iter().find(|(f, e)| *f <= pos && pos < *e)?;
    Some((pos - from) / (to - from))
}

const ARC_PEAK: f64 = 0.6;

// Todd's phrase arc, from -1 at the ends of a phrase to 1 at 60 % of its length.
fn arc(x: f64) -> f64 {
    let p = 0.5f64.ln() / ARC_PEAK.ln();
    (std::f64::consts::PI * x.powf(p)).sin() * 2.0 - 1.0
}

// A phrase starts a little slow, is a little fast at its peak, and slows most at its end:
// from -1/3 to 1/3, then to -1.
fn tempo_arc(x: f64) -> f64 {
    let v = arc(x);
    if x < ARC_PEAK {
        v / 3.0
    } else {
        (2.0 * v - 1.0) / 3.0
    }
}

// The start of the score also starts a phrase.
fn between(mut starts: Vec<f64>, total: f64) -> Vec<(f64, f64)> {
    starts.push(0.0);
    starts.push(total);
    starts.sort_by(f64::total_cmp);
    starts.dedup();
    starts.windows(2).map(|w| (w[0], w[1])).collect()
}

fn phrase_marks(a: &Analysis) -> Vec<(f64, PhraseMark)> {
    let mut out = Vec::new();
    for (b, bar) in a.score.bars.iter().enumerate() {
        for line in bar.lines.iter().filter(|l| l.lane == Lane::Phrase) {
            for (start, _, tok) in flatten(&line.els) {
                if let Tok::Phrase(m) = tok {
                    out.push((ticks(a.abs(b, start)), *m));
                }
            }
        }
    }
    out
}

fn score_length(a: &Analysis) -> f64 {
    a.bars
        .last()
        .zip(a.score.bars.last())
        .map_or(0.0, |(info, bar)| ticks(info.start + bar.layout.length()))
}

// Why the detection starts a phrase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Start,
    Slur,
    AfterSlur,
    Section,
    Rest,
    Theme,
    LongNote,
    Tonic,
    Length,
}

impl Reason {
    pub fn text(self) -> &'static str {
        match self {
            Reason::Start => "the start of the score",
            Reason::Slur => "a slur of two bars or longer",
            Reason::AfterSlur => "the end of a long slur",
            Reason::Section => "a section ends",
            Reason::Rest => "a rest in rh",
            Reason::Theme => "an earlier theme comes back",
            Reason::LongNote => "a long note in rh",
            Reason::Tonic => "the bass on the tonic",
            Reason::Length => "four bars with no stronger end",
        }
    }
}

pub struct DetectedPhrase {
    pub bar: usize,
    pub time: Frac,
    pub mark: PhraseMark,
    pub reason: Reason,
}

struct PhraseStart {
    at: f64,
    mark: PhraseMark,
    reason: Reason,
}

// The phrase starts that --phrase finds, in the bar and at the time where they are.
pub fn detected_phrases(a: &Analysis) -> Vec<DetectedPhrase> {
    let by_bar = leaves_by_bar(a);
    let mut out: Vec<DetectedPhrase> = Vec::new();
    for p in find_phrases(a, &by_bar) {
        let Some(b) = (0..a.bars.len())
            .rev()
            .find(|&b| ticks(a.bars[b].start) <= p.at + 0.5)
        else {
            continue;
        };
        let lead = a.score.bars[b].layout.lead;
        let times = std::iter::once(lead).chain(by_bar[b].iter().map(|&id| a.leaves[id].start));
        let found = times
            .into_iter()
            .find(|&t| (ticks(a.abs(b, t)) - p.at).abs() < 0.5);
        let Some(time) = found else { continue };
        if out.last().is_some_and(|l| l.bar == b && l.time == time) {
            continue;
        }
        out.push(DetectedPhrase {
            bar: b,
            time,
            mark: p.mark,
            reason: p.reason,
        });
    }
    out
}

const TOP: Lane = Lane::Voice(Staff::Upper, 1);
// The cost of each phrase when the bars are cut: a higher cost gives fewer, longer phrases.
const PHRASE_COST: f64 = 2.0;
// The end of a range of bars (a long slur or the end of the score) ends a phrase as a
// section does.
const RANGE_END: f64 = 3.0;

type Leaf<'b, 'a> = &'b crate::analysis::Leaf<'a>;

// Slurs of two bars or longer in `rh` are phrases; shorter slurs group a motif. The time
// between them is cut into groups of 2 to 8 bars, 4 if no boundary is stronger.
fn find_phrases(a: &Analysis, by_bar: &[Vec<usize>]) -> Vec<PhraseStart> {
    let cues = all_cues(a, by_bar);
    let at = |leaf: usize, end: bool| {
        let l = &a.leaves[leaf];
        ticks(a.abs(l.bar, if end { l.end() } else { l.start }))
    };
    let mut slurs: Vec<(f64, f64)> = a
        .slurs
        .iter()
        .filter(|s| a.leaves[s.start.leaf].lane == TOP)
        .map(|s| (at(s.start.leaf, false), at(s.end.leaf, true), s.start.leaf))
        .filter(|(f, t, leaf)| t - f >= 2.0 * ticks(a.score.bars[a.leaves[*leaf].bar].layout.full))
        .map(|(f, t, _)| (f, t))
        .collect();
    slurs.sort_by(|x, y| x.0.total_cmp(&y.0));
    let section_at = |t: f64| {
        (1..a.bars.len())
            .find(|&b| (ticks(a.bars[b].start) - t).abs() < 0.5)
            .is_some_and(|b| cues[b].section)
    };
    let start = |at: f64, reason: Reason| PhraseStart {
        at,
        mark: if section_at(at) {
            PhraseMark::Section
        } else {
            PhraseMark::Phrase
        },
        reason,
    };
    let mut out: Vec<PhraseStart> = Vec::new();
    let mut cur = 0.0;
    let mut reason = Reason::Start;
    for (f, t) in slurs {
        if f < cur {
            continue;
        }
        if f > cur {
            out.push(start(cur, reason));
            out.extend(group_bars(a, &cues, cur, f));
        }
        out.push(start(f, Reason::Slur));
        cur = t;
        reason = Reason::AfterSlur;
    }
    let total = score_length(a);
    if cur < total {
        out.push(start(cur, reason));
        out.extend(group_bars(a, &cues, cur, total));
    }
    out
}

// The phrase starts after `from` and before `to`.
fn group_bars(a: &Analysis, cues: &[Cues], from: f64, to: f64) -> Vec<PhraseStart> {
    let start = |b: usize| ticks(a.bars[b].start);
    let mut k = (0..a.bars.len())
        .rev()
        .find(|&b| start(b) <= from)
        .unwrap_or(0);
    let has_next = k + 1 < a.bars.len() && start(k + 1) < to;
    if has_next && a.score.bars[k].layout.is_partial() {
        k += 1;
    }
    let mut out = Vec::new();
    let mut cur = from;
    for b in best_cuts(a, cues, k, to) {
        let c = &cues[b];
        if c.start <= cur {
            continue;
        }
        out.push(PhraseStart {
            at: c.start,
            mark: c.mark(),
            reason: c.reason(),
        });
        cur = c.start;
    }
    out
}

// The cut bars after bar `k` that give the best total: the strength of each phrase end,
// less half a point for each bar that the phrase is longer or shorter than 4, less the cost.
fn best_cuts(a: &Analysis, cues: &[Cues], k: usize, to: f64) -> Vec<usize> {
    let n = (k..a.bars.len())
        .take_while(|&b| ticks(a.bars[b].start) < to)
        .count();
    let mut best: Vec<Option<(f64, usize)>> = vec![None; n + 1];
    best[0] = Some((0.0, 0));
    for j in 2..=n {
        let end = if j == n {
            RANGE_END
        } else {
            cues[k + j].score()
        };
        for len in 2..=8.min(j) {
            let Some((before, _)) = best[j - len] else {
                continue;
            };
            let total = before + end - 0.5 * (len as f64 - 4.0).abs() - PHRASE_COST;
            if best[j].is_none_or(|(t, _)| total >= t) {
                best[j] = Some((total, j - len));
            }
        }
    }
    let mut out = Vec::new();
    let mut j = n;
    while j > 0 {
        let Some((_, i)) = best[j] else {
            break;
        };
        if j != n {
            out.push(k + j);
        }
        j = i;
    }
    out.reverse();
    out
}

// What ends a phrase before bar `b`, and where the next phrase starts.
#[derive(Default)]
struct Cues {
    section: bool,
    rest: bool,
    long: bool,
    tonic: bool,
    theme: bool,
    slur_over: bool,
    start: f64,
}

impl Cues {
    fn score(&self) -> f64 {
        let weight = |on: bool, w: f64| if on { w } else { 0.0 };
        weight(self.section, 3.0)
            + weight(self.rest, 2.0)
            + weight(self.theme, 2.0)
            + weight(self.long, 1.0)
            + weight(self.tonic, 1.0)
            - weight(self.slur_over, 2.0)
    }

    fn reason(&self) -> Reason {
        if self.section {
            Reason::Section
        } else if self.rest {
            Reason::Rest
        } else if self.theme {
            Reason::Theme
        } else if self.long {
            Reason::LongNote
        } else if self.tonic {
            Reason::Tonic
        } else {
            Reason::Length
        }
    }

    fn mark(&self) -> PhraseMark {
        if self.section {
            PhraseMark::Section
        } else {
            PhraseMark::Phrase
        }
    }
}

// The cues of each bar; bar 0 has none.
fn all_cues(a: &Analysis, by_bar: &[Vec<usize>]) -> Vec<Cues> {
    let themes: Vec<Vec<Vec<i32>>> = (0..a.bars.len())
        .map(|b| theme_signature(a, by_bar, b))
        .collect();
    std::iter::once(Cues::default())
        .chain((1..a.bars.len()).map(|b| cues(a, by_bar, &themes, b)))
        .collect()
}

fn cues(a: &Analysis, by_bar: &[Vec<usize>], themes: &[Vec<Vec<i32>>], b: usize) -> Cues {
    let prev = b - 1;
    let bars = &a.score.bars;
    let ends_section = bars[prev].attrs.iter().any(|at| {
        matches!(
            at,
            Attr::Repeat { end: Some(_), .. }
                | Attr::Jump(_)
                | Attr::Barline {
                    style: BarStyle::Double | BarStyle::Final,
                    ..
                }
        )
    });
    let starts_section = bars[b]
        .attrs
        .iter()
        .any(|at| matches!(at, Attr::Repeat { start: true, .. } | Attr::Sign(_)));
    let up = upbeat(a, by_bar, b);
    let cut = match up {
        Some(Upbeat::Cut(t)) => Some(t),
        _ => None,
    };
    let before = top_leaves(a, by_bar, prev);
    let last = before.iter().rfind(|l| cut.is_none_or(|t| l.start < t));
    let first = match cut {
        Some(_) => None,
        None => top_leaves(a, by_bar, b).first().copied(),
    };
    Cues {
        section: ends_section || starts_section || volta(&bars[prev]) != volta(&bars[b]),
        rest: last.is_some_and(|l| is_rest(l)) || first.is_some_and(is_rest),
        long: last.is_some_and(|l| !is_rest(l) && l.dur * 2 >= bars[prev].layout.length()),
        tonic: bass_on_tonic(a, by_bar, prev),
        theme: theme_returns(themes, b),
        slur_over: slur_over(a, b, up.as_ref().map(Upbeat::at)),
        start: up.map_or(ticks(a.bars[b].start), |u| ticks(a.abs(prev, u.at()))),
    }
}

fn top_leaves<'b, 'a>(a: &'b Analysis<'a>, by_bar: &[Vec<usize>], b: usize) -> Vec<Leaf<'b, 'a>> {
    let mut top: Vec<Leaf> = by_bar[b]
        .iter()
        .map(|&id| &a.leaves[id])
        .filter(|l| l.lane == TOP)
        .collect();
    top.sort_by_key(|l| l.start);
    top
}

fn is_note(l: Leaf) -> bool {
    l.event().is_some_and(|ev| !ev.notes().is_empty())
}

fn is_rest(l: Leaf) -> bool {
    l.event().is_some_and(|ev| ev.notes().is_empty())
}

// The start of an upbeat in the bar before a phrase. After a slur start or a rest it is
// also where the phrase before ends; after a long note it only moves the start.
enum Upbeat {
    Cut(Frac),
    StartOnly(Frac),
}

impl Upbeat {
    fn at(&self) -> Frac {
        match self {
            Upbeat::Cut(t) | Upbeat::StartOnly(t) => *t,
        }
    }
}

// The upbeat to bar `b` in the second half of the bar before, if `rh` ends that bar with
// a note: a slur that starts there and goes into bar `b`, the notes after the last rest, or
// the notes after a note at least twice as long as them.
fn upbeat(a: &Analysis, by_bar: &[Vec<usize>], b: usize) -> Option<Upbeat> {
    let prev = b - 1;
    let layout = &a.score.bars[prev].layout;
    let end = layout.lead + layout.length();
    let half = layout.lead + layout.length() / 2;
    let top = top_leaves(a, by_bar, prev);
    if !top.last().is_some_and(|l| is_note(l)) {
        return None;
    }
    let slur = a
        .slurs
        .iter()
        .filter_map(|s| {
            let from = &a.leaves[s.start.leaf];
            let into_b = a.leaves[s.end.leaf].bar >= b;
            let late = from.bar == prev && from.start > half;
            (from.lane == TOP && late && into_b).then_some(from.start)
        })
        .min();
    if let Some(t) = slur {
        return Some(Upbeat::Cut(t));
    }
    if let Some(rest) = top.iter().rfind(|l| is_rest(l)) {
        let t = rest.end();
        return (half <= t && t < end).then_some(Upbeat::Cut(t));
    }
    top.iter()
        .find(|l| {
            let in_second_half = l.end() > half && l.end() < end;
            in_second_half && l.dur >= (end - l.end()) * 2
        })
        .map(|l| Upbeat::StartOnly(l.end()))
}

// A slur of `rh` starts before the cut and ends after the bar line before bar `b`.
fn slur_over(a: &Analysis, b: usize, upbeat: Option<Frac>) -> bool {
    let line = a.bars[b].start;
    let cut = upbeat.map_or(line, |t| a.abs(b - 1, t));
    a.slurs.iter().any(|s| {
        let from = &a.leaves[s.start.leaf];
        let to = &a.leaves[s.end.leaf];
        let over = a.abs(from.bar, from.start) < cut && a.abs(to.bar, to.start) >= line;
        from.lane == TOP && over
    })
}

// The pitches of `rh` at the start of each cell of bar `b`; a rest has none.
fn theme_signature(a: &Analysis, by_bar: &[Vec<usize>], b: usize) -> Vec<Vec<i32>> {
    let top = top_leaves(a, by_bar, b);
    a.score.bars[b]
        .layout
        .cell_starts()
        .into_iter()
        .map(|t| {
            top.iter()
                .find(|l| l.start <= t && t < l.end())
                .and_then(|l| l.event())
                .map(|ev| ev.notes().iter().map(|n| n.pitch.midi()).collect())
                .unwrap_or_default()
        })
        .collect()
}

// At least 3 of 4 cells share a pitch, or both are rests. Exact equality misses a theme
// that comes back with one note added.
fn similar(x: &[Vec<i32>], y: &[Vec<i32>]) -> bool {
    if x.len() != y.len() {
        return false;
    }
    let same = |p: &[i32], q: &[i32]| {
        let both_rests = p.is_empty() && q.is_empty();
        both_rests || p.iter().any(|m| q.contains(m))
    };
    let common = x.iter().zip(y).filter(|(p, q)| same(p, q)).count();
    common * 4 >= x.len() * 3
}

// Bars `b` and `b + 1` repeat two earlier bars, and the bar before `b` does not repeat
// the bar before them.
fn theme_returns(themes: &[Vec<Vec<i32>>], b: usize) -> bool {
    if b + 1 >= themes.len() {
        return false;
    }
    if themes[b].iter().filter(|s| !s.is_empty()).count() < 2 {
        return false;
    }
    (0..b.saturating_sub(1)).any(|e| {
        let new_start = e == 0 || !similar(&themes[e - 1], &themes[b - 1]);
        similar(&themes[e], &themes[b]) && similar(&themes[e + 1], &themes[b + 1]) && new_start
    })
}

// The lowest note of the lower staff that sounds in the last cell of the bar is the tonic.
fn bass_on_tonic(a: &Analysis, by_bar: &[Vec<usize>], bar: usize) -> bool {
    let layout = &a.score.bars[bar].layout;
    let last_cell = layout.cell_starts().last().copied().unwrap_or(layout.lead);
    let bass = by_bar[bar]
        .iter()
        .map(|&id| &a.leaves[id])
        .filter(|l| l.lane.staff() == Some(Staff::Lower))
        .filter(|l| l.end() > last_cell)
        .flat_map(|l| l.event().map(Event::notes).unwrap_or_default())
        .map(|n| n.pitch.midi())
        .min();
    let key = a.bars[bar].key;
    let major = (i32::from(key.fifths) * 7).rem_euclid(12);
    let tonic = match key.mode {
        Some(Mode::Minor) => (major + 9) % 12,
        _ => major,
    };
    bass.is_some_and(|m| m.rem_euclid(12) == tonic)
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

fn gate(marks: &[TokMark], touch: Touch) -> Gate {
    let has = |art| marks.contains(&TokMark::Art(art));
    let legato = touch == Touch::Legato;
    let mezzo = has(Articulation::Tenuto) || legato;
    if has(Articulation::Staccato) && mezzo {
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
    let octaves = step.div_euclid(7);
    let octave =
        p.octave + i8::try_from(octaves).expect("a step from 0..7 moves at most one octave");
    let step = u8::try_from(step.rem_euclid(7)).expect("rem_euclid(7) is in 0..7");
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
        TokMark::Ornament { kind, above, below } => {
            // The one sign of a mordent is for its lower note.
            let mordent = matches!(kind, OrnamentKind::Mordent | OrnamentKind::LongMordent);
            let lower = if mordent { below.or(above) } else { below };
            Some(Figure::Ornament {
                kind,
                upper: neighbor(p, 1, above, a, bar),
                lower: neighbor(p, -1, lower, a, bar),
            })
        }
        TokMark::Tremolo(n) => Some(Figure::Tremolo(n)),
        _ => None,
    })
}

struct Slot {
    on: f64,
    len: f64,
    downbeat: bool,
}

// Notes under a slur, and grace notes, play legato.
#[derive(Clone, Copy, PartialEq)]
enum Touch {
    Legato,
    Plain,
}

fn raw_note(
    a: &Analysis,
    r: NoteRef,
    n: &Note,
    marks: &[TokMark],
    slot: &Slot,
    touch: Touch,
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
        gate: gate(marks, touch),
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
                    _ => ArpGroup::Staff(Analysis::home_staff(leaf.lane)),
                },
            )),
            _ => None,
        }),
        downbeat: slot.downbeat && main,
        segment: 0,
    })
}

fn leaf_notes(
    a: &Analysis,
    id: usize,
    on: f64,
    tempo: &TempoMap,
    touch: Touch,
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
        // One grace note with no slash is an appoggiatura: on the beat, with half the value.
        // The other grace notes end on the beat.
        let appoggiatura = g.notes.len() == 1 && !g.slash;
        let each = if appoggiatura {
            (len / 2.0).min(TICKS_PER_QUARTER)
        } else {
            unit.min(TICKS_PER_QUARTER / n)
        };
        let first = if appoggiatura { on } else { on - n * each };
        for (gi, gn) in g.notes.iter().enumerate() {
            let slot = Slot {
                on: first + gi as f64 * each,
                len: each,
                downbeat: false,
            };
            for (ni, note) in gn.notes.iter().enumerate() {
                let r = NoteRef {
                    leaf: id,
                    part: Part::Grace(gi),
                    note: ni,
                };
                out.extend(raw_note(a, r, note, &gn.marks, &slot, Touch::Legato));
            }
        }
        if appoggiatura {
            main.on += each;
            main.len -= each;
        }
    }
    if let Some((hold, _)) = a.pauses.get(&id).and_then(|m| pause_hold(*m)) {
        main.len -= hold.min(main.len / 2.0);
    }
    let after = ev.after.as_ref().map(|g| {
        let n = g.notes.len() as f64;
        let each = unit.min(main.len / (2.0 * n));
        main.len -= n * each;
        (g, each)
    });
    // An ornament on a chord is on its top note.
    let top = (0..ev.notes().len()).max_by_key(|&i| ev.notes()[i].pitch.midi());
    for (ni, note) in ev.notes().iter().enumerate() {
        let r = NoteRef {
            leaf: id,
            part: Part::Main,
            note: ni,
        };
        let Some(mut raw) = raw_note(a, r, note, &ev.marks, &main, touch) else {
            continue;
        };
        let ornament = matches!(raw.figure, Some(Figure::Ornament { .. }));
        if ornament && Some(ni) != top {
            raw.figure = None;
        }
        out.push(raw);
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
                out.extend(raw_note(a, r, note, &gn.marks, &slot, Touch::Legato));
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
    let count = (total / step).round().max(2.0) as u32;
    let step = total / f64::from(count);
    for k in 0..count {
        let id = if k % 2 == 0 { first } else { last };
        let Some(ev) = a.leaves[id].event() else {
            continue;
        };
        let slot = Slot {
            on: on + f64::from(k) * step,
            len: step,
            downbeat: k == 0,
        };
        for (ni, note) in ev.notes().iter().enumerate() {
            let r = NoteRef {
                leaf: id,
                part: Part::Main,
                note: ni,
            };
            if let Some(mut raw) = raw_note(a, r, note, &ev.marks, &slot, Touch::Plain) {
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
            let target = later
                .remove(&(r.lane, r.key))
                .filter(|&i| out[i].segment == r.segment)
                .or_else(|| {
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
            let n = (len / step).round().max(1.0);
            vec![(main, len / n); n as usize]
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
                    // An odd count ends the trill on the main note.
                    let n = ((len / fast).round() as u32).max(3);
                    let n = if n % 2 == 0 { n - 1 } else { n };
                    (0..n)
                        .map(|i| (if i % 2 == 0 { main } else { upper }, len / f64::from(n)))
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
    let ornament = matches!(fig, Figure::Ornament { .. });
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
            if i > 0 && ornament {
                p.vel -= ORNAMENT_SOFTER;
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

// Grace notes and arpeggios can start before the first beat; then all of the
// performance starts later by `lead`.
fn timed(raw: Vec<Raw>, tempo: &TempoMap, humanize: Humanize) -> (Vec<Played>, f64) {
    let mut notes: Vec<Timed> = raw
        .into_iter()
        .map(|r| Timed {
            on: r.on + r.attack,
            off: r.on + sounding_len(r.gate, r.end - r.on) + r.release,
            vel: r.vel,
            raw: r,
        })
        .collect();
    spread_arpeggios(&mut notes, tempo);
    let lead = -notes.iter().map(|n| n.on).fold(0.0, f64::min);
    for n in &mut notes {
        n.on += lead;
        n.off += lead;
    }
    if let Humanize::Seed(seed) = humanize {
        humanize_notes(&mut notes, tempo, seed);
    }
    let played = notes
        .into_iter()
        .map(|n| {
            let shortest = tempo.ticks(n.on, SHORTEST_MS);
            Played {
                on: n.on.max(0.0),
                off: n.off.max(n.on + shortest),
                key: u8::try_from(n.raw.key.clamp(0, 127)).expect("clamped to 0..=127"),
                velocity: n.vel.round().clamp(1.0, 127.0) as u8,
                staff: Analysis::home_staff(n.raw.lane),
            }
        })
        .collect();
    (played, lead)
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
    let mut chords: BTreeMap<(Staff, i64), Vec<usize>> = BTreeMap::new();
    for (i, n) in notes.iter().enumerate() {
        let staff = Analysis::home_staff(n.raw.lane);
        chords
            .entry((staff, n.raw.on.round() as i64))
            .or_default()
            .push(i);
    }
    for ids in chords.values() {
        let at = notes[ids[0]].raw.on;
        let ms = |v: f64| tempo.ticks(at, v);
        let shift = ms(rng.normal(6.0, 18.0));
        let loud = rng.normal(2.0, 5.0);
        for &i in ids {
            let n = &mut notes[i];
            n.on += shift + ms(rng.normal(2.0, 6.0));
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
    let mut onsets: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    for (i, n) in notes.iter().enumerate() {
        onsets.entry(n.raw.on.round() as i64).or_default().push(i);
    }
    for ids in onsets.values().filter(|ids| ids.len() > 1) {
        let top = ids
            .iter()
            .copied()
            .filter(|&i| Analysis::home_staff(notes[i].raw.lane) == Staff::Upper)
            .max_by_key(|&i| notes[i].raw.key);
        if let Some(i) = top {
            let at = notes[i].raw.on;
            notes[i].on -= tempo.ticks(at, MELODY_LEAD_MS);
            notes[i].vel += MELODY_VEL;
        }
    }
}

fn spread_arpeggios(notes: &mut [Timed], tempo: &TempoMap) {
    let mut groups: BTreeMap<(i64, usize), Vec<usize>> = BTreeMap::new();
    for (i, n) in notes.iter().enumerate() {
        let Some((_, g)) = n.raw.arp else { continue };
        let id = match g {
            ArpGroup::Staff(Staff::Upper) => 0,
            ArpGroup::Staff(Staff::Lower) => 1,
            ArpGroup::CrossStaff => 2,
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
        // The roll ends on the beat.
        let step = tempo.ticks(notes[ids[0]].raw.on, ARPEGGIO_MS);
        for (&i, notes_after) in ids.iter().rev().zip(0u32..) {
            notes[i].on -= f64::from(notes_after) * step;
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

// Whether the pedal is down at the start of each bar, in the order of the score.
fn held_into(a: &Analysis) -> Vec<bool> {
    let mut out = Vec::with_capacity(a.score.bars.len());
    let mut down = false;
    for b in 0..a.score.bars.len() {
        out.push(down);
        let mut events: Vec<_> = a.pedals.iter().filter(|p| p.bar == b).collect();
        events.sort_by_key(|p| p.at);
        if let Some(last) = events.last() {
            down = last.action != PedalAction::Stop;
        }
    }
    out
}

fn pedal(a: &Analysis, order: &[Instance], tempo: &TempoMap) -> Vec<(f64, u8)> {
    let mut out = Vec::new();
    let mut down = false;
    let held = held_into(a);
    for (k, inst) in order.iter().enumerate() {
        let lead = a.score.bars[inst.bar].layout.lead;
        // After a jump, or at the start of a bar range, a held pedal goes down again.
        let enters = k == 0 || order[k - 1].bar + 1 != inst.bar;
        if enters && held[inst.bar] && !down {
            let gap = if k == 0 {
                0.0
            } else {
                tempo.ticks(inst.start, PEDAL_CHANGE_MS)
            };
            out.push((inst.start + gap, 127));
            down = true;
        }
        for p in a.pedals.iter().filter(|p| p.bar == inst.bar) {
            let t = inst.start + ticks(p.at - lead);
            let gap = t + tempo.ticks(t, PEDAL_CHANGE_MS);
            // A press right after a lift waits, so that the dampers fall.
            let lifted_now = out.last() == Some(&(t, 0));
            match (p.action, down) {
                (PedalAction::Start, false) if lifted_now => out.push((gap, 127)),
                (PedalAction::Start, false) => out.push((t, 127)),
                (PedalAction::Start, true) | (PedalAction::Change, _) => {
                    out.extend([(t, 0), (gap, 127)]);
                }
                (PedalAction::Stop, _) => out.push((t, 0)),
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
            Attr::Program(p) => {
                program = u8::try_from(p.saturating_sub(1).min(127)).expect("clamped to 0..=127");
            }
            // MusicXML: volume 0 to 100, pan -90 (left) to 90 (right).
            Attr::Volume(v) => volume = Some((v * 127.0 / 100.0).round().clamp(0.0, 127.0) as u8),
            Attr::Pan(v) => pan = Some((64.0 + v * 63.0 / 90.0).round().clamp(0.0, 127.0) as u8),
            _ => {}
        }
    }
    (program, volume, pan)
}
