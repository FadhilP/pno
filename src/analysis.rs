use std::collections::{BTreeMap, HashMap, HashSet};

use crate::diag::Diag;
use crate::frac::{Frac, NoteValue};
use crate::model::{
    Attr, El, ElKind, Event, GroupHead, Head, Key, Lane, MarkKind, Note, Ottava, Pedal, PedalStyle,
    Pitch, PitchMark, Place, Score, Staff, Tok, TupletLook,
};
use crate::text::write::default_ratio;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Part {
    Grace(usize),
    Main,
    After(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NoteRef {
    pub leaf: usize,
    pub part: Part,
    pub note: usize,
}

#[derive(Clone, Debug)]
pub struct TupletGroup {
    pub head: GroupHead,
    pub actual: u32,
    pub normal: u32,
    pub first_leaf: usize,
    pub last_leaf: usize,
    pub depth: usize,
}

#[derive(Clone, Debug)]
pub struct Leaf<'a> {
    pub bar: usize,
    pub lane: Lane,
    pub src: usize,
    pub start: Frac,
    pub dur: Frac,
    pub written: Frac,
    pub tok: &'a Tok,
    pub groups: Vec<usize>,
}

impl<'a> Leaf<'a> {
    pub fn event(&self) -> Option<&'a Event> {
        match self.tok {
            Tok::Event(ev) => Some(ev),
            _ => None,
        }
    }

    pub fn end(&self) -> Frac {
        self.start + self.dur
    }
}

#[derive(Clone, Debug)]
pub struct BarInfo {
    pub start: Frac,
    pub key: Key,
    pub vel_marks: HashMap<u8, f64>,
    pub vel_lanes: BTreeMap<Lane, f64>,
}

#[derive(Clone, Debug)]
pub struct Span<'a> {
    pub lane: Lane,
    pub tok: &'a Tok,
    pub start: (usize, Frac),
    pub end: (usize, Frac),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PedalAction {
    Start,
    Stop,
    Change,
}

#[derive(Clone, Copy, Debug)]
pub struct PedalEvent {
    pub bar: usize,
    pub at: Frac,
    pub action: PedalAction,
}

#[derive(Clone, Debug)]
pub struct SlurPair {
    pub start: NoteRef,
    pub end: NoteRef,
    pub number: u32,
    pub above: Option<bool>,
}

pub struct Analysis<'a> {
    pub score: &'a Score,
    pub bars: Vec<BarInfo>,
    pub leaves: Vec<Leaf<'a>>,
    pub groups: Vec<TupletGroup>,
    pub lane_leaves: BTreeMap<Lane, Vec<usize>>,
    pub tie_starts: HashMap<NoteRef, NoteRef>,
    pub slurs: Vec<SlurPair>,
    pub spans: Vec<Span<'a>>,
    pub pedals: Vec<PedalEvent>,
    pub pedal_style: PedalStyle,
    pub signs: HashSet<NoteRef>,
    pub diags: Vec<Diag>,
}

// The bar before `b` for ties: a second volta continues from the bar before the first volta.
pub fn prev_bar(score: &Score, b: usize) -> Option<usize> {
    let p = b.checked_sub(1)?;
    let volta = |i: usize| {
        score.bars[i].attrs.iter().find_map(|a| match a {
            Attr::Volta { numbers, .. } => Some(numbers.clone()),
            _ => None,
        })
    };
    let (Some(mine), Some(theirs)) = (volta(b), volta(p)) else {
        return Some(p);
    };
    if mine == theirs {
        return Some(p);
    }
    let mut first = p;
    while first > 0 && volta(first - 1) == Some(theirs.clone()) {
        first -= 1;
    }
    first.checked_sub(1)
}

fn slur_above(marks: &[crate::model::TokMark]) -> Option<bool> {
    marks.iter().find_map(|m| match m {
        crate::model::TokMark::SlurUp => Some(true),
        crate::model::TokMark::SlurDown => Some(false),
        _ => None,
    })
}

pub fn at(score: &Score, bar: usize, lane: Lane, cell: Option<usize>) -> String {
    let label = &score.bars[bar].label;
    match cell {
        Some(c) => format!("{label} {lane} cell {}", c + 1),
        None => format!("{label} {lane}"),
    }
}

