use std::collections::{BTreeMap, HashMap};

use super::read::{
    BeamState, DirKind, Edge, PedalKind, Source, SrcDir, SrcHead, SrcMeasure, SrcNote, SrcSlur,
    stop,
};
use super::report::{Report, Row};
use crate::analysis::Part;
use crate::frac::{Frac, NoteValue};
use crate::model::{
    AccDisplay, Attr, Bar, BarLayout, DynMark, DynText, El, ElKind, Event, Grace, GraceNote,
    GroupHead, Head, Key, Lane, Line, MarkKind, Meter, Note, Pedal, PedalStyle, PhraseMark,
    PitchMark, Place, Score, Slurs, Staff, Tok, TokMark,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct Keep {
    pub stems: bool,
    pub beams: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TokKey {
    pub bar: usize,
    pub lane: Lane,
    pub start: Frac,
}

#[derive(Clone, Debug, Default)]
pub struct TokSrc {
    pub end: Frac,
    pub tie_starts: Vec<(Part, usize)>,
    pub velocities: Vec<(Part, Vec<f64>)>,
    pub slurs: Vec<(Part, SrcSlur)>,
}

pub struct MarkVelocity {
    pub bar: usize,
    pub mark: DynMark,
    pub velocity: f64,
}

pub struct Built {
    pub score: Score,
    pub toks: BTreeMap<TokKey, TokSrc>,
    pub marks: Vec<MarkVelocity>,
    pub numbers: Vec<String>,
    pub rounded: Vec<bool>,
}

const DEFAULT_VOLUME: f64 = 100.0 / 127.0 * 100.0;

pub fn token_parts(ev: &Event) -> Vec<Part> {
    let mut out: Vec<Part> = (0..ev.grace.as_ref().map_or(0, |g| g.notes.len()))
        .map(Part::Grace)
        .collect();
    out.push(Part::Main);
    out.extend((0..ev.after.as_ref().map_or(0, |g| g.notes.len())).map(Part::After));
    out
}

pub fn part_notes(ev: &Event, p: Part) -> &[Note] {
    match p {
        Part::Main => ev.notes(),
        Part::Grace(i) => ev.grace.as_ref().map_or(&[], |g| &g.notes[i].notes),
        Part::After(i) => ev.after.as_ref().map_or(&[], |g| &g.notes[i].notes),
    }
}

struct Layout {
    layout: BarLayout,
    cells_attr: Option<u32>,
    second_part: bool,
}

fn meter_cells(meter: &mut Meter, m: &SrcMeasure, rep: &mut Report) -> Result<Vec<Frac>, String> {
    if let Ok(c) = meter.cells() {
        return Ok(c);
    }
    let num = meter.numerator();
    if !matches!(meter.beat_type, 8 | 16 | 32) || num < 2 {
        return Err(stop(&format!("the time signature {meter}"), &m.number));
    }
    let pattern = beam_pattern(meter, m).unwrap_or_else(|| {
        let threes = if num % 2 == 1 { 1 } else { 0 };
        let mut p = vec![2; ((num - 3 * threes) / 2) as usize];
        p.extend(std::iter::repeat_n(3, threes as usize));
        p
    });
    meter.pattern = Some(pattern);
    rep.note(format!(
        "measure {}: time={} gets the beat pattern of section 3.6: time={meter}",
        m.number,
        meter.numerator()
    ));
    meter
        .cells()
        .map_err(|e| stop(&format!("the time signature {meter}: {e}"), &m.number))
}

fn beam_pattern(meter: &Meter, m: &SrcMeasure) -> Option<Vec<u32>> {
    let unit = Frac::new(1, i64::from(meter.beat_type));
    let first = m.notes.first()?;
    let mut groups = Vec::new();
    let mut start = None;
    for n in m
        .notes
        .iter()
        .filter(|n| n.voice == first.voice && !n.chord && n.grace.is_none())
    {
        let b1 = n.beams.iter().find(|(l, _)| *l == 1).map(|(_, s)| *s);
        match b1 {
            Some(BeamState::Begin) => start = Some(n.start),
            Some(BeamState::End) => {
                let s = start.take()?;
                let len = (n.start + n.dur - s) / unit;
                groups.push(len);
            }
            Some(_) => {}
            None => groups.push(n.dur / unit),
        }
    }
    let p: Option<Vec<u32>> = groups
        .iter()
        .map(|g| g.is_int().then(|| g.num() as u32))
        .collect();
    let p = p?;
    (p.iter().all(|g| matches!(g, 2 | 3)) && p.iter().sum::<u32>() == meter.numerator())
        .then_some(p)
}

fn layout_of(
    meter: &Meter,
    cells: &[Frac],
    m: &SrcMeasure,
    first: bool,
    prev: Option<&Layout>,
) -> Result<Layout, String> {
    let nominal: Frac = cells.iter().copied().sum();
    let len = if m.end.is_zero() { nominal } else { m.end };
    let base = BarLayout {
        meter: meter.clone(),
        cells: cells.to_vec(),
        lead: Frac::ZERO,
        trail: Frac::ZERO,
        full: nominal,
    };
    if len == nominal {
        return Ok(Layout {
            layout: base,
            cells_attr: None,
            second_part: false,
        });
    }
    if len > nominal {
        let mut out = Vec::new();
        let mut sum = Frac::ZERO;
        for c in cells.iter().cycle() {
            if sum >= len {
                break;
            }
            out.push(*c);
            sum += *c;
        }
        if sum != len {
            return Err(stop(
                "a measure that is longer than its time signature by a part of a beat",
                &m.number,
            ));
        }
        return Ok(Layout {
            cells_attr: Some(out.len() as u32),
            layout: BarLayout { cells: out, ..base },
            second_part: false,
        });
    }
    let prev_open =
        prev.is_some_and(|p| p.layout.is_partial() && !p.second_part && p.cells_attr.is_none());
    let second = prev_open && prev.is_some_and(|p| p.layout.length() + len == nominal);
    let from_end = first || second;
    let mut k = 0;
    let mut sum = Frac::ZERO;
    while sum < len {
        let c = if from_end {
            cells[cells.len() - 1 - k]
        } else {
            cells[k]
        };
        sum += c;
        k += 1;
    }
    let (chosen, lead, trail) = if from_end {
        (cells[cells.len() - k..].to_vec(), sum - len, Frac::ZERO)
    } else {
        (cells[..k].to_vec(), Frac::ZERO, sum - len)
    };
    let whole_cells_at_end = !from_end && trail.is_zero();
    Ok(Layout {
        cells_attr: whole_cells_at_end.then_some(k as u32),
        layout: BarLayout {
            cells: chosen,
            lead,
            trail,
            ..base
        },
        second_part: second,
    })
}

struct VoiceLeaf<'s> {
    start: Frac,
    dur: Frac,
    src: Vec<&'s SrcNote>,
    graces: Vec<Vec<&'s SrcNote>>,
    afters: Vec<Vec<&'s SrcNote>>,
}

// MuseScore and pno number the voices 1 to 4 on the upper staff and 5 to 8 on the lower staff.
fn staff_by_number(voice: &str) -> Option<Staff> {
    match voice.parse::<u32>().ok()? {
        1..=4 => Some(Staff::Upper),
        5..=8 => Some(Staff::Lower),
        _ => None,
    }
}

fn group_voices<'s>(
    m: &'s SrcMeasure,
    number: &str,
    by_number: bool,
) -> Result<Vec<(Lane, Vec<&'s SrcNote>)>, String> {
    let mut order: Vec<String> = Vec::new();
    let mut by_voice: HashMap<String, Vec<&SrcNote>> = HashMap::new();
    for n in &m.notes {
        if !by_voice.contains_key(&n.voice) {
            order.push(n.voice.clone());
        }
        by_voice.entry(n.voice.clone()).or_default().push(n);
    }
    let mut staffs: [Vec<(String, Vec<&SrcNote>)>; 2] = [Vec::new(), Vec::new()];
    for v in order {
        let notes = by_voice.remove(&v).unwrap_or_default();
        let visible: Vec<&&SrcNote> = notes.iter().filter(|n| n.head != SrcHead::Hidden).collect();
        if visible.is_empty() {
            continue;
        }
        let upper = visible.iter().filter(|n| n.staff == Staff::Upper).count();
        let lower = visible.len() - upper;
        let by_count = match upper.cmp(&lower) {
            std::cmp::Ordering::Greater => Staff::Upper,
            std::cmp::Ordering::Less => Staff::Lower,
            std::cmp::Ordering::Equal => visible[0].staff,
        };
        let home = staff_by_number(&v)
            .filter(|_| by_number)
            .unwrap_or(by_count);
        staffs[usize::from(home.number() - 1)].push((v, notes));
    }
    let mut out = Vec::new();
    for (i, list) in staffs.iter_mut().enumerate() {
        let staff = if i == 0 { Staff::Upper } else { Staff::Lower };
        list.sort_by_key(|(v, _)| v.parse::<u32>().unwrap_or(u32::MAX));
        if list.len() > 4 {
            return Err(stop("more than four voices on a staff", number));
        }
        for (k, (_, notes)) in list.drain(..).enumerate() {
            out.push((Lane::Voice(staff, k as u8 + 1), notes));
        }
    }
    Ok(out)
}

