use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::build::{Built, TokKey, part_notes, token_parts};
use super::read::{Edge, SrcSlur, stop};
use super::report::{Report, Row};
use crate::analysis::{Analysis, NoteRef, Part};
use crate::frac::Frac;
use crate::model::{
    Attr, DynMark, El, ElKind, Event, Lane, PitchMark, Place, Score, SlurStart, Tok, TokMark,
};

type Slot = (TokKey, Part);

fn events_mut(score: &mut Score) -> BTreeMap<TokKey, &mut Event> {
    fn walk<'s>(
        bar: usize,
        lane: Lane,
        els: &'s mut [El],
        out: &mut BTreeMap<TokKey, &'s mut Event>,
    ) {
        for e in els {
            let start = e.start;
            match &mut e.kind {
                ElKind::Tok(Tok::Event(ev)) => {
                    out.insert(TokKey { bar, lane, start }, ev);
                }
                ElKind::Group(_, c) => walk(bar, lane, c, out),
                ElKind::Tok(_) => {}
            }
        }
    }
    let mut out = BTreeMap::new();
    for (b, bar) in score.bars.iter_mut().enumerate() {
        for line in bar.lines.iter_mut().filter(|l| l.lane.is_voice()) {
            walk(b, line.lane, &mut line.els, &mut out);
        }
    }
    out
}

fn lane_order(events: &BTreeMap<TokKey, &mut Event>) -> BTreeMap<Lane, Vec<TokKey>> {
    let mut out: BTreeMap<Lane, Vec<TokKey>> = BTreeMap::new();
    for k in events.keys() {
        out.entry(k.lane).or_default().push(*k);
    }
    for v in out.values_mut() {
        v.sort_by_key(|k| (k.bar, k.start));
    }
    out
}

fn notes_of(ev: &Event, p: Part) -> &[crate::model::Note] {
    part_notes(ev, p)
}

fn notes_mut(ev: &mut Event, p: Part) -> Option<&mut Vec<crate::model::Note>> {
    match p {
        Part::Main => match &mut ev.head {
            crate::model::Head::Chord(n) => Some(n),
            _ => None,
        },
        Part::Grace(i) => ev.grace.as_mut().map(|g| &mut g.notes[i].notes),
        Part::After(i) => ev.after.as_mut().map(|g| &mut g.notes[i].notes),
    }
}

fn marks_mut(ev: &mut Event, p: Part) -> &mut Vec<TokMark> {
    match p {
        Part::Main => &mut ev.marks,
        Part::Grace(i) => match ev.grace.as_mut() {
            Some(g) => &mut g.notes[i].marks,
            None => &mut ev.marks,
        },
        Part::After(i) => match ev.after.as_mut() {
            Some(g) => &mut g.notes[i].marks,
            None => &mut ev.marks,
        },
    }
}

fn slurs_mut(ev: &mut Event, p: Part) -> &mut crate::model::Slurs {
    match p {
        Part::Main => &mut ev.slurs,
        Part::Grace(i) => match ev.grace.as_mut() {
            Some(g) => &mut g.notes[i].slurs,
            None => &mut ev.slurs,
        },
        Part::After(i) => match ev.after.as_mut() {
            Some(g) => &mut g.notes[i].slurs,
            None => &mut ev.slurs,
        },
    }
}

