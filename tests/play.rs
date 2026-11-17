mod common;

use std::path::Path;
use std::process::Command;

use common::{path, pno, root, temp_dir, write};

#[derive(Debug, Clone, Copy, PartialEq)]
struct Note {
    on: u32,
    off: u32,
    key: u8,
    vel: u8,
}

#[derive(Default)]
struct Midi {
    notes: Vec<Note>,
    pedal: Vec<(u32, u8)>,
    tempo: Vec<(u32, u32)>,
    end: u32,
}

fn varlen(b: &[u8], i: &mut usize) -> u32 {
    let mut v = 0;
    loop {
        let x = b[*i];
        *i += 1;
        v = (v << 7) | u32::from(x & 0x7F);
        if x & 0x80 == 0 {
            return v;
        }
    }
}

fn read_midi(file: &Path) -> Midi {
    let b = std::fs::read(file).expect("MIDI file");
    assert_eq!(&b[..4], b"MThd");
    let mut m = Midi::default();
    let mut i = 14;
    while i < b.len() {
        assert_eq!(&b[i..i + 4], b"MTrk");
        let len = u32::from_be_bytes(b[i + 4..i + 8].try_into().expect("4 bytes")) as usize;
        let end = i + 8 + len;
        i += 8;
        let mut t = 0;
        let mut open: Vec<(u8, u32, u8)> = Vec::new();
        while i < end {
            t += varlen(&b, &mut i);
            let status = b[i];
            i += 1;
            match status {
                0xFF => {
                    let kind = b[i];
                    i += 1;
                    let n = varlen(&b, &mut i) as usize;
                    if kind == 0x2F {
                        m.end = m.end.max(t);
                    }
                    if kind == 0x51 {
                        let us = u32::from_be_bytes([0, b[i], b[i + 1], b[i + 2]]);
                        m.tempo.push((t, (60_000_000 + us / 2) / us));
                    }
                    i += n;
                }
                0xC0 => i += 1,
                0xB0 => {
                    if b[i] == 64 {
                        m.pedal.push((t, b[i + 1]));
                    }
                    i += 2;
                }
                0x90 => {
                    open.push((b[i], t, b[i + 1]));
                    i += 2;
                }
                0x80 => {
                    let k = open
                        .iter()
                        .position(|o| o.0 == b[i])
                        .expect("a note off has a note on");
                    let (key, on, vel) = open.remove(k);
                    m.notes.push(Note {
                        on,
                        off: t,
                        key,
                        vel,
                    });
                    i += 2;
                }
                s => panic!("unexpected MIDI status {s:#x}"),
            }
        }
        assert!(open.is_empty(), "each note ends");
    }
    m.notes.sort_by_key(|n| (n.on, n.key));
    m
}

fn export(dir: &Path, score: &Path, name: &str, extra: &[&str]) -> Midi {
    let out = dir.join(name);
    let mut args = vec!["export", path(score), path(&out)];
    args.extend_from_slice(extra);
    let run = pno(&args);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    read_midi(&out)
}

const REPEAT: &str = "@ format=1

1 @     repeat=start
1 tempo | =60 | _ | _ | _ |
1 rh    | C5 | D5^a+120^r-60 | E5:st | F5:v+10 |
1 lh    | C3 | - | - | - |
1 ped   | P | - | - | - |

2 @     repeat=end
2 rh    | G5 | - | - | A5 |
2 lh    | ~C3 | - | - | - |

3 rh    | C6 | - | - | - |
3 lh    | R |
";

#[test]
fn midi_plays_repeats_ties_offsets_articulations_and_the_pedal() {
    let dir = temp_dir("play-repeat");
    let m = export(&dir, &write(&dir, "r.pno", REPEAT), "r.mid", &[]);
    let note = |key: u8, on: u32| {
        *m.notes
            .iter()
            .find(|n| n.key == key && n.on == on)
            .unwrap_or_else(|| panic!("no key {key} at {on}: {:?}", m.notes))
    };
    // A note sounds 5 % shorter than its value, as in MuseScore.
    assert_eq!(note(72, 0).off, 456);
    assert_eq!(note(74, 600).off, 876, "^a+120^r-60");
    assert_eq!(note(76, 960).off, 1200, "staccato: half the value");
    assert_eq!(note(77, 1440).vel, 78, "68 and :v+10");
    // The tie joins C3 over two bars; the repeat plays it twice.
    assert_eq!(note(48, 0).off, 3816);
    assert_eq!(note(48, 3840).off, 7656);
    assert_eq!(m.notes.iter().filter(|n| n.key == 72).count(), 2);
    assert_eq!(note(84, 7680).off, 9576, "bar 3 after the repeat");
    assert_eq!(m.pedal, vec![(0, 127), (1920, 0), (3840, 127), (5760, 0)]);
    assert_eq!(m.tempo, vec![(0, 60)]);
    // sfizz stops at the end of the file: 4 s after the last note for its release sound.
    assert_eq!(m.end, 9576 + 4 * 480, "4 quarters at 60 per minute");
}