fn voice_leaves<'s>(
    notes: &[&'s SrcNote],
    lead: Frac,
    number: &str,
) -> Result<Vec<VoiceLeaf<'s>>, String> {
    let mut leaves: Vec<VoiceLeaf> = Vec::new();
    let mut pending: Vec<Vec<&SrcNote>> = Vec::new();
    let mut last_grace_after = false;
    for n in notes {
        if let Some(g) = &n.grace {
            if n.chord {
                let target = if last_grace_after {
                    leaves.last_mut().and_then(|l| l.afters.last_mut())
                } else {
                    pending.last_mut()
                };
                target
                    .ok_or_else(|| stop("a grace chord note with no grace note", number))?
                    .push(n);
                continue;
            }
            let after = g.after && leaves.last().is_some() && pending.is_empty();
            if after {
                if let Some(l) = leaves.last_mut() {
                    l.afters.push(vec![n]);
                }
            } else {
                pending.push(vec![n]);
            }
            last_grace_after = after;
            continue;
        }
        if n.chord {
            let l = leaves
                .last_mut()
                .ok_or_else(|| stop("a chord note with no note before it", number))?;
            if l.start != n.start + lead {
                return Err(stop("a chord note at a different time", number));
            }
            l.src.push(n);
            continue;
        }
        leaves.push(VoiceLeaf {
            start: n.start + lead,
            dur: n.dur,
            src: vec![n],
            graces: std::mem::take(&mut pending),
            afters: Vec::new(),
        });
        last_grace_after = false;
    }
    if !pending.is_empty() {
        let l = leaves
            .last_mut()
            .ok_or_else(|| stop("grace notes with no main note", number))?;
        l.afters.append(&mut pending);
    }
    Ok(leaves)
}

struct Ctx<'r> {
    rep: &'r mut Report,
    keep: Keep,
    accidentals: bool,
    number: String,
}

impl Ctx<'_> {
    fn note(&mut self, n: &SrcNote, home: Staff) -> Option<Note> {
        let SrcHead::Pitch(pitch) = n.head else {
            return None;
        };
        let mut marks = n.pitch_marks.clone();
        if n.staff != home {
            marks.push(PitchMark::CrossStaff(n.staff));
        }
        if n.acc.is_none() && self.accidentals {
            marks.push(PitchMark::Hide);
        }
        Some(Note {
            tied: n.tie_stop,
            hidden: n.hidden,
            pitch,
            acc: n.acc.unwrap_or(AccDisplay::Default),
            marks,
        })
    }

    fn tok_marks(&mut self, notes: &[&SrcNote]) -> Vec<TokMark> {
        let mut out: Vec<TokMark> = Vec::new();
        for (i, n) in notes.iter().enumerate() {
            for m in &n.tok_marks {
                if out.contains(m) {
                    self.rep.drop(Row::Duplicate);
                } else {
                    out.push(*m);
                }
            }
            if n.small && i == 0 && n.grace.is_none() {
                out.push(TokMark::Small);
            }
            if n.stem.is_some() {
                if self.keep.stems {
                    if i == 0 {
                        out.extend(n.stem);
                    }
                } else {
                    self.rep.drop(Row::Stems);
                }
            }
            if !self.keep.beams {
                self.rep.drop_n(Row::Beams, n.beams.len());
            }
        }
        if self.keep.beams {
            match beam_state(notes, 1) {
                Some(BeamState::Begin) => out.push(TokMark::BeamBegin),
                Some(BeamState::End) => out.push(TokMark::BeamEnd),
                _ => {}
            }
        }
        out
    }

    fn grace_note(&mut self, notes: &[&SrcNote], home: Staff) -> GraceNote {
        let mut marks = self.tok_marks(notes);
        if let Some(g) = notes.first().and_then(|n| n.grace.as_ref()) {
            marks.push(TokMark::GraceValue(g.value));
        }
        GraceNote {
            notes: notes.iter().filter_map(|n| self.note(n, home)).collect(),
            marks,
            slurs: Slurs::default(),
        }
    }
}

