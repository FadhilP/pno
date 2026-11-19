mod common;

use common::{path, pno, root, temp_dir, write};

fn score_copy(test: &str) -> std::path::PathBuf {
    let text =
        std::fs::read_to_string(root().join("tests/fixtures/tristesse.pno")).expect("test score");
    write(&temp_dir(test), "score.pno", &text)
}

fn run_ok(args: &[&str]) -> String {
    let run = pno(args);
    assert_eq!(run.code, 0, "{args:?}: {}{}", run.stdout, run.stderr);
    run.stdout
}

fn read(p: &std::path::Path) -> String {
    std::fs::read_to_string(p).expect("file is readable")
}

#[test]
fn sound_shows_held_notes_across_the_barline_and_into_another_voice() {
    let f = score_copy("sound");
    let out = run_ok(&["sound", path(&f), "2"]);
    assert!(
        out.contains("\n2 1      lh B1 | rh A3 D#4~ F#4~\n"),
        "{out}"
    );
    assert!(
        out.contains("\n2 1+1/2  lh B1~ B2~ | rh A3 D#4 G#4\n"),
        "{out}"
    );
}

#[test]
fn sound_adds_the_notes_that_the_pedal_holds() {
    let dir = temp_dir("sound-pedal");
    let text = "@ format=1

1 ped* | P | - | - | - |
1 rh   | E5 | F5 | G5 | A5 |
1 lh   | C3 | G3 | E4 | G3 |

2 rh   | ~A5 | - | - | - |
2 lh   | F3 | C4 | A4 | C4 |
";
    let f = write(&dir, "p.pno", text);
    let out = run_ok(&["sound", path(&f), "--pedal"]);
    assert!(
        out.contains("\n1 3      lh C3* G3* E4 | rh E5* F5* G5\n"),
        "{out}"
    );
    // The change at the barline ends the pedal notes; A5 is still held by its key.
    assert!(
        out.contains("\n2 1      lh F3 | rh A5~  [pedal change]\n"),
        "{out}"
    );
    let plain = run_ok(&["sound", path(&f), "1"]);
    assert!(plain.contains("\n1 3      lh E4 | rh G5\n"), "{plain}");
}

#[test]
fn sound_velocity_shows_the_midi_velocity_of_each_new_note() {
    let dir = temp_dir("sound-velocity");
    let text = "@ format=1

1 rh   | C5+E5^v-20 | - | G5 | - |
1 lh   | C3:v+10 | - | - | - |
";
    let f = write(&dir, "v.pno", text);
    let out = run_ok(&["sound", path(&f), "--velocity"]);
    assert!(
        out.contains("\n1 1      lh C3@78 | rh C5@68 E5@48\n"),
        "{out}"
    );
    assert!(out.contains("\n1 3      lh C3~ | rh G5@68\n"), "{out}");
    let mid = dir.join("v.mid");
    run_ok(&["export", path(&f), path(&mid)]);
    let bytes = std::fs::read(&mid).expect("MIDI is written");
    for (key, velocity) in [(48u8, 78u8), (72, 68), (76, 48), (79, 68)] {
        assert!(
            bytes.windows(3).any(|w| w == [0x90, key, velocity]),
            "the MIDI has no note-on {key} at {velocity}"
        );
    }
}

#[test]
fn analyze_names_chords_and_repeated_bars() {
    let f = score_copy("analyze");
    let out = run_ok(&["analyze", path(&f), "2,10"]);
    // D#4 and F#4 are tied from bar 1 under the G#4.
    assert!(out.contains("\n2    B7 add G# | E\n"), "{out}");
    assert!(out.contains("\n10 rh = 2 rh\n"), "{out}");
}

#[test]
fn fmt_reports_check_errors() {
    let dir = temp_dir("fmt-check");
    let text = "@ format=1

1 rh | C5 | - | - | D5 |
1 lh | R |

2 rh | ~C5 | - | - | - |
2 lh | R |
";
    let f = write(&dir, "t.pno", text);
    let run = pno(&["fmt", path(&f)]);
    assert_eq!(run.code, 1, "{}", run.stdout);
    assert!(run.stdout.contains("error"), "{}", run.stdout);
}

#[test]
fn copy_then_delete_gives_the_same_file() {
    let f = score_copy("copy");
    let before = read(&f);
    run_ok(&["bars", "copy", path(&f), "14", "15"]);
    let copied = read(&f);
    assert!(
        copied.contains("\n17 rh.txt2 | _ \"ritenuto\":below | _ |\n"),
        "one more bar"
    );
    run_ok(&["bars", "delete", path(&f), "15"]);
    assert_eq!(read(&f), before);
}

