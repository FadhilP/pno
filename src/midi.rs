use crate::model::Staff;
use crate::perform::{Performance, TICKS_PER_QUARTER};

// At one tick: note ends, then controllers, then note starts.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Meta,
    NoteOff,
    Control,
    NoteOn,
}

struct Msg {
    tick: u32,
    rank: Rank,
    bytes: Vec<u8>,
}

fn tick(t: f64) -> u32 {
    t.round().max(0.0) as u32
}

fn msg(t: f64, rank: Rank, bytes: Vec<u8>) -> Msg {
    Msg {
        tick: tick(t),
        rank,
        bytes,
    }
}

// The 3 bytes of a MIDI tempo hold at least 3.6 quarters per minute.
const SLOWEST_QPM: f64 = 4.0;
const FASTEST_QPM: f64 = 2000.0;

// A format 1 standard MIDI file: a tempo track, then one track for each staff.
// Both staves use channel 1, so the pedal holds the notes of both.
pub fn write(p: &Performance) -> Result<Vec<u8>, String> {
    let mut conductor: Vec<Msg> = Vec::new();
    for (t, qpm) in &p.tempo {
        let us = (60_000_000.0 / qpm.clamp(SLOWEST_QPM, FASTEST_QPM)).round() as u32;
        let b = us.to_be_bytes();
        conductor.push(msg(*t, Rank::Meta, vec![0xFF, 0x51, 3, b[1], b[2], b[3]]));
    }
    for (t, num, den) in &p.meters {
        let beats = u8::try_from(*num).map_err(|_| {
            format!("MIDI cannot hold a time signature of {num} beats; the most is 255")
        })?;
        let log =
            u8::try_from(den.trailing_zeros()).expect("trailing_zeros of a u32 is at most 32");
        conductor.push(msg(*t, Rank::Meta, vec![0xFF, 0x58, 4, beats, log, 24, 8]));
    }
    let name = |n: &str| {
        let len = u8::try_from(n.len()).expect("track names are rh and lh");
        let mut b = vec![0xFF, 0x03, len];
        b.extend_from_slice(n.as_bytes());
        msg(0.0, Rank::Meta, b)
    };
    let mut upper = vec![name("rh"), msg(0.0, Rank::Control, vec![0xC0, p.program])];
    if let Some(v) = p.volume {
        upper.push(msg(0.0, Rank::Control, vec![0xB0, 7, v]));
    }
    if let Some(v) = p.pan {
        upper.push(msg(0.0, Rank::Control, vec![0xB0, 10, v]));
    }
    let mut lower: Vec<Msg> = vec![name("lh")];
    lower.extend(
        p.pedal
            .iter()
            .map(|(t, v)| msg(*t, Rank::Control, vec![0xB0, 64, *v])),
    );
    for n in &p.notes {
        let track = match n.staff {
            Staff::Upper => &mut upper,
            Staff::Lower => &mut lower,
        };
        let on = msg(n.on, Rank::NoteOn, vec![0x90, n.key, n.velocity]);
        let off = Msg {
            tick: tick(n.off).max(on.tick + 1),
            rank: Rank::NoteOff,
            bytes: vec![0x80, n.key, 64],
        };
        track.extend([on, off]);
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"MThd");
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&3u16.to_be_bytes());
    out.extend_from_slice(&(TICKS_PER_QUARTER as u16).to_be_bytes());
    for track in [conductor, upper, lower] {
        write_track(&mut out, track, tick(p.end));
    }
    Ok(out)
}

fn write_track(out: &mut Vec<u8>, mut msgs: Vec<Msg>, end: u32) {
    msgs.sort_by_key(|m| (m.tick, m.rank));
    let mut body = Vec::new();
    let mut last = 0;
    for m in &msgs {
        push_varlen(&mut body, m.tick - last);
        body.extend_from_slice(&m.bytes);
        last = m.tick;
    }
    push_varlen(&mut body, end.saturating_sub(last));
    body.extend_from_slice(&[0xFF, 0x2F, 0]);
    out.extend_from_slice(b"MTrk");
    let len = u32::try_from(body.len()).expect("a MIDI track is shorter than 4 GiB");
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&body);
}

fn push_varlen(out: &mut Vec<u8>, mut v: u32) {
    let mut groups = vec![(v & 0x7F) as u8];
    v >>= 7;
    while v > 0 {
        groups.push((v & 0x7F) as u8 | 0x80);
        v >>= 7;
    }
    out.extend(groups.iter().rev());
}