impl<'a> Analysis<'a> {
    pub fn new(score: &'a Score) -> Analysis<'a> {
        let mut a = Analysis {
            score,
            bars: Vec::new(),
            leaves: Vec::new(),
            groups: Vec::new(),
            lane_leaves: BTreeMap::new(),
            tie_starts: HashMap::new(),
            slurs: Vec::new(),
            spans: Vec::new(),
            pedals: Vec::new(),
            pedal_style: PedalStyle::Line,
            signs: HashSet::new(),
            diags: Vec::new(),
        };
        a.bar_info();
        a.collect_leaves();
        a.check_values();
        a.resolve_ties();
        a.pair_slurs();
        a.collect_spans();
        a.collect_pedals();
        a.signs = crate::accidental::default_signs(&a);
        a
    }

    pub fn abs(&self, bar: usize, t: Frac) -> Frac {
        self.bars[bar].start + t - self.score.bars[bar].layout.lead
    }

    pub fn bar_end(&self, bar: usize) -> Frac {
        let l = &self.score.bars[bar].layout;
        l.total() - l.trail
    }

    pub fn cell(&self, bar: usize, t: Frac) -> Option<usize> {
        Some(self.score.bars[bar].layout.cell_at(t))
    }

    fn error(&mut self, src: usize, at: String, msg: impl Into<String>) {
        self.diags.push(Diag::error(src, at, msg));
    }

    fn bar_info(&mut self) {
        let mut key = Key {
            fifths: 0,
            mode: None,
        };
        let mut vel_marks: HashMap<u8, f64> = HashMap::new();
        let mut vel_lanes: BTreeMap<Lane, f64> = BTreeMap::new();
        for a in &self.score.header {
            match a {
                Attr::Key(k) => key = *k,
                Attr::VelMark(m, v) => {
                    vel_marks.insert(m.0, *v);
                }
                Attr::VelLane(l, v) => {
                    vel_lanes.insert(*l, *v);
                }
                Attr::PedalStyle(s) => self.pedal_style = *s,
                _ => {}
            }
        }
        let mut t = Frac::ZERO;
        for bar in &self.score.bars {
            for a in &bar.attrs {
                match a {
                    Attr::Key(k) => key = *k,
                    Attr::VelMark(m, v) => {
                        vel_marks.insert(m.0, *v);
                    }
                    Attr::VelLane(l, v) => {
                        vel_lanes.insert(*l, *v);
                    }
                    _ => {}
                }
            }
            self.bars.push(BarInfo {
                start: t,
                key,
                vel_marks: vel_marks.clone(),
                vel_lanes: vel_lanes.clone(),
            });
            t += bar.layout.length();
        }
    }

    fn collect_leaves(&mut self) {
        let score = self.score;
        for (b, bar) in score.bars.iter().enumerate() {
            let mut lines: Vec<&crate::model::Line> =
                bar.lines.iter().filter(|l| l.lane.is_voice()).collect();
            lines.sort_by_key(|l| l.lane.order());
            for line in lines {
                let mut stack = Vec::new();
                self.walk(b, line.lane, line.src, &line.els, &mut stack, Frac::int(1));
            }
        }
        for (i, leaf) in self.leaves.iter().enumerate() {
            self.lane_leaves.entry(leaf.lane).or_default().push(i);
        }
    }

    fn walk(
        &mut self,
        bar: usize,
        lane: Lane,
        src: usize,
        els: &'a [El],
        stack: &mut Vec<usize>,
        factor: Frac,
    ) {
        for e in els {
            match &e.kind {
                ElKind::Tok(Tok::Gap) => {}
                ElKind::Tok(tok) => self.leaves.push(Leaf {
                    bar,
                    lane,
                    src,
                    start: e.start,
                    dur: e.dur,
                    written: e.dur * factor,
                    tok,
                    groups: stack.clone(),
                }),
                ElKind::Group(head, children) => {
                    let (actual, normal) = self.ratio(bar, lane, src, e, *head).unwrap_or((1, 1));
                    let id = self.groups.len();
                    let first = self.leaves.len();
                    self.groups.push(TupletGroup {
                        head: *head,
                        actual,
                        normal,
                        first_leaf: first,
                        last_leaf: first,
                        depth: stack.len(),
                    });
                    stack.push(id);
                    let f = factor * Frac::new(i64::from(actual), i64::from(normal));
                    self.walk(bar, lane, src, children, stack, f);
                    stack.pop();
                    self.groups[id].last_leaf = self.leaves.len().saturating_sub(1);
                    if let GroupHead::Tremolo { .. } = head {
                        self.check_tremolo(bar, lane, src, e, children);
                    }
                }
            }
        }
    }

    fn ratio(
        &mut self,
        bar: usize,
        lane: Lane,
        src: usize,
        e: &El,
        head: GroupHead,
    ) -> Option<(u32, u32)> {
        let loc = at(self.score, bar, lane, self.cell(bar, e.start));
        match head {
            GroupHead::Tremolo { .. } => Some((2, 1)),
            GroupHead::Tuplet { num, ratio, .. } => {
                let Some(b) = ratio.or_else(|| default_ratio(num, e.dur)) else {
                    self.error(
                        src,
                        loc,
                        format!(
                            "a {num}-group over this span needs its ratio, for example {num}:{}[",
                            num - 1
                        ),
                    );
                    return None;
                };
                if !(e.dur / i64::from(b)).is_pow2() {
                    self.error(
                        src,
                        loc,
                        format!(
                            "the span of {num}:{b}[ divided by {b} is not one plain note value"
                        ),
                    );
                    return None;
                }
                Some((num, b))
            }
        }
    }

    fn check_tremolo(&mut self, bar: usize, lane: Lane, src: usize, e: &El, children: &[El]) {
        let notes: Vec<&El> = children
            .iter()
            .filter(|c| matches!(c.kind, ElKind::Tok(Tok::Event(_))))
            .collect();
        let halves =
            notes.len() == 2 && children.len() == 2 && notes.iter().all(|n| n.dur * 2 == e.dur);
        if !halves {
            let loc = at(self.score, bar, lane, self.cell(bar, e.start));
            self.error(
                src,
                loc,
                "a tremolo group has two notes, each with half of its slots",
            );
        }
    }

    fn check_values(&mut self) {
        let mut errs = Vec::new();
        for leaf in &self.leaves {
            let Some(ev) = leaf.event() else { continue };
            if ev.head == Head::BarRest {
                continue;
            }
            if NoteValue::of(leaf.written).is_none() {
                let loc = at(
                    self.score,
                    leaf.bar,
                    leaf.lane,
                    self.cell(leaf.bar, leaf.start),
                );
                errs.push(Diag::error(
                    leaf.src,
                    loc,
                    format!(
                        "a `-` run of {} is not one note value. Split it with a tie: `~` and the pitch, or run `pno fmt --fix`",
                        describe(leaf.written)
                    ),
                ));
            }
        }
        self.diags.extend(errs);
    }

    pub fn prev_bar(&self, b: usize) -> Option<usize> {
        prev_bar(self.score, b)
    }

    pub fn notes_of(&self, r: NoteRef) -> Option<&'a Note> {
        let ev = self.leaves[r.leaf].event()?;
        match r.part {
            Part::Main => ev.notes().get(r.note),
            Part::Grace(g) => ev.grace.as_ref()?.notes.get(g)?.notes.get(r.note),
            Part::After(g) => ev.after.as_ref()?.notes.get(g)?.notes.get(r.note),
        }
    }

