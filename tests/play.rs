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
    assert_eq!(m.end, 9576, "only audio gets 4 s for the release sound");
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
        .filter(|n| n.on < 480 && n.key != 74)
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
        .filter(|n| (400..880).contains(&n.on) && n.key < 76)
        .map(|n| (n.on, n.key))
        .collect();
    assert_eq!(
        beat2,
        vec![(413, 74), (480, 72)],
        "a 70 ms grace note before the beat"
    );
    let arp: Vec<(u32, u8)> = m
        .notes
        .iter()
        .filter(|n| (880..1440).contains(&n.on))
        .map(|n| (n.on, n.key))
        .collect();
    assert_eq!(
        arp,
        vec![(893, 60), (926, 64), (960, 67)],
        "the roll ends on the beat"
    );
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
    let size =
        |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes")) as usize;
    assert_eq!(size(4), bytes.len() - 8, "RIFF size after the cut");
    assert_eq!(
        size(data - 4),
        bytes.len() - data,
        "data size after the cut"
    );
    // The cut keeps 0.05 s after the last sample above -72 dBFS.
    let end = &bytes[bytes.len() - 48_000 * 4 * 6 / 100..];
    assert!(
        end.chunks_exact(2)
            .any(|c| i16::from_le_bytes([c[0], c[1]]).unsigned_abs() > 8),
        "the silence of the 4 s tail is cut"
    );
    if Command::new("ffmpeg").arg("-version").output().is_ok() {
        let mp3 = dir.join("s.mp3");
        let run = pno(&["export", path(&score), path(&mp3), "--sound", path(&sfz)]);
        assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
        assert!(std::fs::metadata(&mp3).expect("MP3 file").len() > 10_000);
    }
}