struct Group {
    head: GroupHead,
    ratio: (u32, u32),
    members: Vec<usize>,
    implicit: bool,
    number: u32,
}

fn group_leaves(
    leaves: &[VoiceLeaf],
    number: &str,
) -> Result<(Vec<Vec<usize>>, Vec<GroupHead>), String> {
    let mut groups: Vec<Group> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut paths = Vec::new();
    for (i, leaf) in leaves.iter().enumerate() {
        let first = leaf.src[0];
        let outer = |open: &[usize], groups: &[Group]| {
            open.iter()
                .map(|g| Frac::new(i64::from(groups[*g].ratio.0), i64::from(groups[*g].ratio.1)))
                .fold(Frac::int(1), |a, b| a * b)
        };
        let close_implicit = |open: &mut Vec<usize>, groups: &[Group]| {
            while open.last().is_some_and(|g| groups[*g].implicit) {
                open.pop();
            }
        };
        if first.tuplets.iter().any(|t| t.edge == Edge::Start) || first.time_mod.is_none() {
            close_implicit(&mut open, &groups);
        }
        for t in first.tuplets.iter().filter(|t| t.edge == Edge::Start) {
            let ratio = match t.ratio {
                Some(r) => r,
                None => {
                    let (a, n) = first.time_mod.unwrap_or((1, 1));
                    let own = Frac::new(i64::from(a), i64::from(n)) / outer(&open, &groups);
                    if open.is_empty() {
                        (a, n)
                    } else {
                        (own.num() as u32, own.den() as u32)
                    }
                }
            };
            open.push(groups.len());
            groups.push(Group {
                head: GroupHead::Tuplet {
                    num: ratio.0,
                    ratio: Some(ratio.1),
                    look: t.look,
                },
                ratio,
                members: Vec::new(),
                implicit: false,
                number: t.number,
            });
        }
        if let Some((Edge::Start, strokes)) = first.tremolo {
            open.push(groups.len());
            groups.push(Group {
                head: GroupHead::Tremolo { strokes },
                ratio: (2, 1),
                members: Vec::new(),
                implicit: false,
                number: 0,
            });
        }
        let in_tremolo = open
            .iter()
            .any(|g| matches!(groups[*g].head, GroupHead::Tremolo { .. }));
        if let Some((a, n)) = first.time_mod.filter(|_| !in_tremolo) {
            let want = Frac::new(i64::from(a), i64::from(n));
            let have = outer(&open, &groups);
            if want != have {
                if !open.is_empty() && !groups[open[open.len() - 1]].implicit {
                    return Err(stop("a tuplet whose notes do not have its ratio", number));
                }
                close_implicit(&mut open, &groups);
                let own = want / outer(&open, &groups);
                open.push(groups.len());
                groups.push(Group {
                    head: GroupHead::Tuplet {
                        num: own.num() as u32,
                        ratio: Some(own.den() as u32),
                        look: crate::model::TupletLook::Default,
                    },
                    ratio: (own.num() as u32, own.den() as u32),
                    members: Vec::new(),
                    implicit: true,
                    number: 0,
                });
            }
        }
        for g in &open {
            groups[*g].members.push(i);
        }
        paths.push(open.clone());
        for t in first.tuplets.iter().filter(|t| t.edge == Edge::Stop) {
            let pos = open.iter().rposition(|g| {
                !groups[*g].implicit
                    && groups[*g].number == t.number
                    && matches!(groups[*g].head, GroupHead::Tuplet { .. })
            });
            match pos {
                Some(p) if p == open.len() - 1 => {
                    open.pop();
                }
                Some(_) => return Err(stop("tuplets that overlap with no nesting", number)),
                None => {}
            }
        }
        if let Some((Edge::Stop, _)) = first.tremolo {
            if open
                .last()
                .is_some_and(|g| matches!(groups[*g].head, GroupHead::Tremolo { .. }))
            {
                open.pop();
            }
        }
        if let Some(g) = open.last().copied().filter(|g| groups[*g].implicit) {
            let span: Frac = groups[g].members.iter().map(|m| leaves[*m].dur).sum();
            let (a, n) = groups[g].ratio;
            let written = span * Frac::new(i64::from(a), i64::from(n));
            if (written / i64::from(a)).is_pow2() {
                open.pop();
            }
        }
    }
    if open.iter().any(|g| !groups[*g].implicit) {
        return Err(stop("a tuplet across a barline", number));
    }
    Ok((paths, groups.iter().map(|g| g.head).collect()))
}

fn nest(items: Vec<(Vec<usize>, El)>, depth: usize, heads: &[GroupHead]) -> Vec<El> {
    let mut out: Vec<El> = Vec::new();
    let mut i = 0;
    let items: Vec<(Vec<usize>, El)> = items;
    while i < items.len() {
        let Some(g) = items[i].0.get(depth).copied() else {
            out.push(items[i].1.clone());
            i += 1;
            continue;
        };
        let mut j = i;
        while j < items.len() && items[j].0.get(depth) == Some(&g) {
            j += 1;
        }
        let members: Vec<(Vec<usize>, El)> = items[i..j].to_vec();
        let start = members[0].1.start;
        let end = members[members.len() - 1].1.end();
        let children = nest(members, depth + 1, heads);
        out.push(El {
            start,
            dur: end - start,
            kind: ElKind::Group(heads[g], children),
        });
        i = j;
    }
    out
}

