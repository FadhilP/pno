mod common;

use common::{fixtures, path, pno, temp_dir, write};

fn fmt_text(name: &str, text: &str, args: &[&str]) -> String {
    let dir = temp_dir("fmt");
    let f = write(&dir, &format!("{name}.pno"), text);
    let mut all = vec!["fmt", path(&f)];
    all.extend_from_slice(args);
    let run = pno(&all);
    assert_eq!(run.code, 0, "{name}: {}{}", run.stdout, run.stderr);
    std::fs::read_to_string(&f).expect("formatted file is readable")
}

#[test]
fn fixtures_are_canonical() {
    for f in fixtures() {
        let run = pno(&["fmt", path(&f), "--check"]);
        assert_eq!(
            run.code,
            0,
            "{} is not canonical: {}",
            f.display(),
            run.stderr
        );
    }
}

#[test]
fn fmt_writes_the_canonical_form() {
    let messy = "\
@ time=4/4
@ format=1
1 lh | R |
1 rh | E5 - | - F5 | G4+C4:st:ac A5 | - - |
1 rh.dyn | p:below | _ _ | _ | _ |
1 tempo | \"rit.\" - | - - | _ | _ |
";
    let want = "\
@ format=1

1 tempo  | \"rit.\" | - | _ | _ |
1 rh.dyn | p | _ | _ | _ |
1 rh     | E5 | - F5 | C4+G4:ac:st A5 | - |
1 lh     | R |
";
    let got = fmt_text("messy", messy, &[]);
    assert_eq!(got, want);
    assert_eq!(fmt_text("again", &got, &[]), want, "fmt is idempotent");
}

#[test]
fn fmt_drops_marks_equal_to_their_default() {
    let input = "\
@ format=1
@ key=F

1 ped* | P | - | - | - |
1 rh.clef | treble | _ | _ | _ |
1 rh | Bb4! E4^hide C#5 - | C#5! | D5:up | - |
1 rh2 | G4 | - | - | D4:down |
1 lh | R |

2 rh | C5 | - | - | - |
2 lh | R |
2 ped | P | - | - | - |
";
    let want = "\
@ format=1
@ key=F

1 ped* | P | - | - | - |
1 rh   | Bb4! E4 C#5 - | C#5! | D5 | - |
1 rh2  | G4 | - | - | D4 |
1 lh   | R |

2 rh   | C5 | - | - | - |
2 lh   | R |
";
    assert_eq!(fmt_text("defaults", input, &[]), want);
}

#[test]
fn fix_splits_runs_and_renumbers_bars() {
    let input = "\
@ format=1

5 rh | C5 | - | - E5 | F5 |
5 lh | R |

9 rh | C5+E5 - - - - D5 - - | E5 | F5 | G5 |
9 lh | R |
";
    let want = "\
@ format=1

1 rh | C5 | - | ~C5 E5 | F5 |
1 lh | R |

2 rh | C5+E5 - - - ~C5+~E5 D5 - - | E5 | F5 | G5 |
2 lh | R |
";
    assert_eq!(fmt_text("fix", input, &["--fix"]), want);
}

#[test]
fn fmt_refuses_a_file_that_does_not_parse() {
    let dir = temp_dir("fmt-bad");
    let f = write(
        &dir,
        "bad.pno",
        "@ format=1\n\n1 rh | C5 D5 E5 | - | - | - |\n1 lh | R |\n",
    );
    let before = std::fs::read_to_string(&f).expect("readable");
    let run = pno(&["fmt", path(&f)]);
    assert_eq!(run.code, 1);
    assert_eq!(
        std::fs::read_to_string(&f).expect("readable"),
        before,
        "the file is unchanged"
    );
}