#[test]
fn midi_plays_ornaments_grace_notes_arpeggios_and_fermatas() {
    let dir = temp_dir("play-ornaments");
    let text = "@ format=1
@ key=G

1 rh | E5:tr | {/D5}C5 | C4+E4+G4:arp | B4:ferm |
1 lh | R |
";
    let m = export(&dir, &write(&dir, "o.pno", text), "o.mid", &[]);
    let trill: Vec<u8> = m
        .notes
        .iter()
        .filter(|n| n.on < 480)
        .map(|n| n.key)
        .collect();
    assert_eq!(
        trill,
        vec![76, 78, 76, 78, 76, 78, 76],
        "E5 and F#5 in G major"
    );
    let beat2: Vec<(u32, u8)> = m
        .notes
        .iter()
        .filter(|n| (480..960).contains(&n.on))
        .map(|n| (n.on, n.key))
        .collect();
    assert_eq!(
        beat2,
        vec![(480, 74), (547, 72)],
        "the grace note takes 70 ms"
    );
    let arp: Vec<(u32, u8)> = m
        .notes
        .iter()
        .filter(|n| (960..1440).contains(&n.on))
        .map(|n| (n.on, n.key))
        .collect();
    assert_eq!(arp, vec![(960, 60), (994, 64), (1027, 67)]);
    assert_eq!(m.tempo, vec![(0, 120), (1440, 60), (1920, 120)]);
}

#[test]
fn humanize_is_bounded_and_the_seed_repeats_it() {
    let dir = temp_dir("play-humanize");
    let score = root().join("tests/fixtures/tristesse.pno");
    let plain = export(&dir, &score, "plain.mid", &[]);
    let one = export(&dir, &score, "one.mid", &["--humanize", "--seed", "7"]);
    let again = export(&dir, &score, "again.mid", &["--humanize", "--seed", "7"]);
    let other = export(&dir, &score, "other.mid", &["--humanize", "--seed", "8"]);
    assert_eq!(one.notes, again.notes);
    assert_ne!(one.notes, other.notes);
    assert_eq!(one.notes.len(), plain.notes.len());
    let by_key = |m: &Midi| {
        let mut v: Vec<Note> = m.notes.clone();
        v.sort_by_key(|n| (n.key, n.on));
        v
    };
    let moved: Vec<i64> = by_key(&plain)
        .iter()
        .zip(by_key(&one).iter())
        .map(|(p, h)| {
            assert_eq!(p.key, h.key);
            assert!(p.vel.abs_diff(h.vel) <= 16, "{p:?} {h:?}");
            i64::from(h.on) - i64::from(p.on)
        })
        .collect();
    // At 120 quarters per minute, 1 ms is about 1 tick; the limit is 36 ms.
    assert!(moved.iter().all(|d| d.abs() <= 40), "{moved:?}");
    assert!(moved.iter().any(|d| *d != 0));
}

#[test]
fn export_options_name_the_formats_they_apply_to() {
    let dir = temp_dir("play-options");
    let score = root().join("tests/fixtures/tristesse.pno");
    let pdf = dir.join("x.pdf");
    let run = pno(&["export", path(&score), path(&pdf), "--humanize"]);
    assert_eq!(run.code, 2);
    assert!(
        run.stderr.contains("only to MIDI and audio"),
        "{}",
        run.stderr
    );
    let wav = dir.join("x.wav");
    let sf2 = write(&dir, "piano.sf2", "");
    let run = pno(&["export", path(&score), path(&wav), "--sound", path(&sf2)]);
    assert_eq!(run.code, 1);
    assert!(
        run.stderr.contains("not an SFZ instrument"),
        "{}",
        run.stderr
    );
}

fn sfizz() -> Option<String> {
    let exe = std::env::var("PNO_SFIZZ").unwrap_or_else(|_| "sfizz_render".into());
    Command::new(&exe).arg("--help").output().ok().map(|_| exe)
}

fn sine_wav(file: &Path) {
    let rate: u32 = 48_000;
    let samples: Vec<i16> = (0..rate)
        .map(|i| {
            let x = f64::from(i) * 440.0 * std::f64::consts::TAU / f64::from(rate);
            (x.sin() * 8000.0) as i16
        })
        .collect();
    let data = (samples.len() * 2) as u32;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(file, b).expect("sample is written");
}

