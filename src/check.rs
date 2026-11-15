use crate::analysis::{Analysis, NoteRef, Part};
use crate::diag::Diag;
use crate::frac::Frac;
use crate::model::{Lane, Score, Staff};

// More than a tenth in one hand at one time.
const PIANO: std::ops::RangeInclusive<i32> = 21..=108;
const WIDE_SPAN: i32 = 16;

pub fn expected_labels(score: &Score) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(score.bars.len());
    let mut n = 0u32;
    for (i, bar) in score.bars.iter().enumerate() {
        let l = &bar.layout;
        let short = l.is_partial();
        let prev = i.checked_sub(1).map(|p| &score.bars[p]);
        let prev_open = prev.is_some_and(|p| p.layout.is_partial() && !out[i - 1].ends_with('b'));
        let second_part =
            short && prev_open && i > 0 && (!l.lead.is_zero() || bar.label.ends_with('b'));
        if second_part {
            out.push(format!("{}b", out[i - 1]));
            continue;
        }
        let pickup = i == 0 && short && l.trail.is_zero();
        if !pickup {
            n += 1;
        }
        out.push(n.to_string());
    }
    out
}

pub fn check(score: &Score, parse_diags: &[Diag]) -> Vec<Diag> {
    let mut diags: Vec<Diag> = parse_diags.to_vec();
    let a = Analysis::new(score);
    diags.extend(a.diags.iter().cloned());
    let labels = expected_labels(score);
    for (bar, want) in score.bars.iter().zip(&labels) {
        if bar.label != *want {
            diags.push(Diag::error(
                bar.src,
                bar.label.clone(),
                format!(
                    "this bar is number {want} by its position. Run `pno fmt --fix` to renumber"
                ),
            ));
        }
        for staff in [Staff::Upper, Staff::Lower] {
            let lane = Lane::Voice(staff, 1);
            let has = bar.lines.iter().any(|l| l.lane == lane);
            let failed = parse_diags
                .iter()
                .any(|d| d.at.starts_with(&format!("{} {lane}", bar.label)));
            if !has && !failed {
                diags.push(Diag::error(
                    bar.src,
                    bar.label.clone(),
                    format!("the bar has no {lane} line; write `| R |` for a rest"),
                ));
            }
        }
    }
    velocity_range(&a, &mut diags);
    hand_warnings(&a, &mut diags);
    diags.sort_by_key(|d| (d.line, d.severity));
    diags.dedup();
    diags
}

fn note_refs(a: &Analysis, leaf: usize) -> Vec<NoteRef> {
    let Some(ev) = a.leaves[leaf].event() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (gi, gn) in ev.grace.iter().flat_map(|g| g.notes.iter()).enumerate() {
        out.extend((0..gn.notes.len()).map(|n| NoteRef {
            leaf,
            part: Part::Grace(gi),
            note: n,
        }));
    }
    out.extend((0..ev.notes().len()).map(|n| NoteRef {
        leaf,
        part: Part::Main,
        note: n,
    }));
    for (gi, gn) in ev.after.iter().flat_map(|g| g.notes.iter()).enumerate() {
        out.extend((0..gn.notes.len()).map(|n| NoteRef {
            leaf,
            part: Part::After(gi),
            note: n,
        }));
    }
    out
}

fn velocity_range(a: &Analysis, diags: &mut Vec<Diag>) {
    for (i, leaf) in a.leaves.iter().enumerate() {
        for r in note_refs(a, i) {
            let v = a.velocity(r).note;
            if (1.0..=127.0).contains(&v) || a.muted(r) {
                continue;
            }
            let pitch = a
                .notes_of(r)
                .map(|n| n.pitch.to_string())
                .unwrap_or_default();
            let loc =
                crate::analysis::at(a.score, leaf.bar, leaf.lane, a.cell(leaf.bar, leaf.start));
            diags.push(Diag::error(
                leaf.src,
                loc,
                format!("the velocity of {pitch} is {v}; it must be 1 to 127"),
            ));
        }
    }
}

fn sounding(a: &Analysis, bar: usize, staff: Staff, t: Frac) -> Vec<i32> {
    a.leaves
        .iter()
        .filter(|l| {
            l.bar == bar && Analysis::home_staff(l.lane) == staff && l.start <= t && t < l.end()
        })
        .filter_map(|l| l.event())
        .flat_map(|ev| ev.notes().iter().map(|n| n.pitch.midi()))
        .collect()
}

fn hand_warnings(a: &Analysis, diags: &mut Vec<Diag>) {
    for (b, bar) in a.score.bars.iter().enumerate() {
        let mut onsets: Vec<Frac> = a
            .leaves
            .iter()
            .filter(|l| l.bar == b && l.event().is_some())
            .map(|l| l.start)
            .collect();
        onsets.sort();
        onsets.dedup();
        let mut crossed = false;
        let mut outside = false;
        let mut wide = [false, false];
        for t in onsets {
            let rh = sounding(a, b, Staff::Upper, t);
            let lh = sounding(a, b, Staff::Lower, t);
            let cell = bar.layout.cell_at(t) + 1;
            if let (Some(low), Some(high)) = (rh.iter().min(), lh.iter().max()) {
                if low < high && !crossed {
                    crossed = true;
                    diags.push(Diag::warning(
                        bar.src,
                        format!("{} cell {cell}", bar.label),
                        "the hands cross: rh plays below lh",
                    ));
                }
            }
            if !outside && rh.iter().chain(&lh).any(|m| !PIANO.contains(m)) {
                outside = true;
                diags.push(Diag::warning(
                    bar.src,
                    format!("{} cell {cell}", bar.label),
                    "a note is outside the piano (A0 to C8)",
                ));
            }
            for (k, (notes, lane)) in [(&rh, "rh"), (&lh, "lh")].into_iter().enumerate() {
                let (Some(lo), Some(hi)) = (notes.iter().min(), notes.iter().max()) else {
                    continue;
                };
                if hi - lo > WIDE_SPAN && !wide[k] {
                    wide[k] = true;
                    diags.push(Diag::warning(
                        bar.src,
                        format!("{} {lane} cell {cell}", bar.label),
                        format!("a span of {} semitones in one hand", hi - lo),
                    ));
                }
            }
        }
    }
}