fn fill(els: Vec<El>, from: Frac, to: Frac, gap: Tok) -> Vec<El> {
    let mut out = Vec::new();
    let mut t = from;
    for e in els {
        if e.start > t {
            out.push(El {
                start: t,
                dur: e.start - t,
                kind: ElKind::Tok(gap.clone()),
            });
        }
        t = e.end();
        out.push(e);
    }
    if to > t {
        out.push(El {
            start: t,
            dur: to - t,
            kind: ElKind::Tok(gap),
        });
    }
    out
}

fn fill_groups(els: Vec<El>) -> Vec<El> {
    els.into_iter()
        .map(|e| match e.kind {
            ElKind::Group(h, c) => {
                let end = e.start + e.dur;
                let c = fill(fill_groups(c), e.start, end, Tok::Empty);
                El {
                    start: e.start,
                    dur: e.dur,
                    kind: ElKind::Group(h, c),
                }
            }
            k => El { kind: k, ..e },
        })
        .collect()
}

pub fn with_x(els: Vec<El>, layout: &BarLayout) -> Vec<El> {
    let mut out = Vec::new();
    if !layout.lead.is_zero() {
        out.push(El {
            start: Frac::ZERO,
            dur: layout.lead,
            kind: ElKind::Tok(Tok::Gap),
        });
    }
    let end = layout.total() - layout.trail;
    out.extend(fill(els, layout.lead, end, Tok::Empty));
    if !layout.trail.is_zero() {
        out.push(El {
            start: end,
            dur: layout.trail,
            kind: ElKind::Tok(Tok::Gap),
        });
    }
    out
}

fn factor(path: &[usize], groups_ratio: &[(u32, u32)]) -> Frac {
    path.iter()
        .map(|g| Frac::new(i64::from(groups_ratio[*g].0), i64::from(groups_ratio[*g].1)))
        .fold(Frac::int(1), |a, b| a * b)
}

fn ratio_of(h: GroupHead) -> (u32, u32) {
    match h {
        GroupHead::Tuplet { num, ratio, .. } => (num, ratio.unwrap_or(1)),
        GroupHead::Tremolo { .. } => (2, 1),
    }
}

fn voice_line(
    cx: &mut Ctx,
    bar: usize,
    lane: Lane,
    notes: &[&SrcNote],
    layout: &BarLayout,
    level: &dyn Fn(Frac) -> f64,
    toks: &mut BTreeMap<TokKey, TokSrc>,
) -> Result<Vec<El>, String> {
    let number = cx.number.clone();
    let home = lane.staff().unwrap_or(Staff::Upper);
    let leaves = voice_leaves(notes, layout.lead, &number)?;
    let (paths, heads) = group_leaves(&leaves, &number)?;
    let ratios: Vec<(u32, u32)> = heads.iter().map(|h| ratio_of(*h)).collect();
    let mut cursor = layout.lead;
    let mut items: Vec<(Vec<usize>, El)> = Vec::new();
    let only_bar_rest = leaves.len() == 1
        && leaves[0].src[0].head == SrcHead::BarRest
        && leaves[0].start == layout.lead
        && leaves[0].dur == layout.length();
    for (leaf, path) in leaves.iter().zip(&paths) {
        if leaf.start < cursor {
            return Err(stop("notes that overlap in one voice", &number));
        }
        cursor = leaf.start + leaf.dur;
        let first = leaf.src[0];
        let f = factor(path, &ratios);
        if first.head != SrcHead::Hidden && first.head != SrcHead::BarRest {
            let written = leaf.dur * f;
            let ok = match first.written {
                Some(w) => NoteValue::of(written) == Some(w),
                None => NoteValue::of(written).is_some(),
            };
            if !ok {
                return Err(stop(
                    "a note whose written value does not match its duration",
                    &number,
                ));
            }
        }
        let head = match &first.head {
            SrcHead::Hidden => {
                items.push((
                    path.clone(),
                    El {
                        start: leaf.start,
                        dur: leaf.dur,
                        kind: ElKind::Tok(Tok::Empty),
                    },
                ));
                continue;
            }
            SrcHead::BarRest if only_bar_rest => Head::BarRest,
            SrcHead::Rest | SrcHead::BarRest => {
                Head::Rest((first.staff != home).then_some(first.staff))
            }
            SrcHead::Pitch(_) => {
                Head::Chord(leaf.src.iter().filter_map(|n| cx.note(n, home)).collect())
            }
        };
        let marks = cx.tok_marks(&leaf.src);
        let grace = (!leaf.graces.is_empty()).then(|| Grace {
            slash: leaf.graces[0][0].grace.as_ref().is_some_and(|g| g.slash),
            notes: leaf.graces.iter().map(|g| cx.grace_note(g, home)).collect(),
        });
        let after = (!leaf.afters.is_empty()).then(|| Grace {
            slash: false,
            notes: leaf.afters.iter().map(|g| cx.grace_note(g, home)).collect(),
        });
        let ev = Event {
            grace,
            head,
            marks,
            slurs: Slurs::default(),
            after,
        };
        let mut src = TokSrc {
            end: leaf.start + leaf.dur,
            ..TokSrc::default()
        };
        let mut record = |part: Part, notes: &[&SrcNote]| {
            let pitched: Vec<&&SrcNote> = notes
                .iter()
                .filter(|n| matches!(n.head, SrcHead::Pitch(_)))
                .collect();
            for (k, n) in pitched.iter().enumerate() {
                if n.tie_start {
                    src.tie_starts.push((part, k));
                }
            }
            let vels: Vec<f64> = pitched
                .iter()
                .map(|n| n.velocity.unwrap_or_else(|| level(n.start + layout.lead)))
                .collect();
            src.velocities.push((part, vels));
            for n in notes {
                src.slurs.extend(n.slurs.iter().map(|s| (part, *s)));
            }
        };
        for (i, g) in leaf.graces.iter().enumerate() {
            record(Part::Grace(i), g);
        }
        record(Part::Main, &leaf.src);
        for (i, g) in leaf.afters.iter().enumerate() {
            record(Part::After(i), g);
        }
        toks.insert(
            TokKey {
                bar,
                lane,
                start: leaf.start,
            },
            src,
        );
        let el = El {
            start: leaf.start,
            dur: leaf.dur,
            kind: ElKind::Tok(Tok::Event(Box::new(ev))),
        };
        if head_is_bar_rest(&el) {
            return Ok(vec![el]);
        }
        items.push((path.clone(), el));
    }
    if cx.keep.beams {
        split_beams(&leaves, &mut items);
    }
    let nested = fill_groups(nest(items, 0, &heads));
    let end = layout.total() - layout.trail;
    if cursor > end {
        return Err(stop("a voice that is longer than its measure", &number));
    }
    Ok(with_x(nested, layout))
}