#[test]
fn sfizz_plays_the_score_with_an_sfz_instrument() {
    if sfizz().is_none() {
        eprintln!("sfizz_render not found; skipped");
        return;
    }
    let dir = temp_dir("play-sfizz");
    sine_wav(&dir.join("a4.wav"));
    let sfz = write(
        &dir,
        "sine.sfz",
        "<region> sample=a4.wav pitch_keycenter=69 lokey=0 hikey=127 ampeg_release=0.1\n",
    );
    let score = write(&dir, "s.pno", REPEAT);
    let wav = dir.join("s.wav");
    let run = pno(&[
        "export",
        path(&score),
        path(&wav),
        "--sound",
        path(&sfz),
        "--humanize",
    ]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    let bytes = std::fs::read(&wav).expect("WAV file");
    assert_eq!(&bytes[..4], b"RIFF");
    // 5 bars of 4 beats at 60 quarters per minute: 20 s of 16-bit stereo at 48 kHz.
    assert!(bytes.len() > 19 * 48_000 * 4, "{} bytes", bytes.len());
    let data = bytes
        .windows(4)
        .position(|w| w == b"data")
        .expect("data chunk")
        + 8;
    let peak = bytes[data..]
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]).unsigned_abs())
        .max();
    assert_eq!(peak, Some(29_204), "the loudest sample is at -1 dBFS");
    if Command::new("ffmpeg").arg("-version").output().is_ok() {
        let mp3 = dir.join("s.mp3");
        let run = pno(&["export", path(&score), path(&mp3), "--sound", path(&sfz)]);
        assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
        assert!(std::fs::metadata(&mp3).expect("MP3 file").len() > 10_000);
    }
}

#[test]
fn humanize_plays_tempo_words_and_a_final_ritardando() {
    let dir = temp_dir("play-rit");
    let text = "@ format=1

1 tempo | =100 | _ | _ | _ |
1 rh    | C5 | D5 | E5 | F5 |
1 lh    | R |

2 tempo | \"rit.\" | - | - | - |
2 rh    | G5 | F5 | E5 | D5 |
2 lh    | R |

3 tempo | \"a tempo\" | _ | _ | _ |
3 rh    | C5 | D5 | E5 | F5 |
3 lh    | R |

4 rh    | G5 | F5 | E5 | D5 |
4 lh    | R |

5 rh    | C5 | D5 | E5 | F5 |
5 lh    | R |

6 rh    | C5 | - | - | - |
6 lh    | R |
";
    let score = write(&dir, "rit.pno", text);
    let plain = export(&dir, &score, "plain.mid", &[]);
    assert_eq!(
        plain.tempo,
        vec![(0, 100)],
        "the texts change nothing without --humanize"
    );
    let m = export(&dir, &score, "rit.mid", &["--humanize", "--seed", "1"]);
    let at = |t: u32| {
        m.tempo
            .iter()
            .take_while(|p| p.0 <= t)
            .last()
            .expect("a tempo")
            .1
    };
    let between = |from: u32, to: u32| -> Vec<u32> {
        m.tempo
            .iter()
            .filter(|p| from <= p.0 && p.0 < to)
            .map(|p| p.1)
            .collect()
    };
    let rit = between(1920, 3840);
    assert!(rit.windows(2).all(|w| w[0] >= w[1]), "{rit:?}");
    assert!((75..=80).contains(rit.last().expect("steps")), "{rit:?}");
    assert_eq!(at(3840), 100, "a tempo");
    let last = between(5760, 9601);
    assert!(last.windows(2).all(|w| w[0] >= w[1]), "{last:?}");
    assert_eq!(at(9600), 70, "the last onset is at 70 % of the tempo");
}

#[test]
fn phrase_shapes_slurs_and_bar_groups_that_end_at_a_rest() {
    let dir = temp_dir("play-phrase");
    let text = "@ format=1

1 tempo | =60 | _ | _ | _ |
1 rh    | C5( | D5 | E5 | F5 |
1 lh    | R |

2 rh    | G5 | A5 | B5 | C6) |
2 lh    | R |

3 rh    | C5 | D5 | E5 | F5 |
3 lh    | R |

4 rh    | G5 | F5 | E5 | r |
4 lh    | R |

5 rh    | C5 | D5 | E5 | F5 |
5 lh    | R |

6 rh    | G5 | F5 | E5 | D5 |
6 lh    | R |
";
    let score = write(&dir, "p.pno", text);
    let plain = export(&dir, &score, "plain.mid", &[]);
    assert!(plain.notes.iter().all(|n| n.vel == 68));
    let m = export(&dir, &score, "p.mid", &["--phrase"]);
    assert_eq!(
        export(&dir, &score, "again.mid", &["--phrase"]).notes,
        m.notes
    );
    let vel = |on: u32| m.notes.iter().find(|n| n.on == on).expect("a note").vel;
    // Each phrase starts soft and slow: the slur, bars 3-4 up to the rest, bars 5-6.
    assert_eq!((vel(0), vel(3840), vel(7680)), (62, 62, 62));
    assert!(
        vel(2400) >= 72,
        "the peak of the slur phrase: {}",
        vel(2400)
    );
    let tempo = |t: u32| {
        m.tempo
            .iter()
            .take_while(|p| p.0 <= t)
            .last()
            .expect("a tempo")
            .1
    };
    assert_eq!((tempo(0), tempo(3840), tempo(7680)), (59, 59, 59));
    assert!(
        m.tempo.iter().all(|p| (59..=61).contains(&p.1)),
        "{:?}",
        m.tempo
    );
    assert!(m.tempo.iter().any(|p| p.1 == 61));
}