pub fn ties(b: &mut Built, rep: &mut Report) -> Result<(), String> {
    let bounds: Vec<(Frac, Frac)> = b
        .score
        .bars
        .iter()
        .map(|bar| (bar.layout.lead, bar.layout.total() - bar.layout.trail))
        .collect();
    let prev_bars: Vec<Option<usize>> = (0..bounds.len())
        .map(|i| crate::analysis::prev_bar(&b.score, i))
        .collect();
    let mut events = events_mut(&mut b.score);
    let order = lane_order(&events);
    let mut matched: BTreeSet<(TokKey, Part, usize)> = BTreeSet::new();
    let mut later: Vec<(TokKey, Part, usize)> = Vec::new();
    let mut grace_ends: Vec<(TokKey, usize)> = Vec::new();
    let mut broken = 0;
    for keys in order.values() {
        let slots: Vec<Slot> = keys
            .iter()
            .flat_map(|k| token_parts(events[k]).into_iter().map(move |p| (*k, p)))
            .collect();
        for (si, (key, part)) in slots.iter().enumerate() {
            let Some(src) = b.toks.get(key) else { continue };
            for (p, k) in src.tie_starts.iter().filter(|(p, _)| p == part) {
                let pitch = notes_of(events[key], *p)[*k].pitch;
                let holds = |slot: &Slot| {
                    notes_of(events[&slot.0], slot.1)
                        .iter()
                        .position(|n| n.pitch == pitch && n.tied)
                };
                if let Part::Grace(_) = p {
                    let own_main = (*key, Part::Main);
                    if let Some(i) = holds(&own_main) {
                        matched.insert((own_main.0, own_main.1, i));
                        continue;
                    }
                    // MuseScore 3 writes no tie stop on the main note of a tied grace note.
                    let same_pitch = notes_of(events[key], Part::Main)
                        .iter()
                        .position(|n| n.pitch == pitch);
                    if let Some(i) = same_pitch {
                        matched.insert((own_main.0, own_main.1, i));
                        grace_ends.push((*key, i));
                        continue;
                    }
                }
                let touches = |slot: &Slot| {
                    let at = slot.0;
                    if at.bar == key.bar {
                        return at.start == src.end;
                    }
                    let bar_after = prev_bars[at.bar] == Some(key.bar);
                    bar_after && src.end == bounds[key.bar].1 && at.start == bounds[at.bar].0
                };
                if let Some(next) = slots.get(si + 1).filter(|s| touches(s)) {
                    if let Some(i) = holds(next) {
                        matched.insert((next.0, next.1, i));
                        continue;
                    }
                }
                let end = src.end;
                let other_ends: Vec<(TokKey, Part, usize)> = events
                    .iter()
                    .filter(|(ok, _)| {
                        let same_time = ok.lane != key.lane && ok.start == end && ok.bar == key.bar;
                        let next_bar = ok.lane != key.lane
                            && ok.bar == key.bar + 1
                            && end == bounds[key.bar].1
                            && ok.start == bounds[ok.bar].0;
                        same_time || next_bar
                    })
                    .flat_map(|(ok, ev)| {
                        token_parts(ev).into_iter().flat_map(move |part| {
                            notes_of(ev, part)
                                .iter()
                                .enumerate()
                                .filter(move |(_, n)| n.pitch == pitch && n.tied)
                                .map(move |(i, _)| (*ok, part, i))
                        })
                    })
                    .collect();
                let other = other_ends.iter().copied().find(|m| !matched.contains(m));
                if other.is_none() && !other_ends.is_empty() {
                    return Err(stop(
                        "a tie into another voice with two possible start notes",
                        &b.numbers[key.bar],
                    ));
                }
                if let Some(m) = other {
                    // A `~` names no lane, so a second note of this pitch that ends here is a second start.
                    let second_start = events.iter().any(|(ok, ev)| {
                        let touching = b.toks.get(ok).is_some_and(|t| t.end == end);
                        let same_pitch = notes_of(ev, Part::Main).iter().any(|n| n.pitch == pitch);
                        ok.lane != key.lane && ok.bar == key.bar && touching && same_pitch
                    });
                    if second_start {
                        return Err(stop(
                            "a tie into another voice with two possible start notes",
                            &b.numbers[key.bar],
                        ));
                    }
                    matched.insert(m);
                    continue;
                }
                let main_only = |s: &&Slot| s.1 == Part::Main;
                let target = slots[si + 1..]
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| main_only(s))
                    .find_map(|(off, s)| holds(s).map(|i| (si + 1 + off, *s, i)));
                if let Some((ti, slot, i)) = target {
                    // Over the grace notes of its own end note a tie is a plain `~`.
                    let own_graces = ti > si + 1 && slots[si + 1..ti].iter().all(|s| s.0 == slot.0);
                    if own_graces && touches(&slot) {
                        matched.insert((slot.0, slot.1, i));
                        continue;
                    }
                    let prev = &slots[ti - 1];
                    let between = ti - 1 > si;
                    let sounds_between = notes_of(events[&prev.0], prev.1)
                        .iter()
                        .any(|n| n.pitch == pitch);
                    let prev_starts_tie = b.toks.get(&prev.0).is_some_and(|t| {
                        t.tie_starts.iter().any(|(pp, pk)| {
                            *pp == prev.1 && notes_of(events[&prev.0], *pp)[*pk].pitch == pitch
                        })
                    });
                    // The end belongs to the tie of the note before it: this tie has no end.
                    if between && prev_starts_tie {
                        broken += 1;
                        continue;
                    }
                    if between && sounds_between {
                        return Err(stop(
                            "a tie with notes of the same pitch between its ends",
                            &b.numbers[key.bar],
                        ));
                    }
                    matched.insert((slot.0, slot.1, i));
                    later.push((*key, *p, *k));
                    continue;
                }
                let later_in_other_lane = events.iter().any(|(ok, ev)| {
                    let after = match ok.bar {
                        bar if bar == key.bar => ok.start > end,
                        bar => prev_bars[bar] == Some(key.bar),
                    };
                    let has_end = token_parts(ev).into_iter().any(|part| {
                        notes_of(ev, part)
                            .iter()
                            .any(|n| n.pitch == pitch && n.tied)
                    });
                    ok.lane != key.lane && after && has_end
                });
                if later_in_other_lane {
                    return Err(stop(
                        "a tie to a later note in another voice",
                        &b.numbers[key.bar],
                    ));
                }
                broken += 1;
            }
        }
    }
    for (key, part, k) in later {
        if let Some(n) = events
            .get_mut(&key)
            .and_then(|e| notes_mut(e, part))
            .and_then(|n| n.get_mut(k))
        {
            n.marks.push(PitchMark::TieLater);
        }
    }
    for (key, i) in grace_ends {
        if let Some(n) = events
            .get_mut(&key)
            .and_then(|e| notes_mut(e, Part::Main))
            .and_then(|n| n.get_mut(i))
        {
            n.tied = true;
        }
    }
    for (key, ev) in events.iter_mut() {
        for part in token_parts(ev) {
            if let Some(notes) = notes_mut(ev, part) {
                for (i, n) in notes.iter_mut().enumerate() {
                    if n.tied && !matched.contains(&(*key, part, i)) {
                        n.tied = false;
                        broken += 1;
                    }
                }
            }
        }
    }
    rep.drop_n(Row::BrokenTies, broken);
    Ok(())
}