fn beam_state(notes: &[&SrcNote], level: u8) -> Option<BeamState> {
    notes
        .iter()
        .flat_map(|n| &n.beams)
        .find(|(l, _)| *l == level)
        .map(|(_, s)| *s)
}

// `:b1`: the primary beam continues, and the secondary beams end here and start again on the next note.
fn split_beams(leaves: &[VoiceLeaf], items: &mut [(Vec<usize>, El)]) {
    let mut events: Vec<(usize, usize)> = Vec::new();
    for (li, leaf) in leaves.iter().enumerate() {
        if let Some(ii) = items.iter().position(|(_, e)| {
            e.start == leaf.start && matches!(e.kind, ElKind::Tok(Tok::Event(_)))
        }) {
            events.push((li, ii));
        }
    }
    for w in events.windows(2) {
        let (a, b) = (&leaves[w[0].0], &leaves[w[1].0]);
        let primary = matches!(
            beam_state(&a.src, 1),
            Some(BeamState::Begin | BeamState::Continue)
        );
        let ends = matches!(
            beam_state(&a.src, 2),
            Some(BeamState::End | BeamState::BackwardHook)
        );
        let starts = matches!(
            beam_state(&b.src, 2),
            Some(BeamState::Begin | BeamState::ForwardHook)
        );
        if primary && ends && starts {
            if let ElKind::Tok(Tok::Event(ev)) = &mut items[w[0].1].1.kind {
                ev.marks.push(TokMark::BeamBreak);
            }
        }
    }
}

fn head_is_bar_rest(e: &El) -> bool {
    matches!(&e.kind, ElKind::Tok(Tok::Event(ev)) if ev.head == Head::BarRest)
}

struct Item {
    start: Frac,
    end: Option<Frac>,
    tok: Tok,
}

#[derive(Clone)]
struct OpenSpan {
    lane: Lane,
    number: u32,
    kind: SpanKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpanKind {
    Wedge,
    Dashes,
    Octave,
}

#[derive(Default)]
struct Marks {
    lanes: BTreeMap<Lane, Vec<Item>>,
}

impl Marks {
    fn busy(&self, lane: Lane, t: Frac, open: &[OpenSpan]) -> bool {
        let starts = self.lanes.get(&lane).is_some_and(|items| {
            items
                .iter()
                .any(|i| i.start == t || i.end.is_some_and(|e| i.start < t && t < e))
        });
        starts || open.iter().any(|o| o.lane == lane)
    }

    fn lane_for(&self, staff: Staff, kind: MarkKind, t: Frac, open: &[OpenSpan]) -> Option<Lane> {
        let second = matches!(kind, MarkKind::Dyn | MarkKind::Text | MarkKind::Hairpin);
        let lanes: &[u8] = if second { &[1, 2] } else { &[1] };
        lanes
            .iter()
            .map(|n| Lane::Mark(staff, kind, *n))
            .find(|l| !self.busy(*l, t, open))
    }

