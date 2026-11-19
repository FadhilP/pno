use std::collections::{HashMap, HashSet};

use crate::analysis::{Analysis, NoteRef, Part};
use crate::frac::Frac;
use crate::model::{Key, Note, Ottava, Staff};

const SHARP_ORDER: [u8; 7] = [3, 0, 4, 1, 5, 2, 6];
const FLAT_ORDER: [u8; 7] = [6, 2, 5, 1, 4, 0, 3];

pub fn key_alter(key: Key, step: u8) -> i8 {
    let n = usize::from(key.fifths.unsigned_abs());
    if key.fifths > 0 && SHARP_ORDER[..n].contains(&step) {
        return 1;
    }
    if key.fifths < 0 && FLAT_ORDER[..n].contains(&step) {
        return -1;
    }
    0
}

pub fn default_signs(a: &Analysis) -> HashSet<NoteRef> {
    let mut out = HashSet::new();
    for bar in 0..a.score.bars.len() {
        for staff in [Staff::Upper, Staff::Lower] {
            staff_bar(a, bar, staff, &mut out);
        }
    }
    out
}

fn staff_bar(a: &Analysis, bar: usize, staff: Staff, out: &mut HashSet<NoteRef>) {
    let mut items: Vec<((Frac, i32), NoteRef, &Note, Frac)> = Vec::new();
    for (id, leaf) in a.leaves.iter().enumerate() {
        if leaf.bar != bar || Analysis::home_staff(leaf.lane) != staff {
            continue;
        }
        let Some(ev) = leaf.event() else { continue };
        if let Some(g) = &ev.grace {
            for (gi, gn) in g.notes.iter().enumerate() {
                for (ni, n) in gn.notes.iter().enumerate() {
                    let r = NoteRef {
                        leaf: id,
                        part: Part::Grace(gi),
                        note: ni,
                    };
                    let rank = i32::try_from(gi).expect("an event has few grace groups");
                    items.push(((leaf.start, -1000 + rank), r, n, leaf.start));
                }
            }
        }
        for (ni, n) in ev.notes().iter().enumerate() {
            let r = NoteRef {
                leaf: id,
                part: Part::Main,
                note: ni,
            };
            items.push(((leaf.start, 0), r, n, leaf.start));
        }
        if let Some(g) = &ev.after {
            for (gi, gn) in g.notes.iter().enumerate() {
                for (ni, n) in gn.notes.iter().enumerate() {
                    let r = NoteRef {
                        leaf: id,
                        part: Part::After(gi),
                        note: ni,
                    };
                    let rank = i32::try_from(gi).expect("an event has few grace groups");
                    items.push(((leaf.end(), -2000 + rank), r, n, leaf.start));
                }
            }
        }
    }
    items.sort_by_key(|(k, r, _, _)| (*k, *r));
    let key = a.bars[bar].key;
    let mut state: HashMap<i32, i8> = HashMap::new();
    let position = |n: &Note, t: Frac| {
        let shift = a.ottava_at(staff, bar, t).map_or(0, Ottava::shift);
        i32::from(n.pitch.step) + 7 * i32::from(n.pitch.octave - shift)
    };
    let mut i = 0;
    while i < items.len() {
        let k = items[i].0;
        let group: Vec<_> = items[i..].iter().take_while(|it| it.0 == k).collect();
        i += group.len();
        let mut updates = Vec::new();
        for (_, r, n, t) in &group {
            // A tied or hidden note has no sign and does not change the alteration in effect.
            if n.tied || n.hidden {
                continue;
            }
            let pos = position(n, *t);
            let in_effect = state
                .get(&pos)
                .copied()
                .unwrap_or_else(|| key_alter(key, n.pitch.step));
            if n.pitch.alter != in_effect {
                out.insert(*r);
            }
            updates.push((pos, n.pitch.alter));
        }
        state.extend(updates);
    }
}