    fn find_pitch(notes: &[Note], p: Pitch) -> Option<usize> {
        notes.iter().position(|n| n.pitch == p)
    }

    fn ending_refs(&self, leaf: usize, p: Pitch) -> Option<NoteRef> {
        let ev = self.leaves[leaf].event()?;
        if let Some(after) = &ev.after {
            if let Some((g, gn)) = after.notes.iter().enumerate().next_back() {
                return Self::find_pitch(&gn.notes, p).map(|n| NoteRef {
                    leaf,
                    part: Part::After(g),
                    note: n,
                });
            }
        }
        Self::find_pitch(ev.notes(), p).map(|n| NoteRef {
            leaf,
            part: Part::Main,
            note: n,
        })
    }

    fn resolve_ties(&mut self) {
        let lanes: Vec<(Lane, Vec<usize>)> = self
            .lane_leaves
            .iter()
            .map(|(l, v)| (*l, v.clone()))
            .collect();
        let mut errors = Vec::new();
        for (_lane, ids) in &lanes {
            let mut pending: HashMap<Pitch, NoteRef> = HashMap::new();
            for (k, &id) in ids.iter().enumerate() {
                let leaf = &self.leaves[id];
                let Some(ev) = leaf.event() else { continue };
                if let Some(g) = &ev.grace {
                    for (gi, gn) in g.notes.iter().enumerate() {
                        for (ni, n) in gn.notes.iter().enumerate() {
                            let here = NoteRef {
                                leaf: id,
                                part: Part::Grace(gi),
                                note: ni,
                            };
                            if n.tied {
                                let prev = if gi == 0 { None } else { Some(gi - 1) };
                                let src = prev
                                    .and_then(|p| {
                                        Self::find_pitch(&g.notes[p].notes, n.pitch).map(|x| {
                                            NoteRef {
                                                leaf: id,
                                                part: Part::Grace(p),
                                                note: x,
                                            }
                                        })
                                    })
                                    .or_else(|| self.tie_source(ids, k, n.pitch, &mut pending));
                                self.link(src, here, n, id, &mut errors);
                            }
                            if n.has(PitchMark::TieLater) {
                                pending.insert(n.pitch, here);
                            }
                        }
                    }
                }
                for (ni, n) in ev.notes().iter().enumerate() {
                    let here = NoteRef {
                        leaf: id,
                        part: Part::Main,
                        note: ni,
                    };
                    if n.tied {
                        let from_grace = ev.grace.as_ref().and_then(|g| {
                            g.notes.iter().enumerate().rev().find_map(|(gi, gn)| {
                                Self::find_pitch(&gn.notes, n.pitch).map(|x| NoteRef {
                                    leaf: id,
                                    part: Part::Grace(gi),
                                    note: x,
                                })
                            })
                        });
                        let src =
                            from_grace.or_else(|| self.tie_source(ids, k, n.pitch, &mut pending));
                        self.link(src, here, n, id, &mut errors);
                    }
                    if n.has(PitchMark::TieLater) {
                        pending.insert(n.pitch, here);
                    }
                }
            }
            for (p, r) in pending {
                let leaf = &self.leaves[r.leaf];
                let loc = at(
                    self.score,
                    leaf.bar,
                    leaf.lane,
                    self.cell(leaf.bar, leaf.start),
                );
                errors.push(Diag::error(
                    leaf.src,
                    loc,
                    format!("{p}^t has no later ~{p} in this lane"),
                ));
            }
        }
        self.diags.extend(errors);
    }