    fn push(&mut self, lane: Lane, item: Item) {
        self.lanes.entry(lane).or_default().push(item);
    }
}

fn mark_lines(
    cx: &mut Ctx,
    m: &SrcMeasure,
    layout: &BarLayout,
    onsets: &[(Staff, Frac)],
    open: &mut Vec<OpenSpan>,
    bar: usize,
    marks_out: &mut Vec<MarkVelocity>,
) -> Result<Vec<Line>, String> {
    let number = cx.number.clone();
    let lead = layout.lead;
    let end = layout.total() - layout.trail;
    let mut marks = Marks::default();
    let carried = std::mem::take(open);
    for o in &carried {
        marks.push(
            o.lane,
            Item {
                start: lead,
                end: None,
                tok: Tok::Cont,
            },
        );
    }
    *open = carried;
    let close =
        |marks: &mut Marks, open: &mut Vec<OpenSpan>, kind: SpanKind, number: u32, t: Frac| {
            let Some(i) = open
                .iter()
                .position(|o| o.kind == kind && o.number == number)
            else {
                return false;
            };
            let o = open.remove(i);
            if let Some(item) = marks.lanes.get_mut(&o.lane).and_then(|v| {
                v.iter_mut()
                    .rev()
                    .find(|x| x.end.is_none() && !x.tok.is_point())
            }) {
                item.end = Some(t);
            }
            true
        };
    for (t, staff, clef) in &m.clefs {
        marks.push(
            Lane::Mark(*staff, MarkKind::Clef, 1),
            Item {
                start: *t + lead,
                end: None,
                tok: Tok::Clef(*clef),
            },
        );
    }
    let mut dirs: Vec<&SrcDir> = m.dirs.iter().collect();
    // A span that stops at a time frees its lane for a mark that starts at that time.
    dirs.sort_by_key(|d| (d.t, !d.kind.is_span_stop()));
    for d in dirs {
        let t = d.t + lead;
        if t > end {
            return Err(stop("a direction after the end of the measure", &number));
        }
        match &d.kind {
            DirKind::Dyn { mark, velocity } => {
                let lane = marks
                    .lane_for(d.staff, MarkKind::Dyn, t, open)
                    .ok_or_else(|| stop("three dynamic marks at one time", &number))?;
                let item_end = mark.mark.is_momentary().then(|| {
                    onsets
                        .iter()
                        .filter(|(s, o)| *s == d.staff && *o > t)
                        .map(|(_, o)| *o)
                        .min()
                        .unwrap_or(end)
                });
                if lane == Lane::Mark(d.staff, MarkKind::Dyn, 1) {
                    marks_out.push(MarkVelocity {
                        bar,
                        mark: mark.mark,
                        velocity: *velocity,
                    });
                }
                marks.push(
                    lane,
                    Item {
                        start: t,
                        end: item_end,
                        tok: Tok::Dyn(mark.clone()),
                    },
                );
            }
            DirKind::DynText(text) => {
                let lane = marks
                    .lane_for(d.staff, MarkKind::Dyn, t, open)
                    .ok_or_else(|| stop("three dynamic marks at one time", &number))?;
                marks.push(
                    lane,
                    Item {
                        start: t,
                        end: None,
                        tok: Tok::DynText(text.clone()),
                    },
                );
            }
            DirKind::Wedge {
                start: Some(h),
                number: n,
                place,
            } => {
                let lane = marks
                    .lane_for(d.staff, MarkKind::Hairpin, t, open)
                    .ok_or_else(|| stop("three hairpins at one time", &number))?;
                marks.push(
                    lane,
                    Item {
                        start: t,
                        end: None,
                        tok: Tok::Hairpin(*h, place.filter(|p| *p != Place::Below)),
                    },
                );
                open.push(OpenSpan {
                    lane,
                    number: *n,
                    kind: SpanKind::Wedge,
                });
            }
            DirKind::Wedge {
                start: None,
                number: n,
                ..
            } => {
                if !close(&mut marks, open, SpanKind::Wedge, *n, t) {
                    cx.rep.note(format!(
                        "measure {number}: a hairpin end with no start is dropped"
                    ));
                }
            }
            DirKind::Text { text, dashes } => {
                let lane = marks
                    .lane_for(d.staff, MarkKind::Text, t, open)
                    .ok_or_else(|| stop("three texts at one time", &number))?;
                let mut text = text.clone();
                text.ext = dashes.is_some();
                marks.push(
                    lane,
                    Item {
                        start: t,
                        end: None,
                        tok: Tok::Text(text),
                    },
                );
                if let Some(n) = dashes {
                    open.push(OpenSpan {
                        lane,
                        number: *n,
                        kind: SpanKind::Dashes,
                    });
                }
            }
            DirKind::Tempo { tempo, dashes } => {
                if marks.busy(Lane::Tempo, t, open) {
                    return Err(stop("two tempo marks at one time", &number));
                }
                let mut tempo = tempo.clone();
                tempo.ext = dashes.is_some() && tempo.text.is_some();
                marks.push(
                    Lane::Tempo,
                    Item {
                        start: t,
                        end: None,
                        tok: Tok::Tempo(tempo.clone()),
                    },
                );
                if let (Some(n), true) = (dashes, tempo.ext) {
                    open.push(OpenSpan {
                        lane: Lane::Tempo,
                        number: *n,
                        kind: SpanKind::Dashes,
                    });
                }
            }
            DirKind::DashesStop { number: n } => {
                if !close(&mut marks, open, SpanKind::Dashes, *n, t) {
                    cx.rep.note(format!(
                        "measure {number}: a dashed line end with no start is dropped"
                    ));
                }
            }
            DirKind::Octave {
                start: Some(o),
                number: n,
            } => {
                let lane = Lane::Mark(d.staff, MarkKind::Ottava, 1);
                if marks.busy(lane, t, open) {
                    return Err(stop("two ottava lines on one staff", &number));
                }
                marks.push(
                    lane,
                    Item {
                        start: t,
                        end: None,
                        tok: Tok::Ottava(*o),
                    },
                );
                open.push(OpenSpan {
                    lane,
                    number: *n,
                    kind: SpanKind::Octave,
                });
            }
            DirKind::Octave {
                start: None,
                number: n,
            } => {
                if !close(&mut marks, open, SpanKind::Octave, *n, t) {
                    cx.rep.note(format!(
                        "measure {number}: an ottava end with no start is dropped"
                    ));
                }
            }
            DirKind::Pedal(..) => {}
        }
    }
    let mut lines = Vec::new();
    for (lane, mut items) in marks.lanes {
        items.sort_by_key(|i| i.start);
        let mut els = Vec::new();
        for k in 0..items.len() {
            let next = items.get(k + 1).map_or(end, |n| n.start);
            let it = &items[k];
            let stop_at = match it.end {
                Some(e) => e.min(next),
                None if it.tok.is_point() || matches!(it.tok, Tok::Dyn(_)) => next,
                None => end,
            };
            if stop_at > next {
                return Err(stop("marks of one type that overlap", &number));
            }
            if stop_at <= it.start {
                continue;
            }
            els.push(El {
                start: it.start,
                dur: stop_at - it.start,
                kind: ElKind::Tok(it.tok.clone()),
            });
        }
        if els.is_empty() {
            continue;
        }
        shorten_momentary(&mut els, layout);
        lines.push(Line {
            lane,
            src: 0,
            els: with_x(els, layout),
        });
    }
    Ok(lines)
}

// A breath mark or caesura on a note starts a phrase at the end of the note; at the end
// of the measure, in the next measure.
fn phrase_line(
    m: &SrcMeasure,
    layout: &BarLayout,
    bar: usize,
    fields: &[(usize, Frac, PhraseMark)],
    pause: &mut Option<PhraseMark>,
    rep: &mut Report,
) -> Option<Line> {
    let lead = layout.lead;
    let end = layout.total() - layout.trail;
    let mut marks: BTreeMap<Frac, PhraseMark> = BTreeMap::new();
    if let Some(p) = pause.take() {
        marks.insert(lead, p);
    }
    for (_, t, p) in fields.iter().filter(|f| f.0 == bar) {
        if lead <= *t && *t < end {
            marks.insert(*t, *p);
        } else {
            rep.note(format!(
                "measure {}: a phrase field outside the measure is dropped",
                m.number
            ));
        }
    }
    for n in m.notes.iter().filter(|n| n.pause.is_some()) {
        let Some(p) = n.pause else { continue };
        if n.grace.is_some() {
            rep.note(format!(
                "measure {}: a {} on a grace note is dropped",
                m.number,
                p.text()
            ));
            continue;
        }
        let t = n.start + n.dur + lead;
        if t >= end {
            *pause = Some(p);
        } else {
            marks.insert(t, p);
        }
    }
    let ends: Vec<Frac> = marks.keys().skip(1).copied().chain([end]).collect();
    let els: Vec<El> = marks
        .iter()
        .zip(ends)
        .map(|((t, p), next)| El {
            start: *t,
            dur: next - *t,
            kind: ElKind::Tok(Tok::Phrase(*p)),
        })
        .collect();
    if els.is_empty() {
        return None;
    }
    Some(Line {
        lane: Lane::Phrase,
        src: 0,
        els: with_x(els, layout),
    })
}

// A mark for one moment ends at or before the next onset, so one slot of its cell holds the same notes.
fn shorten_momentary(els: &mut [El], layout: &BarLayout) {
    let mut bounds = layout.cell_starts();
    bounds.push(layout.total());
    for i in 0..els.len() {
        let ElKind::Tok(Tok::Dyn(d)) = &els[i].kind else {
            continue;
        };
        if !d.mark.is_momentary() {
            continue;
        }
        let start = els[i].start;
        let Some(cell) = bounds.windows(2).find(|w| w[0] <= start && start < w[1]) else {
            continue;
        };
        let (cs, ce) = (cell[0], cell[1]);
        let slot = els
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .flat_map(|(_, e)| [e.start, e.start + e.dur])
            .filter(|t| cs < *t && *t < ce)
            .chain([start])
            .fold(ce - cs, |g, t| g.gcd(t - cs));
        let cell = ce - cs;
        let fits = |n: &&i64| {
            let unit = cell / **n;
            unit <= els[i].dur && (slot / unit).is_int()
        };
        if let Some(n) = crate::text::write::cell_counts(cell).iter().find(fits) {
            els[i].dur = cell / *n;
        }
    }
}

fn pedal_line(
    m: &SrcMeasure,
    layout: &BarLayout,
    carried: &mut bool,
    rep: &mut Report,
) -> Result<Line, String> {
    let lead = layout.lead;
    let end = layout.total() - layout.trail;
    let mut events: Vec<(Frac, PedalKind, PedalStyle)> = m
        .dirs
        .iter()
        .filter_map(|d| match d.kind {
            DirKind::Pedal(k, style) => Some((d.t + lead, k, style)),
            _ => None,
        })
        .collect();
    events.sort_by_key(|e| (e.0, e.1 != PedalKind::Stop));
    let mut els = Vec::new();
    let mut cur: Option<(Frac, Tok)> = carried.then_some((lead, Tok::Cont));
    let mut close = |cur: &mut Option<(Frac, Tok)>, t: Frac| {
        if let Some((s, tok)) = cur.take() {
            if t > s {
                els.push(El {
                    start: s,
                    dur: t - s,
                    kind: ElKind::Tok(tok),
                });
            }
        }
    };
    for (t, kind, style) in events {
        let t = t.min(end);
        match kind {
            PedalKind::Start if cur.is_some() => {
                // A second `Ped.` with no `*` before it changes the pedal; pedal lines that overlap are unclear.
                if style == PedalStyle::Line {
                    return Err(stop("pedal lines that overlap", &m.number));
                }
                rep.drop(Row::PedalSignChange);
                close(&mut cur, t);
                cur = Some((t, Tok::Pedal(Pedal::Change)));
            }
            PedalKind::Start => cur = Some((t, Tok::Pedal(Pedal::Press))),
            PedalKind::Change => {
                let held = cur.is_some();
                close(&mut cur, t);
                let p = if held { Pedal::Change } else { Pedal::Press };
                cur = Some((t, Tok::Pedal(p)));
            }
            PedalKind::Stop if cur.is_none() && style == PedalStyle::Sign => {
                rep.drop(Row::PedalSignChange);
            }
            PedalKind::Stop => close(&mut cur, t),
        }
    }
    *carried = cur.is_some();
    close(&mut cur, end);
    Ok(Line {
        lane: Lane::Ped,
        src: 0,
        els: with_x(els, layout),
    })
}

fn source_levels(
    m: &SrcMeasure,
    lead: Frac,
    carried: f64,
) -> (Vec<(Frac, f64, Option<DynMark>)>, f64) {
    let mut marks: Vec<(Frac, f64, Option<DynMark>)> = m
        .dirs
        .iter()
        .filter_map(|d| match &d.kind {
            DirKind::DynText(DynText { level: Some(v), .. }) => Some((d.t + lead, *v, None)),
            DirKind::Dyn { mark, velocity: v } => Some((
                d.t + lead,
                *v,
                mark.mark.is_momentary().then_some(mark.mark),
            )),
            _ => None,
        })
        .collect();
    marks.sort_by_key(|m| m.0);
    let mut last = carried;
    for (_, v, mom) in &marks {
        last = match mom {
            None => *v,
            Some(d) => d.after().map_or(last, DynMark::default_velocity),
        };
    }
    (marks, last)
}

fn level_at(marks: &[(Frac, f64, Option<DynMark>)], carried: f64, t: Frac) -> f64 {
    let mut level = carried;
    for (at, v, mom) in marks {
        if *at > t {
            break;
        }
        level = match mom {
            None => *v,
            Some(_) if *at == t => *v,
            Some(d) => d.after().map_or(level, DynMark::default_velocity),
        };
    }
    level
}

pub fn build(src: &Source, keep: Keep, rep: &mut Report) -> Result<Built, String> {
    let mut header = vec![Attr::Format(1)];
    for (f, t) in &src.meta {
        header.push(Attr::Meta(*f, t.clone()));
    }
    if let Some(p) = src.program.filter(|p| *p != 1) {
        header.push(Attr::Program(p));
    }
    if let Some(v) = src.volume.filter(|v| (v - DEFAULT_VOLUME).abs() > 0.01) {
        header.push(Attr::Volume(v));
    }
    if let Some(p) = src.pan.filter(|p| p.abs() > 0.001) {
        header.push(Attr::Pan(p));
    }
    let mut cx = Ctx {
        rep,
        keep,
        accidentals: src.accidentals_stated,
        number: String::new(),
    };
    let mut meter = Meter::four_four();
    let mut key: Option<Key> = None;
    let mut bars: Vec<Bar> = Vec::new();
    let mut layouts: Vec<Layout> = Vec::new();
    let mut toks = BTreeMap::new();
    let mut marks_out = Vec::new();
    let mut open: Vec<OpenSpan> = Vec::new();
    let mut pedal_held = false;
    let mut pedal_style = None;
    let mut pause = None;
    let mut level = crate::analysis::DEFAULT_VELOCITY;
    let mut numbers = Vec::new();
    let mut rounded = Vec::new();
    let by_number = src
        .measures
        .iter()
        .flat_map(|m| &m.notes)
        .any(|n| n.voice.parse::<u32>().is_ok_and(|v| v > 4));
    for (bi, m) in src.measures.iter().enumerate() {
        cx.number = m.number.clone();
        numbers.push(m.number.clone());
        rounded.push(m.rounded);
        let mut attrs: Vec<Attr> = Vec::new();
        if let Some(k) = m.key {
            if bi == 0 {
                header.push(Attr::Key(k));
            } else if key != Some(k) {
                attrs.push(Attr::Key(k));
            }
            key = Some(k);
        }
        let mut meter_changed = false;
        if let Some(mt) = &m.meter {
            let same = mt.beats == meter.beats
                && mt.beat_type == meter.beat_type
                && mt.symbol == meter.symbol;
            if bi == 0 || !same {
                meter = mt.clone();
                meter_changed = true;
            }
        }
        let cells = meter_cells(&mut meter, m, cx.rep)?;
        if meter_changed {
            if bi == 0 {
                header.push(Attr::Time(meter.clone()));
            } else {
                attrs.push(Attr::Time(meter.clone()));
            }
        }
        for a in &m.attrs {
            match a {
                Attr::Break(_) if bi == 0 => {}
                _ => attrs.push(a.clone()),
            }
        }
        let lay = layout_of(&meter, &cells, m, bi == 0, layouts.last())?;
        if let Some(n) = lay.cells_attr {
            attrs.push(Attr::Cells(n));
        }
        let layout = lay.layout.clone();
        let (lv_marks, next_level) = source_levels(m, layout.lead, level);
        let carried_level = level;
        level = next_level;
        let at = |t: Frac| level_at(&lv_marks, carried_level, t);
        let mut lines = Vec::new();
        let voices = group_voices(m, &m.number, by_number)?;
        let mut onsets = Vec::new();
        for (lane, notes) in &voices {
            for n in notes.iter().filter(|n| n.grace.is_none() && !n.chord) {
                onsets.push((lane.staff().unwrap_or(Staff::Upper), n.start + layout.lead));
            }
            let els = voice_line(&mut cx, bi, *lane, notes, &layout, &at, &mut toks)?;
            lines.push(Line {
                lane: *lane,
                src: 0,
                els,
            });
        }
        for staff in [Staff::Upper, Staff::Lower] {
            let lane = Lane::Voice(staff, 1);
            if !lines.iter().any(|l: &Line| l.lane == lane) {
                lines.push(Line {
                    lane,
                    src: 0,
                    els: with_x(Vec::new(), &layout),
                });
            }
        }
        lines.extend(mark_lines(
            &mut cx,
            m,
            &layout,
            &onsets,
            &mut open,
            bi,
            &mut marks_out,
        )?);
        lines.extend(phrase_line(
            m,
            &layout,
            bi,
            &src.phrase_marks,
            &mut pause,
            cx.rep,
        ));
        lines.push(pedal_line(m, &layout, &mut pedal_held, cx.rep)?);
        pedal_style = pedal_style.or(m.pedal_style);
        bars.push(Bar {
            label: if lay.second_part {
                "b".into()
            } else {
                String::new()
            },
            src: 0,
            attrs,
            lines,
            layout,
        });
        layouts.push(lay);
    }
    if !open.is_empty() {
        cx.rep
            .note("a hairpin, dashed line or ottava with no end ends at the last bar".into());
    }
    if let Some(p) = pause {
        cx.rep
            .note(format!("a {} after the last note is dropped", p.text()));
    }
    let beyond = src
        .phrase_marks
        .iter()
        .filter(|f| f.0 >= src.measures.len())
        .count();
    if beyond > 0 {
        cx.rep.note(format!(
            "{beyond} phrase fields after the last measure are dropped"
        ));
    }
    if let Some(style) = pedal_style {
        header.push(Attr::PedalStyle(style));
    }
    let mut score = Score { header, bars };
    let labels = crate::check::expected_labels(&score);
    let mut changed = 0;
    for ((bar, label), number) in score.bars.iter_mut().zip(labels).zip(&numbers) {
        if label != *number {
            changed += 1;
        }
        bar.label = label;
    }
    cx.rep.drop_n(Row::Labels, changed);
    pedal_rule(&mut score);
    Ok(Built {
        score,
        toks,
        marks: marks_out,
        numbers,
        rounded,
    })
}

fn pedal_rule(score: &mut Score) {
    let mut ranges: Vec<Vec<usize>> = vec![Vec::new()];
    for (i, bar) in score.bars.iter().enumerate() {
        if i > 0 && bar.attrs.iter().any(|a| matches!(a, Attr::Time(_))) {
            ranges.push(Vec::new());
        }
        if let Some(r) = ranges.last_mut() {
            r.push(i);
        }
    }
    for range in ranges {
        let applies: Vec<usize> = range
            .iter()
            .copied()
            .filter(|b| {
                let bar = &score.bars[*b];
                !bar.layout.is_partial() && !bar.attrs.iter().any(|a| matches!(a, Attr::Cells(_)))
            })
            .collect();
        let line_of = |b: usize| {
            score.bars[b]
                .lines
                .iter()
                .find(|l| l.lane == Lane::Ped)
                .map(|l| l.els.clone())
        };
        let mut counts: Vec<(Vec<El>, usize)> = Vec::new();
        for b in &applies {
            let Some(els) = line_of(*b) else { continue };
            let has_pedal = els
                .iter()
                .any(|e| matches!(e.kind, ElKind::Tok(Tok::Pedal(_) | Tok::Cont)));
            if !has_pedal {
                continue;
            }
            match counts.iter_mut().find(|(e, _)| *e == els) {
                Some((_, n)) => *n += 1,
                None => counts.push((els, 1)),
            }
        }
        let Some((rule, n)) = counts.into_iter().max_by_key(|(_, n)| *n) else {
            continue;
        };
        if n * 2 <= applies.len() {
            continue;
        }
        let Some(first) = applies.first().copied() else {
            continue;
        };
        score.bars[first].lines.push(Line {
            lane: Lane::PedRule,
            src: 0,
            els: rule,
        });
    }
}
