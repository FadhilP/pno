mod common;

use common::{fixtures, path, pno, temp_dir, write};

const R: &str = "| R |";

fn check_text(name: &str, body: &str) -> common::Run {
    let dir = temp_dir("check");
    let f = write(
        &dir,
        &format!("{name}.pno"),
        &format!("@ format=1\n\n{body}\n"),
    );
    pno(&["check", path(&f)])
}

#[test]
fn fixtures_pass_check() {
    for f in fixtures() {
        let run = pno(&["check", path(&f)]);
        assert_eq!(
            run.code,
            0,
            "{}:\n{}{}",
            f.display(),
            run.stdout,
            run.stderr
        );
    }
}

#[test]
fn invalid_inputs_give_located_errors() {
    let cases: &[(&str, String, &str)] = &[
        (
            "short_bar",
            format!("1 rh | C5 | D5 | E5 |\n1 lh {R}"),
            "1 rh: 3 cells; time=4/4 has 4 cells",
        ),
        (
            "three_slots",
            format!("1 rh | C5 D5 E5 | D5 | E5 | F5 |\n1 lh {R}"),
            "1 rh cell 1: 3 slots. Use 1, 2, 4, 8, 16 or 32 slots, or mark a tuplet",
        ),
        (
            "bad_run",
            format!("1 rh | C5 | - | - E5 | F5 |\n1 lh {R}"),
            "1 rh cell 1: a `-` run of 5 eighths is not one note value",
        ),
        (
            "tie_without_start",
            format!("1 rh | C5 | ~D5 | E5 | F5 |\n1 lh {R}"),
            "1 rh cell 2: ~D5 has no tie start",
        ),
        (
            "x_differs",
            "0 rh | x C5 | D5 |\n0 lh | r | r |".to_string(),
            "0 lh: the x slots differ from those of the rh line",
        ),
        (
            "unknown_lane",
            format!("1 rh | C5 | D5 | E5 | F5 |\n1 lh {R}\n1 rx {R}"),
            "1 rx: unknown lane `rx`",
        ),
        (
            "dash_first",
            format!("1 rh | C5 | D5 | E5 | F5 |\n1 lh {R}\n\n2 rh | - | D5 | E5 | F5 |\n2 lh {R}"),
            "2 rh cell 1: `-` in the first slot of a bar",
        ),
        (
            "dash_after_empty",
            format!("1 rh | _ - | D5 | E5 | F5 |\n1 lh {R}"),
            "1 rh cell 1: `-` after `_` or `x`",
        ),
        (
            "tie_later_alone",
            format!("1 rh | C5^t | D5 | E5 | F5 |\n1 lh {R}"),
            "C5^t has no later ~C5",
        ),
        (
            "open_slur",
            format!("1 rh | C5( | D5 | E5 | F5 |\n1 lh {R}"),
            "1 rh cell 1: an open slur at the end of the file",
        ),
        (
            "numbered_slur_end",
            format!("1 rh | C5 | D5)1 | E5 | F5 |\n1 lh {R}"),
            "1 rh cell 2: )1 has no (1 before it",
        ),
        (
            "missing_lh",
            "1 rh | C5 | D5 | E5 | F5 |".to_string(),
            "1: the bar has no lh line",
        ),
        (
            "bad_label",
            format!("1 rh {R}\n1 lh {R}\n\n3 rh {R}\n3 lh {R}"),
            "3: this bar is number 2 by its position",
        ),
        (
            "header_attr_in_bar",
            format!("1 @ title=\"x\"\n1 rh {R}\n1 lh {R}"),
            "title= is a header attribute",
        ),
        (
            "hairpin_dash",
            format!("1 rh.hp | - | _ | _ | _ |\n1 rh {R}\n1 lh {R}"),
            "1 rh.hp cell 1: `-` with no span to continue",
        ),
        (
            "pedal_dash",
            format!("1 rh {R}\n1 lh {R}\n1 ped | - | _ | _ | _ |"),
            "1 ped cell 1: `-` when no pedal is held",
        ),
        (
            "velocity_range",
            format!("1 rh | C5:v+60 | D5 | E5 | F5 |\n1 lh {R}"),
            "the velocity of C5 is 140; it must be 1 to 127",
        ),
        (
            "tuplet_slots",
            format!("1 rh | 3[C5 D5] | D5 | E5 | F5 |\n1 lh {R}"),
            "1 rh cell 1: 2 slots in a 3-group",
        ),
        (
            "tuplet_ratio",
            format!("1 rh | 9[C5 D5 E5 F5 G5 A5 B5 C6 D6] | D5 | E5 | F5 |\n1 lh {R}"),
            "needs its ratio, for example 9:8[",
        ),
        (
            "bad_pitch_mark",
            format!("1 rh | C5^q | D5 | E5 | F5 |\n1 lh {R}"),
            "`^q` is not a pitch mark",
        ),
    ];
    for (name, body, want) in cases {
        let run = check_text(name, body);
        assert_eq!(run.code, 1, "{name}: expected an error\n{}", run.stdout);
        assert!(
            run.stdout.contains(want),
            "{name}: expected `{want}` in:\n{}",
            run.stdout
        );
    }
}

#[test]
fn unknown_header_attribute_is_an_error() {
    let dir = temp_dir("check-header");
    let f = write(
        &dir,
        "h.pno",
        "@ format=1\n@ tempo=3\n\n1 rh | R |\n1 lh | R |\n",
    );
    let run = pno(&["check", path(&f)]);
    assert_eq!(run.code, 1);
    assert!(
        run.stdout.contains("unknown attribute `tempo`"),
        "{}",
        run.stdout
    );
}

#[test]
fn hand_warnings_do_not_fail_the_check() {
    let run = check_text(
        "hands",
        "1 rh | C3 | C4+G5 | E5 | F5 |\n1 lh | C5 | - | - | - |",
    );
    assert_eq!(run.code, 0, "{}", run.stdout);
    assert!(
        run.stdout.contains("warning: 1 cell 1: the hands cross"),
        "{}",
        run.stdout
    );
    assert!(
        run.stdout
            .contains("warning: 1 rh cell 2: a span of 19 semitones"),
        "{}",
        run.stdout
    );
}

#[test]
fn bar_range_limits_the_report_but_not_the_result() {
    let body =
        format!("1 rh | C5 | ~D5 | E5 | F5 |\n1 lh {R}\n\n2 rh | C5 | D5 | E5 | F5 |\n2 lh {R}");
    let dir = temp_dir("check-range");
    let f = write(&dir, "r.pno", &format!("@ format=1\n\n{body}\n"));
    let run = pno(&["check", path(&f), "2"]);
    assert_eq!(run.code, 1, "{}", run.stdout);
    assert!(!run.stdout.contains("error"), "{}", run.stdout);
    assert!(
        run.stderr.contains("1 errors in other bars"),
        "{}",
        run.stderr
    );
    let run = pno(&["check", path(&f), "1-2"]);
    assert_eq!(run.code, 1, "{}", run.stdout);
    assert!(run.stdout.contains("error"), "{}", run.stdout);
}