    fn link(
        &mut self,
        src: Option<NoteRef>,
        here: NoteRef,
        n: &Note,
        id: usize,
        errors: &mut Vec<Diag>,
    ) {
        match src {
            Some(s) => {
                self.tie_starts.insert(s, here);
            }
            None => {
                let leaf = &self.leaves[id];
                let loc = at(
                    self.score,
                    leaf.bar,
                    leaf.lane,
                    self.cell(leaf.bar, leaf.start),
                );
                errors.push(Diag::error(
                    leaf.src,
                    loc,
                    format!(
                        "~{} has no tie start. The same pitch must sound directly before it in this lane, end here in another lane, or have ^t earlier",
                        n.pitch
                    ),
                ));
            }
        }
    }

    fn tie_source(
        &self,
        ids: &[usize],
        k: usize,
        p: Pitch,
        pending: &mut HashMap<Pitch, NoteRef>,
    ) -> Option<NoteRef> {
        let leaf = &self.leaves[ids[k]];
        let here = self.abs(leaf.bar, leaf.start);
        let prev = self.prev_in_lane(ids, k);
        if let Some(prev) = prev {
            if let Some(r) = self.ending_refs(prev, p) {
                return Some(r);
            }
        }
        if let Some(r) = pending.remove(&p) {
            return Some(r);
        }
        let bars = [Some(leaf.bar), self.prev_bar(leaf.bar)];
        self.leaves.iter().enumerate().find_map(|(i, other)| {
            let near = bars.contains(&Some(other.bar));
            if !near || other.lane == leaf.lane {
                return None;
            }
            let ends_here = if Some(other.bar) == self.prev_bar(leaf.bar)
                && leaf.start == self.score.bars[leaf.bar].layout.lead
            {
                other.end() == self.bar_end(other.bar)
            } else {
                other.bar == leaf.bar && self.abs(other.bar, other.end()) == here
            };
            if ends_here {
                self.ending_refs(i, p)
            } else {
                None
            }
        })
    }

    fn prev_in_lane(&self, ids: &[usize], k: usize) -> Option<usize> {
        let leaf = &self.leaves[ids[k]];
        if leaf.start > self.score.bars[leaf.bar].layout.lead {
            let p = ids[..k]
                .iter()
                .rev()
                .find(|&&i| self.leaves[i].bar == leaf.bar)?;
            return (self.leaves[*p].end() == leaf.start).then_some(*p);
        }
        let pb = self.prev_bar(leaf.bar)?;
        let p = ids.iter().rev().find(|&&i| self.leaves[i].bar == pb)?;
        (self.leaves[*p].end() == self.bar_end(pb)).then_some(*p)
    }

