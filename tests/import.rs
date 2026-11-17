mod common;

use common::compare::{self, Detail};
use common::{fixtures, path, pno, root, temp_dir, write};

fn original() -> String {
    std::fs::read_to_string(root().join("tests/fixtures/tristesse.musicxml"))
        .expect("original score")
}

fn import(xml: &std::path::Path, out: &std::path::Path, extra: &[&str]) -> common::Run {
    let mut args = vec!["import", path(xml), "-o", path(out)];
    args.extend_from_slice(extra);
    pno(&args)
}

fn export(file: &std::path::Path, out: &std::path::Path) {
    let run = pno(&["export", path(file), path(out)]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
}

fn read(p: &std::path::Path) -> String {
    std::fs::read_to_string(p).expect("output is readable")
}

#[test]
fn test_score_imports_to_the_fixture_and_reports_the_drops() {
    let dir = temp_dir("import-score");
    let out = dir.join("score.pno");
    let run = import(&root().join("tests/fixtures/tristesse.musicxml"), &out, &[]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let fixture = read(&root().join("tests/fixtures/tristesse.pno"));
    assert_eq!(read(&out), fixture);
    for line in [
        "dropped 936: coordinates and sizes (by design)",
        "dropped 414: stems (keep them with --keep stems)",
        "dropped 559: beams (keep them with --keep beams)",
    ] {
        assert!(run.stdout.contains(line), "{line}\n{}", run.stdout);
    }
}

#[test]
fn fixtures_survive_export_and_import() {
    let dir = temp_dir("import-fixtures");
    for f in fixtures() {
        let name = f.file_stem().and_then(|s| s.to_str()).expect("file name");
        let xml = dir.join(format!("{name}.musicxml"));
        let back = dir.join(format!("{name}.pno"));
        export(&f, &xml);
        let run = import(&xml, &back, &[]);
        assert_eq!(run.code, 0, "{name}: {}", run.stderr);
        assert_eq!(read(&back), read(&f), "{name}");
    }
}

#[test]
fn kept_stems_and_beams_export_equal_to_the_original() {
    let dir = temp_dir("import-keep");
    let pno_file = dir.join("keep.pno");
    let xml = dir.join("keep.musicxml");
    let run = import(
        &root().join("tests/fixtures/tristesse.musicxml"),
        &pno_file,
        &["--keep", "stems,beams"],
    );
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(!run.stdout.contains("stems"), "{}", run.stdout);
    export(&pno_file, &xml);
    let want = compare::content(&original(), Detail::StemsBeams);
    let got = compare::content(&read(&xml), Detail::StemsBeams);
    let diffs = compare::differences(&want, &got);
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));

    let plain = dir.join("plain.musicxml");
    export(&root().join("tests/fixtures/tristesse.pno"), &plain);
    let without = compare::content(&read(&plain), Detail::StemsBeams);
    assert!(
        !compare::differences(&want, &without).is_empty(),
        "the comparison sees stems and beams"
    );
}