struct Pair {
    start: Slot,
    end: Slot,
    above: Option<bool>,
}

fn part_rank(p: Part) -> (u8, usize) {
    match p {
        Part::Grace(i) => (0, i),
        Part::Main => (1, 0),
        Part::After(i) => (2, i),
    }
}

pub fn slurs(b: &mut Built, rep: &mut Report) {
    let mut marks: Vec<(Slot, SrcSlur)> = Vec::new();
    for (key, src) in &b.toks {
        for (p, s) in &src.slurs {
            marks.push(((*key, *p), *s));
        }
    }
    // Document order; on one note a stop comes before a start: `)(`.
    marks.sort_by_key(|(_, s)| (s.note, s.edge == Edge::Start));
    let mut open: HashMap<u32, Vec<(Slot, Option<bool>)>> = HashMap::new();
    // MuseScore writes a measure voice by voice, so the stop of a slur from a later voice can come
    // first: such a stop waits for a start of its number earlier in its measure.
    let mut early_stops: HashMap<u32, Vec<Slot>> = HashMap::new();
    let mut pairs: Vec<Pair> = Vec::new();
    let mut broken = 0;
    for (slot, s) in &marks {
        match s.edge {
            Edge::Start => {
                let waiting = early_stops.get_mut(&s.number).and_then(|stops| {
                    let at = stops
                        .iter()
                        .position(|end| end.0.bar == slot.0.bar && end.0.start > slot.0.start)?;
                    Some(stops.remove(at))
                });
                match waiting {
                    Some(end) => pairs.push(Pair {
                        start: *slot,
                        end,
                        above: s.above,
                    }),
                    None => open.entry(s.number).or_default().push((*slot, s.above)),
                }
            }
            Edge::Stop => match open.get_mut(&s.number).and_then(Vec::pop) {
                Some((start, _)) if start == *slot => broken += 1,
                Some((start, above)) => pairs.push(Pair {
                    start,
                    end: *slot,
                    above,
                }),
                None => early_stops.entry(s.number).or_default().push(*slot),
            },
        }
    }
    broken += open.values().map(Vec::len).sum::<usize>();
    broken += early_stops.values().map(Vec::len).sum::<usize>();
    rep.drop_n(Row::BrokenSlurs, broken);
    let mut numbered = vec![false; pairs.len()];
    let mut lanes: BTreeMap<Lane, Vec<(Slot, usize, Edge)>> = BTreeMap::new();
    for (i, p) in pairs.iter().enumerate() {
        if p.start.0.lane != p.end.0.lane {
            numbered[i] = true;
            continue;
        }
        let lane = lanes.entry(p.start.0.lane).or_default();
        lane.push((p.start, i, Edge::Start));
        lane.push((p.end, i, Edge::Stop));
    }
    let place = |s: &Slot| (s.0.bar, s.0.start, part_rank(s.1));
    for events in lanes.values_mut() {
        // At one slot the outer slur starts first and stops last.
        events.sort_by_key(|(slot, i, edge)| {
            let other = match edge {
                Edge::Start => pairs[*i].end,
                Edge::Stop => pairs[*i].start,
            };
            (place(slot), *edge == Edge::Start, Reverse(place(&other)))
        });
        let mut stack: Vec<usize> = Vec::new();
        for (_, i, edge) in events.iter() {
            match edge {
                Edge::Start => stack.push(*i),
                Edge::Stop => {
                    if stack.last() == Some(i) {
                        stack.pop();
                    } else {
                        numbered[*i] = true;
                        stack.retain(|x| x != i);
                    }
                }
            }
        }
    }
    let mut timeline: Vec<(Slot, usize, Edge)> = Vec::new();
    for (i, p) in pairs.iter().enumerate().filter(|(i, _)| numbered[*i]) {
        timeline.push((p.start, i, Edge::Start));
        timeline.push((p.end, i, Edge::Stop));
    }
    timeline.sort_by_key(|(slot, _, edge)| {
        (
            slot.0.bar,
            slot.0.start,
            part_rank(slot.1),
            u8::from(*edge == Edge::Start),
        )
    });
    let mut numbers: HashMap<usize, u32> = HashMap::new();
    let mut in_use: BTreeSet<u32> = BTreeSet::new();
    for (_, i, edge) in &timeline {
        match edge {
            Edge::Start => {
                let n = (1..).find(|n| !in_use.contains(n)).unwrap_or(1);
                in_use.insert(n);
                numbers.insert(*i, n);
            }
            Edge::Stop => {
                if let Some(n) = numbers.get(i) {
                    in_use.remove(n);
                }
            }
        }
    }
    let mut events = events_mut(&mut b.score);
    // The outer slur first: unnumbered slurs pair by nesting, and `:su` or `:sd` is its direction.
    let mut order: Vec<usize> = (0..pairs.len()).collect();
    order.sort_by_key(|&i| {
        let end = pairs[i].end;
        (
            pairs[i].start,
            Reverse((end.0.bar, end.0.start, part_rank(end.1))),
        )
    });
    for i in order {
        let p = &pairs[i];
        let number = numbers.get(&i).copied();
        if let Some(ev) = events.get_mut(&p.end.0) {
            slurs_mut(ev, p.end.1).ends.push(number);
        }
        let Some(ev) = events.get_mut(&p.start.0) else {
            continue;
        };
        let place = p
            .above
            .map(|up| if up { Place::Above } else { Place::Below });
        let starts = &mut slurs_mut(ev, p.start.1).starts;
        if !starts.is_empty() {
            starts.push(SlurStart { number, place });
            continue;
        }
        starts.push(SlurStart {
            number,
            place: None,
        });
        let mark = match place {
            Some(Place::Above) => Some(TokMark::SlurUp),
            Some(Place::Below) => Some(TokMark::SlurDown),
            None => None,
        };
        marks_mut(ev, p.start.1).extend(mark);
    }
}