    fn pair_slurs(&mut self) {
        struct Mark {
            t: Frac,
            order: usize,
            at: NoteRef,
            starts: Vec<crate::model::SlurStart>,
            ends: Vec<Option<u32>>,
            above: Option<bool>,
        }
        let mut marks: Vec<(Lane, Mark)> = Vec::new();
        for (id, leaf) in self.leaves.iter().enumerate() {
            let Some(ev) = leaf.event() else { continue };
            let t = self.abs(leaf.bar, leaf.start);
            if let Some(g) = &ev.grace {
                for (gi, gn) in g.notes.iter().enumerate() {
                    marks.push((
                        leaf.lane,
                        Mark {
                            t,
                            order: gi,
                            at: NoteRef {
                                leaf: id,
                                part: Part::Grace(gi),
                                note: 0,
                            },
                            starts: gn.slurs.starts.clone(),
                            ends: gn.slurs.ends.clone(),
                            above: slur_above(&gn.marks),
                        },
                    ));
                }
            }
            marks.push((
                leaf.lane,
                Mark {
                    t,
                    order: 100,
                    at: NoteRef {
                        leaf: id,
                        part: Part::Main,
                        note: 0,
                    },
                    starts: ev.slurs.starts.clone(),
                    ends: ev.slurs.ends.clone(),
                    above: slur_above(&ev.marks),
                },
            ));
            if let Some(g) = &ev.after {
                for (gi, gn) in g.notes.iter().enumerate() {
                    marks.push((
                        leaf.lane,
                        Mark {
                            t,
                            order: 200 + gi,
                            at: NoteRef {
                                leaf: id,
                                part: Part::After(gi),
                                note: 0,
                            },
                            starts: gn.slurs.starts.clone(),
                            ends: gn.slurs.ends.clone(),
                            above: slur_above(&gn.marks),
                        },
                    ));
                }
            }
        }
        marks.sort_by(|(la, a), (lb, b)| (a.t, a.order, la).cmp(&(b.t, b.order, lb)));
        let mut stacks: HashMap<Lane, Vec<(NoteRef, Option<bool>)>> = HashMap::new();
        let mut named: HashMap<u32, (NoteRef, Option<bool>)> = HashMap::new();
        let mut pairs: Vec<(NoteRef, NoteRef, Option<bool>)> = Vec::new();
        let mut errors = Vec::new();
        // As the import numbers them: at one moment, all lanes end their slurs before a slur starts.
        for moment in marks.chunk_by(|(_, a), (_, b)| (a.t, a.order) == (b.t, b.order)) {
            for (lane, m) in moment {
                for e in &m.ends {
                    let open = match e {
                        Some(n) => named.remove(n),
                        None => stacks.entry(*lane).or_default().pop(),
                    };
                    match open {
                        Some((s, above)) => pairs.push((s, m.at, above)),
                        None => errors.push((
                            m.at,
                            match e {
                                Some(n) => format!("){n} has no ({n} before it"),
                                None => "`)` has no `(` before it in this lane".to_string(),
                            },
                        )),
                    }
                }
            }
            for (lane, m) in moment {
                for (i, s) in m.starts.iter().enumerate() {
                    let above = if i == 0 {
                        m.above
                    } else {
                        s.place.map(|p| p == Place::Above)
                    };
                    match s.number {
                        Some(n) => {
                            if named.insert(n, (m.at, above)).is_some() {
                                errors.push((m.at, format!("slur ({n} is already open")));
                            }
                        }
                        None => stacks.entry(*lane).or_default().push((m.at, above)),
                    }
                }
            }
        }
        for (s, _) in stacks.into_values().flatten().chain(named.into_values()) {
            errors.push((s, "an open slur at the end of the file".to_string()));
        }
        for (r, msg) in errors {
            let leaf = &self.leaves[r.leaf];
            let loc = at(
                self.score,
                leaf.bar,
                leaf.lane,
                self.cell(leaf.bar, leaf.start),
            );
            self.diags.push(Diag::error(leaf.src, loc, msg));
        }
        // MusicXML readers pair slur numbers in document order: the export writes bar, lane, leaf, part.
        let doc = |r: NoteRef| {
            let leaf = &self.leaves[r.leaf];
            (leaf.bar, leaf.lane.order(), r.leaf, r.part)
        };
        let mut pairs: Vec<_> = pairs
            .into_iter()
            .map(|(s, e, above)| (doc(s), doc(e), s, e, above))
            .collect();
        pairs.sort_by_key(|p| (p.0, p.1));
        let mut open = Vec::new();
        for (start_at, end_at, s, e, above) in pairs {
            // A stop on the start note is written before the start: `)(`.
            open.retain(|(end, _)| *end > start_at);
            let number = (1..)
                .find(|n| open.iter().all(|(_, m)| m != n))
                .unwrap_or(1);
            open.push((end_at, number));
            self.slurs.push(SlurPair {
                start: s,
                end: e,
                number,
                above,
            });
        }
    }

