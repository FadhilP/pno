mod common;

use std::fmt::Write as _;

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
        .flat_map(|l| [l, "\n"])
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
        let _ = write!(
            out,
            "<note><pitch><step>{step}</step><octave>5</octave></pitch><duration>{ticks}</duration><voice>1</voice><type>{kind}</type><time-modification><actual-notes>{actual}</actual-notes><normal-notes>{normal}</normal-notes></time-modification><staff>1</staff>{edge}</note>"
        );
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
fn hairpins_with_one_number_on_two_staves_and_back_to_back_survive_import_and_export() {
    let note = |step: &str, octave: u32, ticks: u32, kind: &str, voice: u32, staff: u32| {
        format!(
            "<note><pitch><step>{step}</step><octave>{octave}</octave></pitch><duration>{ticks}</duration><voice>{voice}</voice><type>{kind}</type><staff>{staff}</staff></note>"
        )
    };
    let hp = |kind: &str, staff: u32| {
        format!(
            "<direction placement=\"below\"><direction-type><wedge type=\"{kind}\" number=\"1\"/></direction-type><staff>{staff}</staff></direction>"
        )
    };
    let rh = |step: &str| note(step, 5, 1, "16th", 1, 1);
    let lh = |step: &str| note(step, 3, 2, "eighth", 5, 2);
    // MuseScore numbers the hairpins of each staff from 1. The upper staff has two hairpins that
    // meet at one time; the hairpin of the lower staff starts first and ends last.
    let upper = [
        rh("C"),
        hp("crescendo", 1),
        rh("D"),
        rh("E"),
        hp("stop", 1),
        hp("crescendo", 1),
        rh("F"),
        rh("G"),
        hp("stop", 1),
        rh("A"),
        rh("B"),
        rh("C"),
    ]
    .concat();
    let lower = [
        hp("crescendo", 2),
        lh("C"),
        lh("D"),
        lh("E"),
        hp("stop", 2),
        lh("F"),
    ]
    .concat();
    // A hairpin that starts in a gap of the first voice and ends on a note.
    let gap = [
        hp("diminuendo", 1),
        "<forward><duration>4</duration><voice>1</voice><staff>1</staff></forward>".to_string(),
        note("C", 5, 2, "eighth", 1, 1),
        hp("stop", 1),
        note("D", 5, 2, "eighth", 1, 1),
    ]
    .concat();
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><score-partwise version=\"4.0\"><part-list><score-part id=\"P1\"><part-name>Piano</part-name></score-part></part-list><part id=\"P1\"><measure number=\"1\"><attributes><divisions>4</divisions><key><fifths>0</fifths></key><time><beats>2</beats><beat-type>4</beat-type></time><staves>2</staves><clef number=\"1\"><sign>G</sign><line>2</line></clef><clef number=\"2\"><sign>F</sign><line>4</line></clef></attributes>{upper}<backup><duration>8</duration></backup>{lower}</measure><measure number=\"2\">{gap}<backup><duration>8</duration></backup><note><rest measure=\"yes\"/><duration>8</duration><voice>5</voice><staff>2</staff></note></measure></part></score-partwise>"
    );
    let dir = temp_dir("import-hairpin-numbers");
    let src = write(&dir, "h.musicxml", &xml);
    let back = dir.join("h.pno");
    let run = import(&src, &back, &[]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let again = dir.join("again.musicxml");
    export(&back, &again);
    let diffs = compare::differences(
        &compare::content(&xml, Detail::NoStemsBeams),
        &compare::content(&read(&again), Detail::NoStemsBeams),
    );
    assert!(diffs.is_empty(), "{}\n{}", diffs.join("\n"), read(&back));
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

#[test]
fn a_pedal_above_the_lower_staff_and_a_jump_text_below_are_reported() {
    let dir = temp_dir("import-fixed-places");
    let pedal = "<direction placement=\"above\"><direction-type><pedal type=\"start\" line=\"yes\"/></direction-type><staff>2</staff></direction>";
    let fine = "<direction placement=\"below\"><direction-type><words>Fine</words></direction-type><sound fine=\"yes\"/></direction>";
    let m1 = format!(
        "{}<backup><duration>2</duration></backup>{pedal}{}{fine}</measure>",
        note("C", 2, "half", 1, ""),
        note("C", 2, "half", 2, ""),
    );
    let xml = piano(
        1,
        "<time><beats>2</beats><beat-type>4</beat-type></time>",
        &m1,
    );
    let src = write(&dir, "a.musicxml", &xml);
    let run = import(&src, &dir.join("a.pno"), &[]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    for line in ["above the lower staff", "the place of jump texts"] {
        assert!(run.stdout.contains(line), "{line}\n{}", run.stdout);
    }
}

#[test]
fn a_note_at_a_time_that_no_slot_shows_stops_the_import() {
    let note = |step: &str, ticks: u32, kind: &str, voice: u32| {
        format!(
            "<note><pitch><step>{step}</step><octave>5</octave></pitch><duration>{ticks}</duration><voice>{voice}</voice><type>{kind}</type><staff>1</staff></note>"
        )
    };
    // The second voice starts a third of a beat late with no tuplet: OMR output had this.
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><score-partwise version=\"4.0\"><part-list><score-part id=\"P1\"><part-name>Piano</part-name></score-part></part-list><part id=\"P1\"><measure number=\"7\"><attributes><divisions>480</divisions><key><fifths>0</fifths></key><time><beats>2</beats><beat-type>4</beat-type></time><staves>2</staves><clef number=\"1\"><sign>G</sign><line>2</line></clef><clef number=\"2\"><sign>F</sign><line>4</line></clef></attributes>{}{}<backup><duration>960</duration></backup><forward><duration>160</duration></forward>{}<backup><duration>400</duration></backup><note><rest measure=\"yes\"/><duration>960</duration><voice>5</voice><staff>2</staff></note></measure></part></score-partwise>",
        note("G", 480, "quarter", 1),
        note("A", 480, "quarter", 1),
        note("E", 240, "eighth", 2),
    );
    let dir = temp_dir("import-off-grid");
    let src = write(&dir, "g.musicxml", &xml);
    let run = import(&src, &dir.join("g.pno"), &[]);
    assert_ne!(run.code, 0);
    assert!(
        run.stderr.contains(
            "the import stops at measure 7: a note or rest at a time that no slot or group shows ("
        ),
        "{}",
        run.stderr
    );
    assert!(!run.stderr.contains("internal"), "{}", run.stderr);
}

fn piano(divisions: u32, time: &str, measures: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><score-partwise version=\"4.0\"><part-list><score-part id=\"P1\"><part-name>Piano</part-name></score-part></part-list><part id=\"P1\"><measure number=\"1\"><attributes><divisions>{divisions}</divisions><key><fifths>0</fifths></key>{time}<staves>2</staves><clef number=\"1\"><sign>G</sign><line>2</line></clef><clef number=\"2\"><sign>F</sign><line>4</line></clef></attributes>{measures}</part></score-partwise>"
    )
}