#[test]
fn tempo_words_play_and_humanize_adds_a_final_ritardando() {
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
    assert!(plain.tempo.len() > 3, "rit. plays: {:?}", plain.tempo);
    assert_eq!(
        plain.tempo.last(),
        Some(&(3840, 100)),
        "a tempo, and no final ritardando without --humanize"
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

const PHRASES: &str = "@ format=1

1  tempo | =60 | _ | _ | _ |
1  rh    | C5( | D5 | E5 | F5 |
1  lh    | R |

2  rh    | G5 | A5 | B5 | C6) |
2  lh    | R |

3  rh    | C5 | D5 | E5 | F5 |
3  lh    | R |

4  rh    | G5 | F5 | E5 | D5 |
4  lh    | R |

5  rh    | E5 | F5 | G5 | A5 |
5  lh    | R |

6  rh    | B5 | A5 | G5 | r |
6  lh    | R |

7  rh    | D5 | E5 | F5 | G5 |
7  lh    | R |

8  rh    | A5 | G5 | F5 | E5 |
8  lh    | R |

9  rh    | F5 | G5 | A5 | B5 |
9  lh    | R |

10 rh    | C6 | B5 | A5 | G5 |
10 lh    | R |
";

#[test]
fn phrase_shapes_slurs_and_bar_groups_that_end_at_a_rest() {
    let dir = temp_dir("play-phrase");
    let score = write(&dir, "p.pno", PHRASES);
    let plain = export(&dir, &score, "plain.mid", &[]);
    assert!(plain.notes.iter().all(|n| n.vel == 68));
    let m = export(&dir, &score, "p.mid", &["--phrase"]);
    assert_eq!(
        export(&dir, &score, "again.mid", &["--phrase"]).notes,
        m.notes
    );
    let vel = |on: u32| m.notes.iter().find(|n| n.on == on).expect("a note").vel;
    // Each phrase starts soft and slow: the slur, bars 3-6 up to the rest, bars 7-10.
    assert_eq!((vel(0), vel(3840), vel(11520)), (62, 62, 62));
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
    assert_eq!((tempo(0), tempo(3840), tempo(11520)), (59, 59, 59));
    assert!(
        // 3 % slower at the end of a phrase is 58.2.
        m.tempo.iter().all(|p| (58..=61).contains(&p.1)),
        "{:?}",
        m.tempo
    );
    assert!(m.tempo.iter().any(|p| p.1 == 61));
}

#[test]
fn a_grace_note_on_the_first_beat_moves_the_performance_later() {
    let dir = temp_dir("play-lead");
    let text = "@ format=1

1 rh | {/D5}C5 | D5 | E5 | F5 |
1 lh | C3 | - | - | - |
1 ped | P | - | - | - |
";
    let m = export(&dir, &write(&dir, "l.pno", text), "l.mid", &[]);
    // 70 ms at 120 quarters per minute is 67 ticks.
    let first: Vec<(u32, u8)> = m.notes.iter().take(3).map(|n| (n.on, n.key)).collect();
    assert_eq!(first, vec![(0, 74), (67, 48), (67, 72)]);
    assert_eq!(m.pedal[0], (67, 127));
}

#[test]
fn phrase_marks_replace_the_phrases_that_pno_finds() {
    let dir = temp_dir("play-phrase-marks");
    let score = write(&dir, "p.pno", PHRASES);
    let found = export(&dir, &score, "found.mid", &["--phrase"]);
    let run = pno(&["phrase", path(&score)]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(run.stdout.contains("marked 3 phrases"), "{}", run.stdout);
    let marked = export(&dir, &score, "marked.mid", &["--phrase"]);
    assert_eq!(marked.notes, found.notes, "the marks give the same phrases");
    let text = std::fs::read_to_string(&score).expect("score");
    let text = text
        .replace("3  phrase | / |", "3  phrase | _ |")
        .replace("7  phrase | / |", "7  phrase | _ |");
    let text = text.replace(
        "
4  rh ",
        "
4  phrase | _ | _ | breath | _ |
4  rh ",
    );
    let score = write(&dir, "edited.pno", &text);
    let m = export(&dir, &score, "edited.mid", &["--phrase"]);
    let vel = |on: u32| m.notes.iter().find(|n| n.on == on).expect("a note").vel;
    assert_eq!(vel(0), 62);
    assert!(vel(3840) > 62, "no phrase starts at bar 3: {}", vel(3840));
    assert_eq!(
        vel(6720),
        62,
        "the breath before beat 3 of bar 4 starts a phrase"
    );
    assert!(vel(11520) > 62, "no phrase starts at bar 7: {}", vel(11520));
}

fn keys_from(m: &Midi, from: u32, to: u32) -> Vec<(u32, u8)> {
    m.notes
        .iter()
        .filter(|n| from <= n.on && n.on < to)
        .map(|n| (n.on, n.key))
        .collect()
}

#[test]
fn tempo_words_are_whole_words_and_changes_end_by_rule() {
    let dir = temp_dir("play-words");
    let bars = |first: &str| {
        let mut t = format!(
            "@ format=1\n\n1 tempo | {first} | _ | _ | _ |\n1 rh | C5 | - | - | - |\n1 lh | R |\n"
        );
        for b in 2..=6 {
            t.push_str(&format!("\n{b} rh | C5 | - | - | - |\n{b} lh | R |\n"));
        }
        t
    };
    let spirito = write(&dir, "s.pno", &bars("\"Allegro con spirito\"=120"));
    let m = export(&dir, &spirito, "s.mid", &[]);
    assert_eq!(m.tempo, vec![(0, 120)], "spirito has no rit");

    let text = bars("=60")
        .replace("2 rh", "2 tempo | \"rit.\" | - | - | - |\n2 rh")
        .replace("3 rh", "3 tempo | \"rit.\" | - | - | - |\n3 rh")
        .replace("4 rh", "4 tempo | \"Tempo I\" | _ | _ | _ |\n4 rh")
        .replace("5 rh", "5 tempo | _ | _ | \"a tempo\" | _ |\n5 rh");
    let m = export(
        &dir,
        &write(&dir, "r.pno", &text),
        "r.mid",
        &["--humanize", "--seed", "1"],
    );
    let at = |t: u32| {
        m.tempo
            .iter()
            .take_while(|p| p.0 <= t)
            .last()
            .expect("tempo")
            .1
    };
    assert_eq!(at(1920 * 3 - 1), 45, "two rit. do not multiply: 75 % of 60");
    assert_eq!(at(1920 * 3), 60, "Tempo I");
    assert_eq!(at(1920 * 4 + 960), 60, "a tempo in the bar before the last");
    let rit: Vec<u32> = m
        .tempo
        .iter()
        .filter(|p| p.0 >= 1920 * 4 + 960)
        .map(|p| p.1)
        .collect();
    assert!(
        rit.len() > 2 && rit.windows(2).all(|w| w[0] >= w[1]),
        "the final ritardando starts at the a tempo: {rit:?}"
    );
}

#[test]
fn a_later_tie_does_not_join_notes_across_a_repeat() {
    let dir = temp_dir("play-tie-later");
    let text = "@ format=1

1 @  repeat=start
1 rh | R |
1 lh | C3^t | E3 | G3 | E3 |

2 @  repeat=end volta=1
2 rh | R |
2 lh | ~C3 | - | - | - |

3 @  volta=2
3 rh | R |
3 lh | E3 | - | - | - |

4 rh | R |
4 lh | C3 | - | ~C3 | - |
";
    let m = export(&dir, &write(&dir, "t.pno", text), "t.mid", &[]);
    let off = |on: u32| {
        m.notes
            .iter()
            .find(|n| n.key == 48 && n.on == on)
            .expect("C3")
            .off
    };
    assert_eq!(off(0), 3816, "pass 1: C3 is tied into the first ending");
    assert_eq!(off(3840), 4296, "pass 2: one quarter");
    assert_eq!(off(7680), 9576, "bar 4: the tie to the next note");
}

#[test]
fn a_fermata_keeps_the_speed_of_rolls_grace_notes_and_trills() {
    let dir = temp_dir("play-fermata");
    let text = "@ format=1

1 rh | r | C4+E4+G4+C5:arp:ferm | {/D5}C5:ferm | C5+E5:tr |
1 lh | R |
";
    let m = export(&dir, &write(&dir, "f.pno", text), "f.mid", &[]);
    let roll: Vec<u32> = keys_from(&m, 300, 481).iter().map(|n| n.0).collect();
    // 35 ms at 120 quarters per minute is 33.6 ticks.
    assert!(
        roll.windows(2).all(|w| (33..=34).contains(&(w[1] - w[0]))),
        "{roll:?}"
    );
    let grace = keys_from(&m, 800, 961);
    assert_eq!(grace.first().map(|g| g.1), Some(74));
    assert_eq!(grace.get(1).map(|g| g.0 - grace[0].0), Some(67), "70 ms");
    let trill: Vec<u8> = keys_from(&m, 1440, 1920).iter().map(|n| n.1).collect();
    assert_eq!(
        trill.iter().filter(|&&k| k == 72).count(),
        1,
        "C5 does not trill"
    );
    let upper: Vec<u8> = trill.into_iter().filter(|&k| k != 72).collect();
    assert_eq!(upper.first(), Some(&76));
    assert_eq!(upper.last(), Some(&76), "the trill ends on its main note");
}

#[test]
fn ornaments_and_repeats_play_as_written() {
    let dir = temp_dir("play-turn");
    let text = "@ format=1

1 @  repeat=start
1 rh | C5:turn# | - | - | - |
1 lh | R |

2 @  repeat=end volta=1
2 rh | D5 | - | - | - |
2 lh | R |

3 @  volta=2
3 rh | E5 | - | - | - |
3 lh | R |

4 @  repeat=end
4 rh | F5 | - | - | - |
4 lh | R |

5 rh | G5 | - | - | - |
5 lh | R |
";
    let m = export(&dir, &write(&dir, "o.pno", text), "o.mid", &[]);
    let turn: Vec<u8> = keys_from(&m, 0, 480).iter().map(|n| n.1).collect();
    assert_eq!(
        turn,
        vec![75, 72, 71, 72],
        "the sign is for the upper note only"
    );
    let mut starts: Vec<(u32, u8)> = m.notes.iter().map(|n| (n.on, n.key)).collect();
    starts.sort();
    let bars: Vec<u8> = starts
        .iter()
        .filter(|n| n.0 % 1920 == 0)
        .map(|n| n.1)
        .filter(|&k| k != 75)
        .collect();
    assert_eq!(
        bars,
        vec![74, 76, 77, 77, 79],
        "the repeat after the volta plays"
    );
}

#[test]
fn the_pedal_goes_down_again_after_a_jump_and_at_a_new_press() {
    let dir = temp_dir("play-pedal");
    let text = "@ format=1

1 rh  | C5 | - | - | - |
1 lh  | R |
1 ped | P | - | P | - |

2 @   repeat=start
2 rh  | D5 | - | - | - |
2 lh  | R |
2 ped | - | - | - | - |

3 @   repeat=end
3 rh  | E5 | - | - | - |
3 lh  | R |
3 ped | - | - | - | - |
";
    let m = export(&dir, &write(&dir, "p.pno", text), "p.mid", &[]);
    // 60 ms at 120 quarters per minute is 58 ticks.
    assert_eq!(
        m.pedal,
        vec![
            (0, 127),
            (960, 0),
            (1018, 127),
            (5760, 0),
            (5818, 127),
            (9600, 0)
        ]
    );
}

#[test]
fn breath_marks_and_caesuras_make_a_pause() {
    let dir = temp_dir("play-breath");
    let plain = "@ format=1

1 rh | C5 | D5 | E5 | F5 |
1 lh | R |

2 rh | G5 | - | - | - |
2 lh | R |
";
    let marked = plain.replace("2 rh", "2 phrase | caesura | _ | _ | _ |\n2 rh");
    let a = export(&dir, &write(&dir, "a.pno", plain), "a.mid", &[]);
    let b = export(&dir, &write(&dir, "b.pno", &marked), "b.mid", &[]);
    let f = |m: &Midi| m.notes.iter().find(|n| n.key == 77).expect("F5").off;
    assert_eq!(a.tempo, vec![(0, 120)]);
    // The last 16th before the caesura is held at a quarter of the tempo.
    assert_eq!(b.tempo, vec![(0, 120), (1800, 30), (1920, 120)]);
    assert!(f(&b) <= 1800, "F5 ends before the hold: {}", f(&b));
}

#[test]
fn bars_plays_a_range_with_the_tempo_in_effect() {
    let dir = temp_dir("play-bars");
    let score = write(&dir, "r.pno", REPEAT);
    let m = export(&dir, &score, "b.mid", &["--bars", "2-3"]);
    assert_eq!(m.tempo, vec![(0, 60)]);
    assert_eq!(
        keys_from(&m, 0, 1),
        vec![(0, 48), (0, 79)],
        "G5 starts bar 2; the tied C3 sounds on"
    );
    assert!(
        m.notes.iter().any(|n| n.key == 84 && n.on == 1920),
        "C6 in bar 3"
    );
    assert_eq!(m.pedal, vec![], "the pedal of bar 1 ends there");
}

#[test]
fn pno_phrase_writes_one_line_for_each_bar_and_sections() {
    let dir = temp_dir("play-phrase-cmd");
    let text = "@ format=1

1 rh | C5( | D5 | E5 | F5 |
1 lh | R |

2 rh | G5 | F5 | E5 | D5 |
2 lh | R |

3 rh | C5 | D5) | E5 | F5( |
3 lh | R |

4 rh | G5 | F5 | E5 | D5 |
4 lh | R |

5 @  barline=double
5 rh | C5 | D5 | E5 | F5) |
5 lh | R |

6 rh | C5 | D5 | E5 | F5 |
6 lh | R |

7 rh | G5 | F5 | E5 | D5 |
7 lh | R |
";
    let score = write(&dir, "p.pno", text);
    let run = pno(&["phrase", path(&score)]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert!(
        run.stdout
            .contains("3 cell 4: / (a slur of two bars or longer)"),
        "{}",
        run.stdout
    );
    let out = std::fs::read_to_string(&score).expect("score");
    assert!(out.contains("3 phrase | _ | _ | / | / |"), "{out}");
    assert!(
        out.contains("6 phrase | // |"),
        "a double barline ends a section:\n{out}"
    );
    let m = export(&dir, &score, "p.mid", &["--phrase=2"]);
    let vel = |on: u32| m.notes.iter().find(|n| n.on == on).expect("a note").vel;
    assert_eq!(
        vel(0),
        68 - 12 - 6,
        "phrase and half the section arc, twice the amount"
    );
    let zero = pno(&[
        "export",
        path(&score),
        path(&dir.join("z.mid")),
        "--phrase=0",
    ]);
    assert_eq!(zero.code, 2, "{}", zero.stderr);
}

#[test]
fn a_tempo_needs_a_number_above_zero() {
    let dir = temp_dir("play-q0");
    let text = "@ format=1

1 tempo | q=0 | _ | _ | _ |
1 rh    | C5 | - | - | - |
1 lh    | R |
";
    let run = pno(&["check", path(&write(&dir, "q.pno", text))]);
    assert_eq!(run.code, 1, "{}", run.stdout);
    assert!(
        run.stdout.contains("a tempo needs a number above 0"),
        "{}",
        run.stdout
    );
}

#[test]
fn the_voices_of_one_staff_roll_as_one_arpeggio() {
    let dir = temp_dir("play-arp-lanes");
    let text = "@ format=1

1 rh  | E5+C6:arp | - | - | - |
1 rh2 | C4+G4:arp | - | - | - |
1 lh  | R |
";
    let m = export(&dir, &write(&dir, "a.pno", text), "a.mid", &[]);
    let mut roll: Vec<(u32, u8)> = m.notes.iter().map(|n| (n.on, n.key)).collect();
    roll.sort();
    let keys: Vec<u8> = roll.iter().map(|n| n.1).collect();
    assert_eq!(keys, vec![60, 67, 76, 84], "one roll from the bottom");
    assert!(roll.windows(2).all(|w| w[1].0 > w[0].0), "{roll:?}");
}

#[test]
fn pno_phrase_skips_the_pickup_and_starts_a_phrase_at_its_upbeat() {
    let dir = temp_dir("play-phrase-upbeat");
    let mut text = String::from("@ format=1\n\n0 rh | x G4( |\n0 lh | x r |\n");
    let bars = [
        "C5 | D5) | E5 | F5",
        "G5 | F5 | E5 | D5",
        "E5 | F5 | G5 | A5",
        "B5 | A5 | G5 | r G4",
        "D5 | E5 | F5 | G5",
        "A5 | G5 | F5 | E5",
        "F5 | G5 | A5 | B5",
        "C6 | - | - | -",
    ];
    for (b, cells) in bars.iter().enumerate() {
        text.push_str(&format!("\n{} rh | {cells} |\n{} lh | R |\n", b + 1, b + 1));
    }
    let score = write(&dir, "u.pno", &text);
    assert_eq!(pno(&["fmt", path(&score)]).code, 0);
    let run = pno(&["phrase", path(&score)]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert!(
        run.stdout.contains(
            "0 cell 1: / (the start of the score)\n4 cell 4: / (a rest in rh)\nmarked 2 phrases"
        ),
        "the short slur from the pickup is no phrase:\n{}",
        run.stdout
    );
}
