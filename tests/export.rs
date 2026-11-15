mod common;

use std::collections::BTreeMap;

use common::compare::{self, Detail};
use common::{fixtures, path, pno, root, temp_dir, write};
use roxmltree::{Document, Node, ParsingOptions};

fn parse(xml: &str) -> Document<'_> {
    let opts = ParsingOptions {
        allow_dtd: true,
        ..ParsingOptions::default()
    };
    Document::parse_with_options(xml, opts).expect("MusicXML parses")
}

fn child<'a>(n: Node<'a, 'a>, tag: &str) -> Option<Node<'a, 'a>> {
    n.children().find(|c| c.has_tag_name(tag))
}

fn text(n: Node, tag: &str) -> String {
    child(n, tag)
        .and_then(|c| c.text())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn int(n: Node, tag: &str) -> i64 {
    text(n, tag).parse().unwrap_or(0)
}

fn export(file: &std::path::Path, name: &str) -> String {
    let dir = temp_dir(&format!("export-{name}"));
    let out = dir.join(format!("{name}.musicxml"));
    let run = pno(&["export", path(file), path(&out)]);
    assert_eq!(run.code, 0, "{name}: {}{}", run.stdout, run.stderr);
    std::fs::read_to_string(out).expect("export is readable")
}

#[test]
fn test_score_export_equals_the_original() {
    let original = std::fs::read_to_string(root().join("tests/fixtures/tristesse.musicxml"))
        .expect("original score");
    let mine = export(&root().join("tests/fixtures/tristesse.pno"), "tristesse");
    let want = compare::content(&original, Detail::NoStemsBeams);
    let notes = want
        .measures
        .iter()
        .flatten()
        .filter(|s| s.contains("<note "))
        .count();
    assert_eq!(notes, 415);
    let diffs = compare::differences(&want, &compare::content(&mine, Detail::NoStemsBeams));
    assert!(
        diffs.is_empty(),
        "{}",
        diffs.join(
            "
"
        )
    );
}

#[test]
fn fixtures_export_equal_voices_and_closed_spans() {
    for f in fixtures() {
        let name = f
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("file name")
            .to_string();
        let xml = export(&f, &format!("balance-{name}"));
        let doc = parse(&xml);
        for m in doc.descendants().filter(|n| n.has_tag_name("measure")) {
            let mut voices: BTreeMap<String, i64> = BTreeMap::new();
            for n in m
                .children()
                .filter(|n| n.has_tag_name("note") || n.has_tag_name("forward"))
            {
                let is_chord = child(n, "chord").is_some();
                let is_grace = child(n, "grace").is_some();
                // A forward with no voice only places a direction.
                let no_voice = child(n, "voice").is_none();
                if is_chord || is_grace || no_voice {
                    continue;
                }
                *voices.entry(text(n, "voice")).or_default() += int(n, "duration");
            }
            let lengths: Vec<i64> = voices.values().copied().collect();
            assert!(
                lengths.windows(2).all(|w| w[0] == w[1]),
                "{name} measure {:?}: voices {voices:?}",
                m.attribute("number")
            );
        }
        for tag in [
            "tuplet",
            "slur",
            "tied",
            "wedge",
            "dashes",
            "octave-shift",
            "pedal",
        ] {
            let types: Vec<&str> = doc
                .descendants()
                .filter(|n| n.has_tag_name(tag))
                .filter_map(|n| n.attribute("type"))
                .collect();
            let stops = types.iter().filter(|t| **t == "stop").count();
            let starts = types
                .iter()
                .filter(|t| !["stop", "continue", "change", "let-ring"].contains(*t))
                .count();
            assert_eq!(starts, stops, "{name}: {tag} starts and stops");
        }
    }
}

#[test]
fn spec_examples_export_their_notation() {
    let spec = root().join("tests/fixtures/spec");
    let grace = export(&spec.join("grace.pno"), "grace");
    assert!(grace.contains("<grace slash=\"yes\"/>"));
    assert!(grace.contains("<grace steal-time-previous=\"50\"/>"));
    let tuplets = export(&spec.join("tuplet_spans.pno"), "tuplet_spans");
    assert!(
        tuplets
            .contains("<actual-notes>4</actual-notes>\n          <normal-notes>3</normal-notes>")
    );
    assert!(tuplets.contains("bracket=\"no\" show-number=\"none\""));
    let triplets = export(&spec.join("tuplets.pno"), "spec-tuplets");
    // Only the quarter G5 in a triplet of eighths names the unit.
    assert_eq!(
        triplets
            .matches("<normal-type>eighth</normal-type>")
            .count(),
        1,
        "{triplets}"
    );
    let ottava = export(&spec.join("ottava.pno"), "spec-ottava");
    assert!(
        ottava.contains(
            "placement=\"above\">\n        <direction-type>\n          <octave-shift type=\"stop\""
        ),
        "{ottava}"
    );
    let orn = export(&spec.join("ornaments.pno"), "ornaments");
    assert!(orn.contains(
        "<trill-mark/>\n            <accidental-mark placement=\"above\">flat</accidental-mark>"
    ));
    assert!(orn.contains("<delayed-turn/>"));
    assert!(orn.contains("<words>dolce</words>"));
    let cross = export(&spec.join("cross_staff.pno"), "cross_staff");
    assert!(
        cross.contains("<voice>5</voice>\n        <type>eighth</type>\n        <staff>1</staff>")
    );
    let tremolo = export(&spec.join("tremolo.pno"), "tremolo");
    assert!(tremolo.contains("<tremolo type=\"start\">3</tremolo>"));
    assert!(tremolo.contains("<type>half</type>"));
    let hidden = export(&spec.join("hidden_silent.pno"), "hidden_silent");
    assert!(hidden.contains("<note print-object=\"no\">"));
    assert!(hidden.contains("<note dynamics=\"0\">"));
    // A hidden F#5 does not change the alteration, so only the shown F#5 has a sign.
    assert_eq!(hidden.matches("<accidental>").count(), 1, "{hidden}");
    let dyn_text = export(&spec.join("dynamic_text.pno"), "dynamic_text");
    assert!(
        dyn_text.contains(
            "<other-dynamics>sempre pp</other-dynamics></dynamics>
        </direction-type>
        <staff>1</staff>
        <sound dynamics=\"36.67\"/>"
        ),
        "{dyn_text}"
    );
    let marks = export(&spec.join("mark_tuplets.pno"), "mark_tuplets");
    let at = |s: &str| marks.find(s).unwrap_or(usize::MAX);
    // The hairpin starts on the second triplet eighth (D5) and stops before the third of beat 3 (B5).
    assert!(at("<step>C</step>") < at("crescendo"));
    assert!(at("crescendo") < at("<step>D</step>"));
    assert!(at("<step>A</step>") < at("<wedge type=\"stop\""));
    assert!(at("<wedge type=\"stop\"") < at("<step>B</step>"));
}

#[test]
fn velocity_lines_go_to_the_miscellaneous_block() {
    let dir = temp_dir("export-vel");
    let f = write(
        &dir,
        "v.pno",
        "@ format=1\n@ vel.p=52\n\n1 @ vel.rh=+10\n1 rh.dyn | p | _ | _ | _ |\n1 rh | C5 | D5:v-10 | E5 | F5 |\n1 lh | C3 | - | - | - |\n",
    );
    let xml = export(&f, "v");
    assert!(
        xml.contains("<miscellaneous-field name=\"pno-vel-1\">@ vel.p=52</miscellaneous-field>")
    );
    assert!(
        xml.contains(
            "<miscellaneous-field name=\"pno-vel-2\">1 @ vel.rh=+10</miscellaneous-field>"
        )
    );
    // p is 52; rh adds 10, so C5 is 62 and D5 is back at 52. The lh is at the level of the mark.
    assert!(xml.contains("<sound dynamics=\"57.78\"/>"));
    assert_eq!(
        xml.matches("dynamics=\"68.89\"").count(),
        3,
        "C5, E5 and F5 at 62"
    );
    assert!(
        !xml.contains("<note dynamics=\"57.78\""),
        "no attribute at the mark level"
    );
}