fn note(step: &str, dur: u32, ty: &str, staff: u32, extra: &str) -> String {
    let voice = if staff == 1 { 1 } else { 5 };
    format!(
        "<note><pitch><step>{step}</step><octave>{}</octave></pitch><duration>{dur}</duration><voice>{voice}</voice><type>{ty}</type><staff>{staff}</staff>{extra}</note>",
        if staff == 1 { 5 } else { 3 }
    )
}

fn bar_rest(dur: u32, staff: u32) -> String {
    let voice = if staff == 1 { 1 } else { 5 };
    format!(
        "<note><rest measure=\"yes\"/><duration>{dur}</duration><voice>{voice}</voice><staff>{staff}</staff></note>"
    )
}

// Imports the MusicXML, exports it and imports it again; both imports give the same text.
fn round_trip(name: &str, xml: &str) -> String {
    let dir = temp_dir(name);
    let src = write(&dir, "a.musicxml", xml);
    let first = dir.join("a.pno");
    let run = import(&src, &first, &[]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let back = dir.join("b.musicxml");
    export(&first, &back);
    let second = dir.join("b.pno");
    let run = import(&back, &second, &[]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let text = read(&first);
    assert_eq!(text, read(&second));
    text
}

#[test]
fn a_trill_line_over_several_notes_survives_import_and_export() {
    // MuseScore puts the stop of a trill line on a note of any voice that ends with the line.
    let orn = |x: &str| format!("<notations><ornaments>{x}</ornaments></notations>");
    let v2 = |step: &str, x: &str| {
        format!(
            "<note><pitch><step>{step}</step><octave>4</octave></pitch><duration>480</duration><voice>2</voice><type>quarter</type><staff>1</staff>{x}</note>"
        )
    };
    let m1 = format!(
        "{}{}<backup><duration>960</duration></backup>{}{}<backup><duration>960</duration></backup>{}</measure>",
        note(
            "G",
            480,
            "quarter",
            1,
            &orn("<trill-mark/><wavy-line type=\"start\" number=\"1\"/>")
        ),
        note("A", 480, "quarter", 1, ""),
        v2("E", ""),
        v2("F", &orn("<wavy-line type=\"stop\" number=\"1\"/>")),
        bar_rest(960, 2),
    );
    let xml = piano(
        480,
        "<time><beats>2</beats><beat-type>4</beat-type></time>",
        &m1,
    );
    let text = round_trip("import-trill-lane", &xml);
    assert!(text.contains("rh.trl | tr | - |"), "{text}");
    assert!(text.contains("G5:tr"), "{text}");
}

#[test]
fn a_trill_line_that_starts_and_stops_on_one_note_is_a_trill_line_mark() {
    let line = "<notations><ornaments><trill-mark/><wavy-line type=\"start\" number=\"1\"/><wavy-line type=\"stop\" number=\"1\"/></ornaments></notations>";
    let m1 = format!(
        "{}{}<backup><duration>2</duration></backup>{}</measure>",
        note("G", 1, "quarter", 1, line),
        note("A", 1, "quarter", 1, ""),
        bar_rest(2, 2),
    );
    let xml = piano(
        1,
        "<time><beats>2</beats><beat-type>4</beat-type></time>",
        &m1,
    );
    let text = round_trip("import-one-note-trill", &xml);
    assert!(text.contains("G5:trl"), "{text}");
}

#[test]
fn a_cadenza_bar_longer_than_its_time_signature_survives_import_and_export() {
    // Two quarters and three 32nds in a 2/4 bar: three cells, the last ends with `x`.
    let wedge = |ty: &str| {
        format!(
            "<direction><direction-type><wedge type=\"{ty}\" number=\"1\"/></direction-type><staff>1</staff></direction>"
        )
    };
    let m1 = format!(
        "{}{}{}{}{}<backup><duration>19</duration></backup>{}</measure>",
        wedge("crescendo"),
        note("C", 8, "quarter", 1, ""),
        note("D", 8, "quarter", 1, ""),
        note("E", 1, "32nd", 1, ""),
        note("F", 1, "32nd", 1, "") + &note("G", 1, "32nd", 1, ""),
        bar_rest(19, 2),
    );
    let m2 = format!(
        "<measure number=\"2\">{}{}{}<backup><duration>16</duration></backup>{}</measure>",
        note("C", 8, "quarter", 1, ""),
        wedge("stop"),
        note("D", 8, "quarter", 1, ""),
        bar_rest(16, 2)
    );
    let xml = piano(
        8,
        "<time><beats>2</beats><beat-type>4</beat-type></time>",
        &(m1 + &m2),
    );
    let text = round_trip("import-cadenza", &xml);
    assert!(text.contains("cells=3"), "{text}");
    assert!(text.contains("rh.hp"), "{text}");
}

#[test]
fn a_tie_over_the_grace_notes_of_its_end_note_is_a_plain_tie() {
    let grace = "<note><grace slash=\"yes\"/><pitch><step>D</step><octave>5</octave></pitch><voice>1</voice><type>eighth</type><staff>1</staff></note>";
    let m1 = format!(
        "{}{grace}{}<backup><duration>2</duration></backup>{}</measure>",
        note(
            "C",
            1,
            "quarter",
            1,
            "<notations><tied type=\"start\"/></notations>"
        ),
        note(
            "C",
            1,
            "quarter",
            1,
            "<notations><tied type=\"stop\"/></notations>"
        ),
        bar_rest(2, 2),
    );
    let xml = piano(
        1,
        "<time><beats>2</beats><beat-type>4</beat-type></time>",
        &m1,
    );
    let text = round_trip("import-tie-over-graces", &xml);
    assert!(text.contains("~C5"), "{text}");
    assert!(!text.contains("^t"), "{text}");
}

#[test]
fn a_small_grace_note_survives_import_and_export() {
    let grace = "<note><grace slash=\"yes\"/><cue/><pitch><step>F</step><octave>5</octave></pitch><voice>1</voice><type>eighth</type><staff>1</staff></note>";
    let m1 = format!(
        "{grace}{}<backup><duration>2</duration></backup>{}</measure>",
        note("G", 2, "half", 1, ""),
        bar_rest(2, 2),
    );
    let xml = piano(
        1,
        "<time><beats>2</beats><beat-type>4</beat-type></time>",
        &m1,
    );
    let text = round_trip("import-small-grace", &xml);
    assert!(text.contains("{/F5:small}G5"), "{text}");
}

#[test]
fn a_tie_start_whose_end_belongs_to_a_later_tie_is_broken() {
    let dir = temp_dir("import-claimed-tie-end");
    let tie = |ty: &str| format!("<tie type=\"{ty}\"/>");
    let m1 = format!(
        "{}{}{}{}<backup><duration>4</duration></backup>{}</measure>",
        note("C", 1, "quarter", 1, &tie("start")),
        note("D", 1, "quarter", 1, ""),
        note("C", 1, "quarter", 1, &tie("start")),
        note("C", 1, "quarter", 1, &tie("stop")),
        bar_rest(4, 2),
    );
    let xml = piano(
        1,
        "<time><beats>4</beats><beat-type>4</beat-type></time>",
        &m1,
    );
    let src = write(&dir, "a.musicxml", &xml);
    let out = dir.join("a.pno");
    let run = import(&src, &out, &[]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert!(
        run.stdout.contains("ties with no start or no end note"),
        "{}",
        run.stdout
    );
    assert!(
        read(&out).contains("| C5 | D5 | C5 | ~C5 |"),
        "{}",
        read(&out)
    );
}

#[test]
fn losses_of_dynamic_order_grace_slash_and_drawn_ties_are_reported() {
    let dir = temp_dir("import-new-rows");
    let dyn_mark = |m: &str| {
        format!(
            "<direction><direction-type><dynamics><{m}/></dynamics></direction-type><staff>1</staff></direction>"
        )
    };
    let grace = |step: &str, slash: &str| {
        format!(
            "<note><grace{slash}/><pitch><step>{step}</step><octave>5</octave></pitch><voice>1</voice><type>eighth</type><staff>1</staff></note>"
        )
    };
    let m1 = format!(
        "{}{}{}{}{}{}<backup><duration>2</duration></backup>{}</measure>",
        dyn_mark("p"),
        dyn_mark("sf"),
        grace("D", ""),
        grace("E", " slash=\"yes\""),
        note(
            "C",
            1,
            "quarter",
            1,
            "<notations><tied type=\"start\"/></notations>"
        ),
        note(
            "C",
            1,
            "quarter",
            1,
            "<notations><tied type=\"stop\"/></notations>"
        ),
        bar_rest(2, 2),
    );
    let xml = piano(
        1,
        "<time><beats>2</beats><beat-type>4</beat-type></time>",
        &m1,
    );
    let src = write(&dir, "a.musicxml", &xml);
    let run = import(&src, &dir.join("a.pno"), &[]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    for line in [
        "the order of two dynamic marks at one time",
        "a slash on only some grace notes of a group",
        "ties that are drawn but not played",
    ] {
        assert!(run.stdout.contains(line), "{line}\n{}", run.stdout);
    }
}

#[test]
fn a_hidden_bar_rest_imports_as_an_empty_slot() {
    let m1 = format!(
        "{}<backup><duration>2</duration></backup>{}</measure>",
        note("C", 2, "half", 1, ""),
        bar_rest(2, 2).replace("<note>", "<note print-object=\"no\">"),
    );
    let xml = piano(
        1,
        "<time><beats>2</beats><beat-type>4</beat-type></time>",
        &m1,
    );
    let text = round_trip("import-hidden-bar-rest", &xml);
    let lh = text
        .lines()
        .find(|l| l.contains(" lh "))
        .expect("an lh line");
    assert!(lh.ends_with("| _ |"), "{text}");
}

#[test]
fn a_playback_tempo_that_differs_from_its_metronome_mark_imports_the_mark() {
    // Chopin op. 10 no. 3 shows q=100 and plays 35.
    let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><score-partwise version=\"4.0\"><part-list><score-part id=\"P1\"><part-name>Piano</part-name></score-part></part-list><part id=\"P1\"><measure number=\"1\"><attributes><divisions>1</divisions><key><fifths>0</fifths></key><time><beats>2</beats><beat-type>4</beat-type></time><staves>2</staves><clef number=\"1\"><sign>G</sign><line>2</line></clef><clef number=\"2\"><sign>F</sign><line>4</line></clef></attributes><direction placement=\"above\"><direction-type><metronome><beat-unit>quarter</beat-unit><per-minute>100</per-minute></metronome></direction-type><staff>1</staff><sound tempo=\"35\"/></direction><note><rest measure=\"yes\"/><duration>2</duration><voice>1</voice><staff>1</staff></note><backup><duration>2</duration></backup><note><rest measure=\"yes\"/><duration>2</duration><voice>5</voice><staff>2</staff></note></measure></part></score-partwise>";
    let dir = temp_dir("import-tempo-sound");
    let src = write(&dir, "t.musicxml", xml);
    let out = dir.join("t.pno");
    let run = import(&src, &out, &[]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let text = std::fs::read_to_string(out).expect("import is readable");
    assert!(text.contains("q=100"), "{text}");
    assert!(
        run.stdout
            .contains("a playback tempo that differs from its metronome mark"),
        "{}",
        run.stdout
    );
}

#[test]
fn a_backup_past_the_start_by_the_rounding_of_tuplet_notes_imports() {
    // MuseScore writes triplet eighths of 4/3 divisions as 1, so the backup of the full bar goes
    // one division past its start (Liszt, Mephisto Waltz bar 200).
    let note = |step: &str, dur: u32, ty: &str, tm: &str| {
        format!(
            "<note><pitch><step>{step}</step><octave>5</octave></pitch><duration>{dur}</duration><voice>1</voice><type>{ty}</type>{tm}<staff>1</staff></note>"
        )
    };
    let tm = "<time-modification><actual-notes>3</actual-notes><normal-notes>2</normal-notes></time-modification>";
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><score-partwise version=\"4.0\"><part-list><score-part id=\"P1\"><part-name>Piano</part-name></score-part></part-list><part id=\"P1\"><measure number=\"1\"><attributes><divisions>4</divisions><key><fifths>0</fifths></key><time><beats>2</beats><beat-type>4</beat-type></time><staves>2</staves><clef number=\"1\"><sign>G</sign><line>2</line></clef><clef number=\"2\"><sign>F</sign><line>4</line></clef></attributes>{}{}{}{}<backup><duration>8</duration></backup><note><rest measure=\"yes\"/><duration>8</duration><voice>5</voice><staff>2</staff></note></measure></part></score-partwise>",
        note("C", 1, "eighth", tm),
        note("D", 1, "eighth", tm),
        note("E", 1, "eighth", tm),
        note("F", 4, "quarter", ""),
    );
    let dir = temp_dir("import-backup-rounding");
    let src = write(&dir, "b.musicxml", &xml);
    let out = dir.join("b.pno");
    let run = import(&src, &out, &[]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let text = std::fs::read_to_string(out).expect("import is readable");
    assert!(text.contains("| 3[C5 D5 E5] | F5 |"), "{text}");
}

#[test]
fn a_tuplet_with_the_number_1_stops_the_import() {
    let note = |step: &str, tuplet: &str| {
        format!(
            "<note><pitch><step>{step}</step><octave>5</octave></pitch><duration>480</duration><voice>1</voice><type>quarter</type><staff>1</staff><notations>{tuplet}</notations></note>"
        )
    };
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><score-partwise version=\"4.0\"><part-list><score-part id=\"P1\"><part-name>Piano</part-name></score-part></part-list><part id=\"P1\"><measure number=\"1\"><attributes><divisions>480</divisions><key><fifths>0</fifths></key><time><beats>2</beats><beat-type>4</beat-type></time><staves>2</staves><clef number=\"1\"><sign>G</sign><line>2</line></clef><clef number=\"2\"><sign>F</sign><line>4</line></clef></attributes>{}{}<backup><duration>960</duration></backup><note><rest measure=\"yes\"/><duration>960</duration><voice>5</voice><staff>2</staff></note></measure></part></score-partwise>",
        note("G", "<tuplet type=\"start\"/>"),
        note("A", "<tuplet type=\"stop\"/>"),
    );
    let dir = temp_dir("import-tuplet-1-1");
    let src = write(&dir, "t.musicxml", &xml);
    let run = import(&src, &dir.join("t.pno"), &[]);
    assert_ne!(run.code, 0);
    assert!(
        run.stderr.contains("a tuplet with the number 1"),
        "{}",
        run.stderr
    );
}

#[test]
fn a_hollow_notehead_on_a_short_note_stops_the_import() {
    // Schubert writes hollow sixteenths in D. 899 no. 1; the import drew them filled.
    let note = |step: &str| {
        format!(
            "<note><pitch><step>{step}</step><octave>5</octave></pitch><duration>480</duration><voice>1</voice><type>quarter</type><notehead filled=\"no\">normal</notehead><staff>1</staff></note>"
        )
    };
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><score-partwise version=\"4.0\"><part-list><score-part id=\"P1\"><part-name>Piano</part-name></score-part></part-list><part id=\"P1\"><measure number=\"1\"><attributes><divisions>480</divisions><key><fifths>0</fifths></key><time><beats>2</beats><beat-type>4</beat-type></time><staves>2</staves><clef number=\"1\"><sign>G</sign><line>2</line></clef><clef number=\"2\"><sign>F</sign><line>4</line></clef></attributes>{}{}<backup><duration>960</duration></backup><note><rest measure=\"yes\"/><duration>960</duration><voice>5</voice><staff>2</staff></note></measure></part></score-partwise>",
        note("G"),
        note("A"),
    );
    let dir = temp_dir("import-notehead-fill");
    let src = write(&dir, "f.musicxml", &xml);
    let run = import(&src, &dir.join("f.pno"), &[]);
    assert_ne!(run.code, 0);
    assert!(
        run.stderr
            .contains("a notehead fill that is not the fill of its note value"),
        "{}",
        run.stderr
    );
}

#[test]
fn a_grace_tie_with_no_stop_ends_on_its_main_note() {
    let dir = temp_dir("import-grace-tie");
    let fixture = root().join("tests/fixtures/spec/grace_slur_tie.pno");
    let xml = dir.join("g.musicxml");
    export(&fixture, &xml);
    // The form of MuseScore 3: no stop on the main notes.
    let edited = read(&xml)
        .replace("<tie type=\"stop\"/>", "")
        .replace("<tied type=\"stop\"/>", "");
    let src = write(&dir, "edited.musicxml", &edited);
    let back = dir.join("back.pno");
    let run = import(&src, &back, &[]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(read(&back), read(&fixture));
}

#[test]
fn a_part_with_one_staff_survives_import_and_export() {
    let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><score-partwise version=\"4.0\"><part-list><score-part id=\"P1\"><part-name>Piano</part-name></score-part></part-list><part id=\"P1\"><measure number=\"1\"><attributes><divisions>1</divisions><key><fifths>0</fifths></key><time><beats>2</beats><beat-type>4</beat-type></time><clef><sign>G</sign><line>2</line></clef></attributes><note><pitch><step>C</step><octave>5</octave></pitch><duration>1</duration><voice>1</voice><type>quarter</type></note><note><pitch><step>D</step><octave>5</octave></pitch><duration>1</duration><voice>1</voice><type>quarter</type></note></measure></part></score-partwise>";
    let text = round_trip("import-one-staff", xml);
    assert!(text.contains("@ staves=1"), "{text}");
    assert!(!text.contains(" lh "), "{text}");
}

#[test]
fn new_marks_of_version_1_survive_import_and_export() {
    let dirs = "<direction placement=\"above\"><direction-type><rehearsal>A</rehearsal></direction-type><staff>1</staff></direction><direction placement=\"above\"><direction-type><words enclosure=\"rectangle\">4</words></direction-type><staff>1</staff></direction>";
    let gl = |t: &str| {
        format!("<notations><glissando line-type=\"wavy\" type=\"{t}\" number=\"1\"/></notations>")
    };
    let m1 = format!(
        "<attributes><clef number=\"1\"><sign>G</sign><line>2</line><clef-octave-change>-1</clef-octave-change></clef></attributes>{dirs}{}{}{}<backup><duration>1024</duration></backup>{}</measure>",
        note("C", 256, "256th", 1, &gl("start")),
        note("G", 256, "256th", 1, &gl("stop")),
        note("A", 512, "128th", 1, ""),
        bar_rest(1024, 2),
    );
    let xml = piano(
        16384,
        "<time symbol=\"cut\"><beats>4</beats><beat-type>4</beat-type></time>",
        &m1,
    );
    let text = round_trip("import-new-marks", &xml);
    for want in [
        "time=4/4:cut",
        "treble8vb",
        "\"A\":mark",
        "\"4\":box",
        "^gl",
    ] {
        assert!(text.contains(want), "{want} in {text}");
    }
}