#[test]
fn kept_beams_of_grace_notes_survive_export_and_import() {
    let dir = temp_dir("import-grace");
    let text = "@ format=1\n\n1 rh | {/D5}C5 | {E5:bb,D5:be}C5 - B4 C5 | D5{C5:bb,D5:be} | E5 |\n1 lh | R |\n";
    let f = write(&dir, "g.pno", text);
    let xml = dir.join("g.musicxml");
    let back = dir.join("back.pno");
    export(&f, &xml);
    let run = import(&xml, &back, &["--keep", "beams"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(read(&back), text);
}

#[test]
fn velocity_lines_survive_export_and_import() {
    let dir = temp_dir("import-vel");
    let text = "@ format=1\n@ vel.p=52\n\n1 @      vel.rh=+10\n1 rh.dyn | p | _ | _ | _ |\n1 rh     | C5 | D5:v-10 | E5 | F5 |\n1 lh     | C3 | - | - | - |\n";
    let f = write(&dir, "v.pno", text);
    let xml = dir.join("v.musicxml");
    let back = dir.join("back.pno");
    export(&f, &xml);
    let run = import(&xml, &back, &[]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(read(&back), text);
}

#[test]
fn import_does_not_overwrite_without_an_output_file() {
    let dir = temp_dir("import-exists");
    let xml = write(&dir, "s.musicxml", &original());
    let target = write(&dir, "s.pno", "keep me");
    let run = pno(&["import", path(&xml)]);
    assert_eq!(run.code, 2, "{}", run.stdout);
    assert!(run.stderr.contains("give the output file with -o"));
    assert_eq!(read(&target), "keep me");
}

#[test]
fn import_stops_at_an_unsupported_element() {
    let dir = temp_dir("import-lyric");
    let xml = original().replacen("</pitch>", "</pitch>\n<lyric><text>la</text></lyric>", 1);
    let f = write(&dir, "l.musicxml", &xml);
    let run = import(&f, &dir.join("l.pno"), &[]);
    assert_ne!(run.code, 0);
    assert!(
        run.stderr
            .contains("the import stops at measure 0: lyrics is not supported"),
        "{}",
        run.stderr
    );
    assert!(!dir.join("l.pno").exists());
}

#[test]
fn musescore_3_forms_import_to_the_same_text() {
    let dir = temp_dir("import-ms3");
    let text = "@ format=1\n@ time=3/4\n@ volume=77.9528\n\n1 tempo  | \"Allegro\"=120 | _ | _ |\n1 rh.dyn | _ | fz _ | _ |\n1 rh     | C5 | E5 - - B4 | C5 |\n1 lh     | R |\n\n2 @      cells=2\n2 rh     | C5 | - |\n2 lh     | R |\n";
    let f = write(&dir, "m.pno", text);
    let xml = dir.join("m.musicxml");
    export(&f, &xml);
    let exported = read(&xml);
    assert!(exported.contains("<rest measure=\"yes\"/>"));
    let old_forms = exported
        .replace("<rest measure=\"yes\"/>", "<rest/>")
        .replace(" system=\"only-top\"", "")
        .replace(
            "<words>Allegro</words>",
            "<words>Alle</words><words>gro</words>",
        );
    let old = write(&dir, "old.musicxml", &old_forms);
    for (source, name) in [(&xml, "back.pno"), (&old, "old.pno")] {
        let back = dir.join(name);
        let run = import(source, &back, &[]);
        assert_eq!(run.code, 0, "{name}: {}", run.stderr);
        assert_eq!(read(&back), text, "{name}");
    }
}

#[test]
fn page_texts_with_no_type_go_to_the_field_of_their_place() {
    let dir = temp_dir("import-page-texts");
    let f = write(
        &dir,
        "a.pno",
        "@ format=1\n\n1 rh | C5 | D5 | E5 | F5 |\n1 lh | R |\n",
    );
    let xml = dir.join("a.musicxml");
    export(&f, &xml);
    let words = |x: u32, y: u32, justify: &str, size: u32, text: &str| {
        format!(
            "<credit-words default-x=\"{x}\" default-y=\"{y}\" justify=\"{justify}\" font-size=\"{size}\">{text}</credit-words>"
        )
    };
    let credit = |w: &[String]| format!("<credit page=\"1\">{}</credit>\n", w.concat());
    let page = [
        "<defaults><page-layout><page-height>1700</page-height></page-layout></defaults>\n"
            .to_string(),
        credit(&[words(600, 1600, "center", 24, "Sonata")]),
        credit(&[words(600, 1550, "center", 12, "Op. 2")]),
        credit(&[
            words(1100, 1500, "right", 10, "A. Composer"),
            words(1100, 1480, "right", 10, "(1900)"),
        ]),
        credit(&[words(100, 1500, "left", 10, "Allegro")]),
        credit(&[words(600, 1540, "center", 12, "No. 1")]),
        credit(&[words(600, 50, "center", 8, "Copyright 2026")]),
    ]
    .concat();
    let source = read(&xml).replacen("  <part-list>", &format!("{page}  <part-list>"), 1);
    let source = write(&dir, "b.musicxml", &source);
    let back = dir.join("b.pno");
    let run = import(&source, &back, &[]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let got = read(&back);
    let want = concat!(
        "@ format=1\n@ title=\"Sonata\"\n@ subtitle=\"Op. 2\\nNo. 1\"\n",
        "@ composer=\"A. Composer\\n(1900)\"\n@ lyricist=\"Allegro\"\n@ copyright=\"Copyright 2026\"\n"
    );
    assert!(got.starts_with(want), "{got}");
}

#[test]
fn a_dynamic_mark_with_no_velocity_gets_the_table_velocity() {
    let dir = temp_dir("import-table-velocity");
    let text = "@ format=1\n\n1 rh.dyn | p | _ | sf | _ |\n1 rh     | C5 | D5 | E5 | F5 |\n1 lh     | R |\n";
    let f = write(&dir, "t.pno", text);
    let xml = dir.join("t.musicxml");
    export(&f, &xml);
    let bare: String = read(&xml)
        .lines()
        .filter(|l| !l.contains("<sound dynamics="))
        .map(|l| format!("{l}\n"))
        .collect();
    let bare = write(&dir, "bare.musicxml", &bare);
    let back = dir.join("back.pno");
    let run = import(&bare, &back, &[]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(read(&back), text);
    assert!(
        run.stdout
            .contains("dropped 2: dynamic marks with no velocity"),
        "{}",
        run.stdout
    );
    let again = dir.join("again.musicxml");
    export(&back, &again);
    let diffs = compare::differences(
        &compare::content(&read(&bare), Detail::NoStemsBeams),
        &compare::content(&read(&again), Detail::NoStemsBeams),
    );
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}

#[test]
fn a_pedal_sign_while_the_pedal_is_down_is_a_change() {
    let dir = temp_dir("import-pedal-sign");
    let text = "@ format=1\n@ pedal-style=sign\n\n1 ped* | P | C | - | _ |\n1 rh   | C5 | D5 | E5 | F5 |\n1 lh   | R |\n";
    let f = write(&dir, "t.pno", text);
    let xml = dir.join("t.musicxml");
    export(&f, &xml);
    let signs = read(&xml).replace("<pedal type=\"change\"", "<pedal type=\"start\"");
    let signs = write(&dir, "signs.musicxml", &signs);
    let back = dir.join("back.pno");
    let run = import(&signs, &back, &[]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(read(&back), text);
    assert!(
        run.stdout.contains("pno writes a pedal change"),
        "{}",
        run.stdout
    );
    let again = dir.join("again.musicxml");
    export(&back, &again);
    let diffs = compare::differences(
        &compare::content(&read(&signs), Detail::NoStemsBeams),
        &compare::content(&read(&again), Detail::NoStemsBeams),
    );
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}

fn tuplet_notes(steps: &str, ticks: &[&str], kind: &str, ratio: (u32, u32)) -> String {
    let (actual, normal) = ratio;
    let last = steps.len() - 1;
    let mut out = String::new();
    for (i, (step, ticks)) in steps.chars().zip(ticks).enumerate() {
        if ticks.starts_with("F1") {
            out.push_str("<forward><duration>1</duration></forward>");
        }
        let ticks = ticks.trim_start_matches("F1");
        let edge = match i {
            0 => "<notations><tuplet type=\"start\"/></notations>",
            i if i == last => "<notations><tuplet type=\"stop\"/></notations>",
            _ => "",
        };
        out.push_str(&format!(
            "<note><pitch><step>{step}</step><octave>5</octave></pitch><duration>{ticks}</duration><voice>1</voice><type>{kind}</type><time-modification><actual-notes>{actual}</actual-notes><normal-notes>{normal}</normal-notes></time-modification><staff>1</staff>{edge}</note>"
        ));
    }
    out
}

#[test]
fn tuplet_lengths_that_the_divisions_round_import_exactly() {
    let note = |step: &str, ticks: u32, kind: &str| {
        format!(
            "<note><pitch><step>{step}</step><octave>5</octave></pitch><duration>{ticks}</duration><voice>1</voice><type>{kind}</type><staff>1</staff></note>"
        )
    };
    let rest = "<backup><duration>960</duration></backup><note><rest measure=\"yes\"/><duration>960</duration><voice>5</voice><staff>2</staff></note>";
    // 7 × 34 is two divisions short of the eighth; MuseScore corrects 9 × 53 with <forward>1.
    let septuplet = tuplet_notes("CDEFGAB", &["34"; 7], "32nd", (7, 4));
    let nonuplet = tuplet_notes(
        "CDEFGABCD",
        &["53", "53", "F153", "53", "53", "F153", "53", "53", "F153"],
        "32nd",
        (9, 8),
    );
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><score-partwise version=\"4.0\"><part-list><score-part id=\"P1\"><part-name>Piano</part-name></score-part></part-list><part id=\"P1\"><measure number=\"1\"><attributes><divisions>480</divisions><key><fifths>0</fifths></key><time><beats>2</beats><beat-type>4</beat-type></time><staves>2</staves><clef number=\"1\"><sign>G</sign><line>2</line></clef><clef number=\"2\"><sign>F</sign><line>4</line></clef></attributes>{septuplet}{}{}<backup><duration>958</duration></backup><note><rest measure=\"yes\"/><duration>960</duration><voice>5</voice><staff>2</staff></note></measure><measure number=\"2\">{nonuplet}{}{rest}</measure></part></score-partwise>",
        note("E", 240, "eighth"),
        note("G", 480, "quarter"),
        note("A", 480, "quarter"),
    );
    let dir = temp_dir("import-rounded-tuplets");
    let src = write(&dir, "t.musicxml", &xml);
    let back = dir.join("t.pno");
    let run = import(&src, &back, &[]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let text = read(&back);
    assert!(
        text.contains("1 rh | 7[C5 D5 E5 F5 G5 A5 B5] E5 | G5 |"),
        "{text}"
    );
    assert!(
        text.contains("2 rh | 9:8[C5 D5 E5 F5 G5 A5 B5 C5 D5] | A5 |"),
        "{text}"
    );
    let again = dir.join("again.musicxml");
    export(&back, &again);
    let diffs = compare::differences(
        &compare::content(&xml, Detail::NoStemsBeams),
        &compare::content(&read(&again), Detail::NoStemsBeams),
    );
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}

#[test]
fn breath_marks_that_pno_cannot_place_are_dropped_with_a_note() {
    let dir = temp_dir("import-breath");
    let xml = dir.join("p.musicxml");
    export(&root().join("tests/fixtures/spec/phrase.pno"), &xml);
    let text = read(&xml);
    let breath = "<notations>\n          <articulations>\n            <breath-mark/>\n          </articulations>\n        </notations>\n";
    let rest = text.find("<rest/>").expect("a rest");
    let rest_end = rest + text[rest..].find("</note>").expect("end of the rest");
    let last_end = text.rfind("</note>").expect("a note");
    let mut edited = text.clone();
    edited.insert_str(last_end, breath);
    edited.insert_str(rest_end, breath);
    let xml = write(&dir, "edited.musicxml", &edited);
    let out = dir.join("back.pno");
    let run = import(&xml, &out, &[]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    for line in [
        "a breath-mark on a rest is dropped",
        "a breath after the last note is dropped",
    ] {
        assert!(run.stdout.contains(line), "{line}\n{}", run.stdout);
    }
}