#[test]
fn insert_writes_rest_bars_in_the_meter_before_them() {
    let dir = temp_dir("insert");
    let f = write(
        &dir,
        "i.pno",
        "@ format=1\n@ time=3/4\n\n1 rh | C5 | D5 | E5 |\n1 lh | R |\n",
    );
    run_ok(&["bars", "insert", path(&f), "end", "--count", "2"]);
    assert_eq!(
        read(&f),
        "@ format=1\n@ time=3/4\n\n1 rh | C5 | D5 | E5 |\n1 lh | R |\n\n2 rh | R |\n2 lh | R |\n\n3 rh | R |\n3 lh | R |\n"
    );
}

#[test]
fn an_edit_that_fails_check_leaves_the_file_unchanged() {
    let dir = temp_dir("edit-fails");
    let text = "@ format=1\n\n1 rh | C5 | - | - | - |\n1 lh | R |\n\n2 rh | ~C5 | - | - | - |\n2 lh | R |\n";
    let f = write(&dir, "t.pno", text);
    let run = pno(&["bars", "delete", path(&f), "1"]);
    assert_eq!(run.code, 1, "{}", run.stdout);
    assert!(run.stderr.contains("is not changed"), "{}", run.stderr);
    assert_eq!(read(&f), text);
}

#[test]
fn transpose_spells_in_the_new_key_and_returns() {
    let f = score_copy("transpose");
    let before = read(&f);
    run_ok(&["transpose", path(&f), "2"]);
    let up = read(&f);
    assert!(up.contains("@ key=6#\n"), "{up}");
    assert!(
        up.contains("\n1  lh      | F#2 C#3:ac - C#3 | C#2 C#3:ac - C#3 |\n"),
        "{up}"
    );
    assert!(up.contains("\n2  rh3     | ~E#4 E#4 | _ |\n"), "{up}");
    run_ok(&["transpose", path(&f), "-2"]);
    assert_eq!(read(&f), before);
}

#[test]
fn a_transposition_past_the_octave_range_names_the_pitch() {
    let dir = temp_dir("transpose-range");
    let text = "@ format=1\n\n1 rh | C8 | - | - | - |\n1 lh | R |\n";
    let f = write(&dir, "t.pno", text);
    let run = pno(&["transpose", path(&f), "24"]);
    assert_eq!(run.code, 1, "{}", run.stdout);
    assert!(
        run.stderr
            .contains("C8 is out of range after the transposition"),
        "{}",
        run.stderr
    );
    assert_eq!(read(&f), text);
}

#[test]
fn set_replaces_a_lane_and_adds_a_lane() {
    let dir = temp_dir("set");
    let f = write(
        &dir,
        "s.pno",
        "@ format=1

1 rh | C5 | D5 | E5 | F5 |
1 lh | R |

2 rh | G5 | - | - | - |
2 lh | R |
",
    );
    let out = run_ok(&[
        "set",
        path(&f),
        "2 lh | C3 | - | - | - |",
        "1 rh2 | A4 | - | - | - |",
    ]);
    assert_eq!(
        read(&f),
        "@ format=1

1 rh  | C5 | D5 | E5 | F5 |
1 rh2 | A4 | - | - | - |
1 lh  | R |

2 rh  | G5 | - | - | - |
2 lh  | C3 | - | - | - |
"
    );
    assert!(!out.contains("1 rh |"), "{out}");
    assert!(!out.contains("1 rh  |"), "{out}");
    assert!(
        out.contains(
            "-2 lh | R |
"
        ),
        "{out}"
    );
    assert!(
        out.contains(
            "+2 lh  | C3 | - | - | - |
"
        ),
        "{out}"
    );
}

#[test]
fn set_with_an_error_or_an_unknown_bar_leaves_the_file_unchanged() {
    let dir = temp_dir("set-fails");
    let text = "@ format=1

1 rh | C5 | D5 | E5 | F5 |
1 lh | R |
";
    let f = write(&dir, "s.pno", text);
    let run = pno(&["set", path(&f), "1 lh | C3 | - | - |"]);
    assert_eq!(run.code, 1, "{}", run.stdout);
    assert!(run.stderr.contains("is not changed"), "{}", run.stderr);
    let run = pno(&["set", path(&f), "7 lh | C3 | - | - | - |"]);
    assert_eq!(run.code, 2, "{}", run.stdout);
    assert!(run.stderr.contains("there is no bar 7"), "{}", run.stderr);
    assert_eq!(read(&f), text);
}