fn parse_vel_line(line: &str) -> Option<(Option<String>, Attr)> {
    let mut words = line.split_whitespace();
    let first = words.next()?;
    let (label, attr) = if first == "@" {
        (None, words.next()?)
    } else {
        if words.next()? != "@" {
            return None;
        }
        (Some(first.to_string()), words.next()?)
    };
    let (name, value) = attr.split_once('=')?;
    let a = crate::text::attr::parse_attr(name, value).ok()?;
    Some((label, a))
}

pub fn mark_velocities(b: &mut Built, vel_lines: &[String], rep: &mut Report) {
    if !vel_lines.is_empty() {
        for line in vel_lines {
            match parse_vel_line(line) {
                Some((None, a)) => b.score.header.push(a),
                Some((Some(label), a)) => {
                    match b.score.bars.iter_mut().find(|bar| bar.label == label) {
                        Some(bar) => bar.attrs.push(a),
                        None => rep.note(format!(
                            "the velocity line `{line}` names no bar; it is dropped"
                        )),
                    }
                }
                None => rep.note(format!(
                    "the velocity line `{line}` does not parse; it is dropped"
                )),
            }
        }
        return;
    }
    let mut current: HashMap<DynMark, f64> = HashMap::new();
    let mut bar_value: HashMap<(usize, DynMark), f64> = HashMap::new();
    for m in &b.marks {
        let v = m.velocity;
        if let Some(first) = bar_value.get(&(m.bar, m.mark)) {
            if (first - v).abs() > 0.001 {
                rep.drop(Row::SameNameVelocity);
            }
            continue;
        }
        bar_value.insert((m.bar, m.mark), v);
        let now = current
            .get(&m.mark)
            .copied()
            .unwrap_or(m.mark.default_velocity());
        if (now - v).abs() > 0.001 {
            b.score.bars[m.bar].attrs.push(Attr::VelMark(m.mark, v));
            current.insert(m.mark, v);
        }
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

pub fn note_velocities(b: &mut Built, lane_rule: bool) {
    // `None` is a note that does not sound (`v0`).
    let offsets: Vec<(TokKey, Part, Vec<Option<f64>>)> = {
        let a = Analysis::new(&b.score);
        let mut out = Vec::new();
        for (id, leaf) in a.leaves.iter().enumerate() {
            let Some(ev) = leaf.event() else { continue };
            let key = TokKey {
                bar: leaf.bar,
                lane: leaf.lane,
                start: leaf.start,
            };
            let Some(src) = b.toks.get(&key) else {
                continue;
            };
            for part in token_parts(ev) {
                let Some((_, targets)) = src.velocities.iter().find(|(p, _)| *p == part) else {
                    continue;
                };
                let mut offs = Vec::new();
                for (i, t) in targets.iter().enumerate() {
                    if *t == 0.0 {
                        offs.push(None);
                        continue;
                    }
                    let now = a
                        .velocity(NoteRef {
                            leaf: id,
                            part,
                            note: i,
                        })
                        .note;
                    offs.push(Some(round2(t - now)));
                }
                out.push((key, part, offs));
            }
        }
        out
    };
    let mut lane_offset: BTreeMap<Lane, f64> = BTreeMap::new();
    if lane_rule {
        let mut by_lane: BTreeMap<Lane, Vec<f64>> = BTreeMap::new();
        for (key, _, offs) in &offsets {
            by_lane
                .entry(key.lane)
                .or_default()
                .extend(offs.iter().flatten());
        }
        for (lane, offs) in by_lane {
            let mut counts: Vec<(f64, usize)> = Vec::new();
            for o in &offs {
                match counts.iter_mut().find(|(v, _)| (v - o).abs() < 0.001) {
                    Some((_, n)) => *n += 1,
                    None => counts.push((*o, 1)),
                }
            }
            if let Some((v, n)) = counts.into_iter().max_by_key(|(_, n)| *n) {
                if n * 2 > offs.len() && v.abs() > 0.001 {
                    lane_offset.insert(lane, v);
                }
            }
        }
        for (lane, v) in &lane_offset {
            let first = offsets
                .iter()
                .filter(|(k, _, _)| k.lane == *lane)
                .map(|(k, _, _)| k.bar)
                .min();
            if let Some(bar) = first {
                b.score.bars[bar].attrs.push(Attr::VelLane(*lane, *v));
            }
        }
    }
    let mut events = events_mut(&mut b.score);
    for (key, part, offs) in offsets {
        let shift = lane_offset.get(&key.lane).copied().unwrap_or(0.0);
        let offs: Vec<Option<f64>> = offs.iter().map(|o| o.map(|o| round2(o - shift))).collect();
        let Some(ev) = events.get_mut(&key) else {
            continue;
        };
        let Some(first) = offs.first().copied() else {
            continue;
        };
        let same = |a: Option<f64>, b: Option<f64>| match (a, b) {
            (Some(a), Some(b)) => (a - b).abs() < 0.001,
            (None, None) => true,
            _ => false,
        };
        if offs.iter().all(|o| same(*o, first)) {
            match first {
                None => marks_mut(ev, part).push(TokMark::Mute),
                Some(v) if v.abs() > 0.001 => marks_mut(ev, part).push(TokMark::Vel(v)),
                Some(_) => {}
            }
            continue;
        }
        if let Some(notes) = notes_mut(ev, part) {
            for (n, o) in notes.iter_mut().zip(&offs) {
                match o {
                    None => n.marks.push(PitchMark::Mute),
                    Some(v) if v.abs() > 0.001 => n.marks.push(PitchMark::Vel(*v)),
                    Some(_) => {}
                }
            }
        }
    }
}