    fn lane_els(&self, bar: usize, lane: Lane) -> Option<&'a [El]> {
        let score: &'a Score = self.score;
        score.bars[bar]
            .lines
            .iter()
            .find(|l| l.lane == lane)
            .map(|l| l.els.as_slice())
    }

    fn src_of(&self, bar: usize, lane: Lane) -> usize {
        self.score.bars[bar]
            .lines
            .iter()
            .find(|l| l.lane == lane)
            .map_or(self.score.bars[bar].src, |l| l.src)
    }

    fn collect_spans(&mut self) {
        let mut lanes: Vec<Lane> = Vec::new();
        for bar in &self.score.bars {
            for l in &bar.lines {
                let span_lane = matches!(l.lane, Lane::Tempo | Lane::Mark(..));
                if span_lane && !lanes.contains(&l.lane) {
                    lanes.push(l.lane);
                }
            }
        }
        for lane in lanes {
            let mut open: Option<Span<'a>> = None;
            for b in 0..self.score.bars.len() {
                let Some(els) = self.lane_els(b, lane) else {
                    if let Some(s) = open.take() {
                        self.spans.push(s);
                    }
                    continue;
                };
                let els = flatten(els);
                for (start, end, tok) in els {
                    if *tok == Tok::Cont {
                        match &mut open {
                            Some(s) => s.end = (b, end),
                            None => {
                                let loc = at(self.score, b, lane, self.cell(b, start));
                                let src = self.src_of(b, lane);
                                self.error(src, loc, "`-` with no span to continue");
                            }
                        }
                        continue;
                    }
                    if let Some(s) = open.take() {
                        self.spans.push(s);
                    }
                    let spans = matches!(tok, Tok::Hairpin(..) | Tok::Ottava(_)) || tok.extended();
                    let span = Span {
                        lane,
                        tok,
                        start: (b, start),
                        end: (b, end),
                    };
                    if spans {
                        open = Some(span);
                    } else if !matches!(tok, Tok::Empty | Tok::Gap) {
                        self.spans.push(span);
                    }
                }
                let reaches_end = open.as_ref().is_some_and(|s| s.end == (b, self.bar_end(b)));
                if !reaches_end {
                    if let Some(s) = open.take() {
                        self.spans.push(s);
                    }
                }
            }
            if let Some(s) = open.take() {
                self.spans.push(s);
            }
        }
        self.spans.sort_by_key(|s| (s.start.0, s.start.1));
    }

    pub fn pedal_lines(&self) -> Vec<Option<(&'a [El], usize)>> {
        let mut rule: Option<(&'a [El], usize)> = None;
        let mut out = Vec::new();
        for (b, bar) in self.score.bars.iter().enumerate() {
            if bar.attrs.iter().any(|a| matches!(a, Attr::Time(_))) {
                rule = None;
            }
            let src = |lane| self.src_of(b, lane);
            if let Some(els) = self.lane_els(b, Lane::PedRule) {
                rule = has_pedal(els).then_some((els, src(Lane::PedRule)));
            }
            let own = self.lane_els(b, Lane::Ped).map(|e| (e, src(Lane::Ped)));
            let rule_applies =
                !bar.layout.is_partial() && !bar.attrs.iter().any(|a| matches!(a, Attr::Cells(_)));
            out.push(own.or(if rule_applies { rule } else { None }));
        }
        out
    }

    fn stop(&mut self, bar: usize, at: Frac) {
        self.pedals.push(PedalEvent {
            bar,
            at,
            action: PedalAction::Stop,
        });
    }

    fn collect_pedals(&mut self) {
        let lines = self.pedal_lines();
        let mut carried: Option<usize> = None;
        for (b, line) in lines.iter().enumerate() {
            let items = line.map(|(els, _)| flatten(els)).unwrap_or_default();
            let mut from_prev = carried.take();
            let mut held_until: Option<Frac> = None;
            for (i, (start, end, tok)) in items.iter().enumerate() {
                let is_pedal = matches!(tok, Tok::Pedal(_));
                // A `-` in a tuplet group is its own item.
                let held_on = is_pedal || **tok == Tok::Cont;
                if let Some(e) = held_until {
                    if !held_on || e != *start {
                        self.stop(b, e);
                        held_until = None;
                    }
                }
                if i == 0 && !is_pedal && **tok != Tok::Cont {
                    if let Some(pb) = from_prev.take() {
                        self.stop(pb, self.bar_end(pb));
                    }
                }
                match tok {
                    Tok::Cont => {
                        let continues = held_until == Some(*start);
                        if from_prev.take().is_some() || continues {
                            held_until = Some(*end);
                        } else {
                            let loc = at(self.score, b, Lane::Ped, self.cell(b, *start));
                            let src = line.map_or(0, |(_, s)| s);
                            self.error(src, loc, "`-` when no pedal is held");
                        }
                    }
                    Tok::Pedal(p) => {
                        let held = held_until.is_some() || from_prev.is_some();
                        let action = match (p, held) {
                            (Pedal::Change, true) => PedalAction::Change,
                            (Pedal::Press, true) => {
                                match from_prev {
                                    Some(pb) => self.stop(pb, self.bar_end(pb)),
                                    None => self.stop(b, *start),
                                }
                                PedalAction::Start
                            }
                            _ => PedalAction::Start,
                        };
                        self.pedals.push(PedalEvent {
                            bar: b,
                            at: *start,
                            action,
                        });
                        from_prev = None;
                        held_until = Some(*end);
                    }
                    _ => {}
                }
            }
            if let Some(pb) = from_prev {
                self.stop(pb, self.bar_end(pb));
            }
            if let Some(e) = held_until {
                if e == self.bar_end(b) {
                    carried = Some(b);
                } else {
                    self.stop(b, e);
                }
            }
        }
        if let Some(pb) = carried {
            self.stop(pb, self.bar_end(pb));
        }
        self.pedals
            .sort_by_key(|p| (p.bar, p.at, p.action != PedalAction::Stop));
    }

    pub fn ottava_at(&self, staff: Staff, bar: usize, t: Frac) -> Option<Ottava> {
        let here = self.abs(bar, t);
        self.spans.iter().find_map(|s| match (s.lane, s.tok) {
            (Lane::Mark(st, MarkKind::Ottava, _), Tok::Ottava(o)) if st == staff => {
                let from = self.abs(s.start.0, s.start.1);
                let to = self.abs(s.end.0, s.end.1);
                (from <= here && here < to).then_some(*o)
            }
            _ => None,
        })
    }

    fn mark_at(&self, staff: Staff, t: Frac) -> Option<LevelFrom> {
        let dyns: Vec<(Frac, Frac, Staff, LevelFrom)> = self
            .spans
            .iter()
            .filter_map(|s| {
                let from = match (s.lane, s.tok) {
                    (Lane::Mark(_, MarkKind::Dyn, 1), Tok::Dyn(d)) => LevelFrom::Mark(d.mark),
                    (Lane::Mark(_, MarkKind::Dyn, 1), Tok::DynText(d)) => LevelFrom::Text(d.level?),
                    _ => return None,
                };
                Some((
                    self.abs(s.start.0, s.start.1),
                    self.abs(s.end.0, s.end.1),
                    s.lane.staff()?,
                    from,
                ))
            })
            .collect();
        let applies = |start: Frac, st: Staff| {
            st == staff
                || !dyns
                    .iter()
                    .any(|(s2, _, st2, _)| *s2 == start && *st2 == staff)
        };
        let mut base = None;
        for (start, end, st, from) in &dyns {
            if *start > t || !applies(*start, *st) {
                continue;
            }
            let LevelFrom::Mark(mark) = from else {
                base = Some(*from);
                continue;
            };
            if !mark.is_momentary() {
                base = Some(*from);
            } else if t < *end {
                return Some(*from);
            } else {
                base = mark.after().map(LevelFrom::Mark).or(base);
            }
        }
        base
    }

    pub fn mark_velocity(&self, bar: usize, mark: crate::model::DynMark) -> f64 {
        self.bars[bar]
            .vel_marks
            .get(&mark.0)
            .copied()
            .unwrap_or_else(|| mark.default_velocity())
    }

    pub fn level_at(&self, staff: Staff, bar: usize, t: Frac) -> f64 {
        match self.mark_at(staff, self.abs(bar, t)) {
            Some(LevelFrom::Mark(m)) => self.mark_velocity(bar, m),
            Some(LevelFrom::Text(v)) => v,
            None => DEFAULT_VELOCITY,
        }
    }

    pub fn muted(&self, r: NoteRef) -> bool {
        let Some(ev) = self.leaves[r.leaf].event() else {
            return false;
        };
        let tok_marks = match r.part {
            Part::Main => Some(&ev.marks),
            Part::Grace(g) => ev
                .grace
                .as_ref()
                .and_then(|x| x.notes.get(g))
                .map(|n| &n.marks),
            Part::After(g) => ev
                .after
                .as_ref()
                .and_then(|x| x.notes.get(g))
                .map(|n| &n.marks),
        };
        tok_marks.is_some_and(|m| m.contains(&crate::model::TokMark::Mute))
            || self
                .notes_of(r)
                .is_some_and(|n| n.has(crate::model::PitchMark::Mute))
    }

    pub fn velocity(&self, r: NoteRef) -> Velocity {
        let leaf = &self.leaves[r.leaf];
        let level = self.level_at(Self::home_staff(leaf.lane), leaf.bar, leaf.start);
        let lane = self.bars[leaf.bar]
            .vel_lanes
            .get(&leaf.lane)
            .copied()
            .unwrap_or(0.0);
        let token = leaf.event().and_then(|ev| match r.part {
            Part::Main => ev.vel(),
            Part::Grace(g) => ev
                .grace
                .as_ref()?
                .notes
                .get(g)?
                .marks
                .iter()
                .find_map(vel_of),
            Part::After(g) => ev
                .after
                .as_ref()?
                .notes
                .get(g)?
                .marks
                .iter()
                .find_map(vel_of),
        });
        let note = self.notes_of(r).and_then(Note::vel);
        let sum = level + lane + token.unwrap_or(0.0) + note.unwrap_or(0.0);
        Velocity {
            mark: level,
            note: if self.muted(r) { 0.0 } else { sum },
        }
    }

    pub fn home_staff(lane: Lane) -> Staff {
        lane.staff().unwrap_or(Staff::Upper)
    }

    pub fn voice_count(&self, bar: usize, staff: Staff) -> usize {
        let has_notes = |els: &[El]| {
            flatten(els)
                .iter()
                .any(|(_, _, t)| matches!(t, Tok::Event(_)))
        };
        self.score.bars[bar]
            .lines
            .iter()
            .filter(|l| matches!(l.lane, Lane::Voice(s, _) if s == staff) && has_notes(&l.els))
            .count()
    }

    pub fn tuplet_look(&self, g: usize) -> TupletLook {
        match self.groups[g].head {
            GroupHead::Tuplet { look, .. } => look,
            GroupHead::Tremolo { .. } => TupletLook::Default,
        }
    }
}

pub const DEFAULT_VELOCITY: f64 = 68.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum LevelFrom {
    Mark(crate::model::DynMark),
    Text(f64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Velocity {
    pub mark: f64,
    pub note: f64,
}

fn vel_of(m: &crate::model::TokMark) -> Option<f64> {
    match m {
        crate::model::TokMark::Vel(v) => Some(*v),
        _ => None,
    }
}

fn has_pedal(els: &[El]) -> bool {
    flatten(els)
        .iter()
        .any(|(_, _, t)| matches!(t, Tok::Pedal(_) | Tok::Cont))
}

pub fn flatten(els: &[El]) -> Vec<(Frac, Frac, &Tok)> {
    let mut out = Vec::new();
    for e in els {
        match &e.kind {
            ElKind::Tok(t) => out.push((e.start, e.end(), t)),
            ElKind::Group(_, c) => out.extend(flatten(c)),
        }
    }
    out
}

fn describe(d: Frac) -> String {
    let unit = match d.den() {
        1 => "whole notes",
        2 => "halves",
        4 => "quarters",
        8 => "eighths",
        16 => "16ths",
        32 => "32nds",
        64 => "64ths",
        _ => return format!("{d} of a whole note"),
    };
    format!("{} {unit}", d.num())
}
