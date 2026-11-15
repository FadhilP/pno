use std::collections::{BTreeMap, BTreeSet, HashMap};

use roxmltree::{Document, Node, NodeId, ParsingOptions};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    NoStemsBeams,
    StemsBeams,
}

pub struct Content {
    pub header: BTreeSet<String>,
    pub measures: Vec<Vec<String>>,
}

pub fn parse(xml: &str) -> Document<'_> {
    let opts = ParsingOptions {
        allow_dtd: true,
        ..ParsingOptions::default()
    };
    Document::parse_with_options(xml, opts).expect("MusicXML parses")
}

// Section 4.1: coordinates and sizes, and the font style of text.
const DROPPED_ATTRS: [&str; 13] = [
    "color",
    "default-x",
    "default-y",
    "relative-x",
    "relative-y",
    "width",
    "font-style",
    "font-family",
    "font-size",
    "font-weight",
    "justify",
    "halign",
    "valign",
];

const MOMENTARY: [&str; 8] = ["sf", "sfz", "fz", "rf", "rfz", "sffz", "fp", "sfp"];
const DEFAULT_LEVEL: f64 = 80.0 / 90.0 * 100.0;
const P_LEVEL: f64 = 49.0 / 90.0 * 100.0;

fn child<'a>(n: Node<'a, 'a>, tag: &str) -> Option<Node<'a, 'a>> {
    n.children().find(|c| c.has_tag_name(tag))
}

fn own_text(n: Node) -> String {
    n.text().unwrap_or("").to_string()
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

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 {
        a.abs().max(1)
    } else {
        gcd(b, a % b)
    }
}

fn frac(v: i64, div: i64) -> String {
    let g = gcd(v, div);
    format!("{}/{}", v / g, div / g)
}

// A time in whole notes: numerator and denominator.
type Q = (i64, i64);

fn q(n: i64, d: i64) -> Q {
    let g = gcd(n, d);
    (n / g, d / g)
}

fn q_add(a: Q, b: Q) -> Q {
    q(a.0 * b.1 + b.0 * a.1, a.1 * b.1)
}

fn q_less(a: Q, b: Q) -> bool {
    a.0 * b.1 < b.0 * a.1
}

fn q_text(a: Q) -> String {
    frac(a.0, a.1)
}

fn type_length(name: &str) -> Option<Q> {
    let types = [
        "maxima", "long", "breve", "whole", "half", "quarter", "eighth", "16th", "32nd", "64th",
        "128th", "256th", "512th", "1024th",
    ];
    let i = types.iter().position(|t| *t == name)? as u32;
    Some(if i <= 3 {
        (1 << (3 - i), 1)
    } else {
        (1, 1 << (i - 3))
    })
}

// As the import: a tuplet note within one division of its written length has that length.
fn exact_length(e: Node, ticks: Q, whole: i64) -> Option<Q> {
    let tm = child(e, "time-modification")?;
    let (n, d) = type_length(&text(e, "type"))?;
    let dots = e.children().filter(|c| c.has_tag_name("dot")).count() as u32;
    let written = q(n * ((1 << (dots + 1)) - 1), d << dots);
    let exact = q(
        written.0 * int(tm, "normal-notes"),
        written.1 * int(tm, "actual-notes").max(1),
    );
    let one = (1, whole);
    let near = q_less(exact, q_add(ticks, one)) && q_less(ticks, q_add(exact, one));
    near.then_some(exact)
}

// As the import: the first rounded note of a tuplet starts on a multiple of its plain value.
fn tuplet_start(e: Node, pos: i64, whole: i64) -> Option<Q> {
    let (n, d) = type_length(&text(e, "type"))?;
    if (n * whole) % d != 0 {
        return None;
    }
    let u = n * whole / d;
    let actual = int(child(e, "time-modification")?, "actual-notes");
    let nearest = (pos + u / 2) / u * u;
    ((nearest - pos).abs() <= actual).then(|| q(nearest, whole))
}

// The start and length of each note that is not a grace note, as the import reads them.
fn note_times(m: Node, div: i64) -> HashMap<NodeId, (Q, Q)> {
    let whole = div * 4;
    let mut out = HashMap::new();
    let mut pos = 0i64;
    let mut last_start = (0, 1);
    let mut exact_end: Option<Q> = None;
    let mut slack = 0;
    for e in m.children().filter(Node::is_element) {
        match e.tag_name().name() {
            "backup" => pos -= int(e, "duration"),
            "forward" => pos += int(e, "duration"),
            "note" if child(e, "grace").is_none() => {
                let d = int(e, "duration");
                let ticks = q(d, whole);
                let len = exact_length(e, ticks, whole).unwrap_or(ticks);
                if child(e, "chord").is_some() {
                    out.insert(e.id(), (last_start, len));
                    continue;
                }
                let at = q(pos, whole);
                let room = (slack, whole);
                let mut start = match exact_end {
                    Some(end) if q_less(end, q_add(at, room)) && q_less(at, q_add(end, room)) => {
                        end
                    }
                    _ => at,
                };
                if len != ticks && start == at {
                    start = tuplet_start(e, pos, whole).unwrap_or(at);
                }
                if len != ticks {
                    let actual =
                        child(e, "time-modification").map_or(1, |t| int(t, "actual-notes"));
                    slack = (slack + 1).min(actual);
                } else if start == at {
                    slack = 0;
                }
                out.insert(e.id(), (start, len));
                last_start = start;
                exact_end = Some(q_add(start, len));
                pos += d;
            }
            _ => {}
        }
    }
    out
}

// A MusicXML default that the export does not write.
fn is_default(tag: &str, name: &str, value: &str) -> bool {
    matches!(
        (tag, name, value),
        ("strong-accent", "type", "up") | ("tuplet", "number", "1")
    )
}

// MusicXML: with no `sign`, a pedal with a line has no sign, and a pedal with no line has one.
fn pedal_defaults(n: Node, attrs: &mut Vec<String>) {
    let line = n.attribute("line").unwrap_or("no");
    if n.attribute("line").is_none() {
        attrs.push("line=no".into());
    }
    if n.attribute("sign").is_none() {
        let sign = if line == "yes" { "no" } else { "yes" };
        attrs.push(format!("sign={sign}"));
    }
}

// 4.1: a text or tempo with no placement is at the default place of its staff.
fn text_placement(direction: Node) -> Option<String> {
    let marks: Vec<&str> = direction
        .children()
        .filter(|c| c.has_tag_name("direction-type"))
        .flat_map(|t| t.children().filter(Node::is_element))
        .map(|m| m.tag_name().name())
        .collect();
    let lower = text(direction, "staff") == "2";
    let place = if marks.contains(&"metronome") {
        "above"
    } else if marks.iter().all(|m| *m == "words") && !marks.is_empty() {
        if lower { "below" } else { "above" }
    } else {
        return None;
    };
    Some(format!("placement={place}"))
}

// MusicXML: a `normal-type` equal to the type of the note is the default.
fn unit_is_note_type(time_mod: Node) -> bool {
    let Some(note) = time_mod.parent() else {
        return false;
    };
    let dots = |n: Node, tag: &str| n.children().filter(|c| c.has_tag_name(tag)).count();
    text(time_mod, "normal-type") == text(note, "type")
        && dots(time_mod, "normal-dot") == dots(note, "dot")
}

struct Rules {
    detail: Detail,
    span_ends: HashMap<NodeId, String>,
    pedal_types: HashMap<NodeId, &'static str>,
}

fn divisions(m: Node) -> Option<i64> {
    m.descendants()
        .find(|n| n.has_tag_name("divisions"))
        .map(|d| d.text().unwrap_or("1").trim().parse().unwrap_or(1))
}

#[derive(Clone, Copy)]
struct Timed<'a> {
    order: usize,
    measure: usize,
    start: Q,
    node: Node<'a, 'a>,
}

fn timed<'a>(doc: &'a Document) -> Vec<Timed<'a>> {
    let mut out = Vec::new();
    let mut div = 1;
    for (measure, m) in doc
        .descendants()
        .filter(|n| n.has_tag_name("measure"))
        .enumerate()
    {
        div = divisions(m).unwrap_or(div);
        let times = note_times(m, div);
        let (mut pos, mut last) = (0i64, 0i64);
        for e in m.children().filter(Node::is_element) {
            let start = match e.tag_name().name() {
                "backup" => {
                    pos -= int(e, "duration");
                    continue;
                }
                "forward" => {
                    pos += int(e, "duration");
                    continue;
                }
                "note" => {
                    let chord = child(e, "chord").is_some();
                    let start = if chord { pos - last } else { pos };
                    if !chord {
                        last = int(e, "duration");
                        pos += last;
                    }
                    times.get(&e.id()).map_or(q(start, div * 4), |t| t.0)
                }
                "direction" => q(pos + int(e, "offset"), div * 4),
                _ => continue,
            };
            out.push(Timed {
                order: out.len(),
                measure,
                start,
                node: e,
            });
        }
    }
    out
}

struct SpanMark<'a> {
    // As MusicXML readers: document order; on one note a stop before a start.
    order: (usize, bool),
    place: String,
    node: Node<'a, 'a>,
}

const SPANS: [&str; 4] = ["slur", "wedge", "dashes", "octave-shift"];

// Span numbers are labels that the import may change: each end is named by the place of the other end.
fn span_ends(doc: &Document) -> HashMap<NodeId, String> {
    let mut marks: Vec<SpanMark> = Vec::new();
    for t in timed(doc) {
        let place = format!("{}@{}", t.measure, q_text(t.start));
        for s in t
            .node
            .descendants()
            .filter(|s| SPANS.contains(&s.tag_name().name()))
        {
            let Some(kind) = s.attribute("type").filter(|t| *t != "continue") else {
                continue;
            };
            marks.push(SpanMark {
                order: (t.order, kind != "stop"),
                place: place.clone(),
                node: s,
            });
        }
    }
    marks.sort_by_key(|m| m.order);
    // A stop closes the last open span of its type and number.
    let mut open: HashMap<(&str, &str), Vec<(String, NodeId)>> = HashMap::new();
    let mut ends = HashMap::new();
    for SpanMark {
        order: (_, is_start),
        place,
        node: s,
    } in marks
    {
        let key = (s.tag_name().name(), s.attribute("number").unwrap_or("1"));
        if is_start {
            open.entry(key).or_default().push((place, s.id()));
        } else if let Some((from, id)) = open.get_mut(&key).and_then(Vec::pop) {
            ends.insert(id, format!("to={place}"));
            ends.insert(s.id(), format!("from={from}"));
        }
    }
    ends
}

// 4.1: a `Ped.` sign while the pedal is down is a pedal change; a `*` sign with no pedal down is nothing.
fn pedal_types(doc: &Document) -> HashMap<NodeId, &'static str> {
    let mut pedals: Vec<(Timed, Node)> = timed(doc)
        .into_iter()
        .flat_map(|t| {
            let ps: Vec<Node> = t
                .node
                .descendants()
                .filter(|p| p.has_tag_name("pedal"))
                .collect();
            ps.into_iter().map(move |p| (t, p))
        })
        .collect();
    let is_start = |p: &Node| p.attribute("type") != Some("stop");
    // As the import: in time order, a stop before a start at one moment.
    pedals.sort_by(|(a, pa), (b, pb)| {
        let (sa, sb) = (a.start.0 * b.start.1, b.start.0 * a.start.1);
        (a.measure, sa, is_start(pa)).cmp(&(b.measure, sb, is_start(pb)))
    });
    let mut held = false;
    let mut out = HashMap::new();
    for (_, p) in pedals {
        let sign = p.attribute("line") != Some("yes");
        match p.attribute("type") {
            Some("start") if held && sign => {
                out.insert(p.id(), "change");
            }
            Some("change") if !held => {
                out.insert(p.id(), "start");
                held = true;
            }
            Some("stop") if !held && sign => {
                out.insert(p.id(), "");
            }
            Some("stop") => held = false,
            Some("start" | "change") => held = true,
            _ => {}
        }
    }
    out
}

impl Rules {
    fn skip_child(&self, parent: &str, tag: &str) -> bool {
        let presentation = matches!(tag, "stem" | "beam") && parent == "note";
        match (parent, tag) {
            // The size of a note is compared as `small` of its chord.
            ("note", "voice" | "duration" | "cue") => true,
            _ if presentation => self.detail == Detail::NoStemsBeams,
            // 4.1: the vertical position of rests.
            ("rest", "display-step" | "display-octave") => true,
            // 4.1: the voice numbers of the source; the offset is in the time.
            ("direction", "voice" | "offset") => true,
            // 4.1: text lines with a bracket.
            ("direction-type", "bracket") => true,
            _ => false,
        }
    }

    fn skip_attr(&self, parent: &str, grand: &str, name: &str) -> bool {
        if DROPPED_ATTRS.contains(&name) {
            return true;
        }
        // 4.1: the placement of articulations, ornaments and fingering, and the bracket of a tuplet.
        let placed = matches!(grand, "articulations" | "ornaments" | "technical");
        match name {
            "placement" if placed => true,
            "bracket" => parent == "tuplet",
            "size" => parent == "type",
            // Compared as the level of the direction.
            "dynamics" if parent == "sound" => true,
            "dynamics" => parent == "note",
            // A rest with no type that fills its measure is also a bar rest (MuseScore 2 and 3).
            "measure" => parent == "rest",
            // 4.1: the staves that show a tempo mark.
            "system" | "directive" => parent == "direction",
            _ => false,
        }
    }

    fn canon(&self, n: Node, grand: &str) -> String {
        let tag = n.tag_name().name();
        let mut attrs: Vec<String> = n
            .attributes()
            .filter(|a| !self.skip_attr(tag, grand, a.name()))
            .filter(|a| !is_default(tag, a.name(), a.value()))
            .map(|a| format!("{}={}", a.name(), a.value()))
            .collect();
        if let Some(end) = self.span_ends.get(&n.id()) {
            attrs.retain(|a| !a.starts_with("number="));
            attrs.push(end.clone());
        }
        if tag == "pedal" {
            if let Some(kind) = self.pedal_types.get(&n.id()) {
                if kind.is_empty() {
                    return String::new();
                }
                attrs.retain(|a| !a.starts_with("type="));
                attrs.push(format!("type={kind}"));
            }
            pedal_defaults(n, &mut attrs);
        }
        if tag == "direction" && n.attribute("placement").is_none() {
            attrs.extend(text_placement(n));
        }
        attrs.sort();
        let words: Vec<Node> = n.children().filter(Node::is_element).collect();
        let only_words = !words.is_empty() && words.iter().all(|w| w.has_tag_name("words"));
        if tag == "direction-type" && only_words {
            // Words that follow each other are one text.
            let text: String = words.iter().filter_map(|w| w.text()).collect();
            if text.trim().is_empty() {
                return String::new();
            }
            return format!("<{tag} ><words >{}</></>", text.trim());
        }
        // A hidden quarter metronome is `=N`, the same as `<sound tempo>` alone.
        let hidden = n.attribute("print-object") == Some("no");
        let quarter = text(n, "beat-unit") == "quarter" && child(n, "beat-unit-dot").is_none();
        if tag == "metronome" && hidden && quarter {
            return String::new();
        }
        let own_unit = tag == "time-modification" && unit_is_note_type(n);
        let mut kids: Vec<String> = n
            .children()
            .filter(Node::is_element)
            .filter(|c| !self.skip_child(tag, c.tag_name().name()))
            .filter(|c| !(own_unit && matches!(c.tag_name().name(), "normal-type" | "normal-dot")))
            .map(|c| self.canon(c, tag))
            .filter(|c| !c.is_empty())
            .collect();
        if tag == "direction" {
            // 4.1: the staff of a pedal mark; pno writes each one below the lower staff.
            let pedal = n.descendants().any(|d| d.has_tag_name("pedal"));
            if pedal {
                kids.retain(|k| !k.starts_with("<staff "));
            }
            if pedal || child(n, "staff").is_none() {
                let staff = if pedal {
                    "<staff >2</>"
                } else {
                    "<staff >1</>"
                };
                let at = kids.iter().position(|k| k.starts_with("<sound"));
                kids.insert(at.unwrap_or(kids.len()), staff.into());
            }
        }
        let empty = attrs.is_empty() && kids.is_empty();
        if empty && matches!(tag, "sound" | "direction-type") {
            return String::new();
        }
        if matches!(
            tag,
            "notations" | "articulations" | "ornaments" | "technical"
        ) {
            kids.sort();
            // 4.1: duplicate marks on one chord.
            kids.dedup();
        }
        let body = n
            .text()
            .map(str::trim)
            .filter(|t| !t.is_empty() && kids.is_empty())
            .unwrap_or("");
        format!("<{tag} {}>{body}{}</>", attrs.join(" "), kids.join(""))
    }
}

struct Level {
    carried: f64,
}

fn dynamics_name<'a>(direction: Node<'a, 'a>) -> Option<&'a str> {
    direction
        .descendants()
        .find(|d| d.has_tag_name("dynamics"))
        .and_then(|d| d.children().find(Node::is_element))
        .map(|d| d.tag_name().name())
}

// 4.1: a mark with no velocity has the table velocity.
fn direction_level(direction: Node) -> Option<f64> {
    child(direction, "sound")
        .and_then(|s| s.attribute("dynamics"))
        .and_then(|v| v.parse::<f64>().ok())
        .or_else(|| dynamics_name(direction).and_then(table_level))
}

fn table_level(mark: &str) -> Option<f64> {
    let velocity = match mark {
        "pppp" => 8.0,
        "ppp" => 16.0,
        "pp" => 33.0,
        "p" => 49.0,
        "mp" => 64.0,
        "mf" => 80.0,
        "f" | "fp" => 96.0,
        "ff" | "sf" | "sfz" | "sfp" | "fz" | "rf" | "rfz" => 112.0,
        "fff" | "sffz" => 126.0,
        "ffff" => 127.0,
        _ => return None,
    };
    Some(velocity / 90.0 * 100.0)
}

impl Level {
    fn marks(&self, m: Node) -> Vec<(i64, f64, Option<&'static str>)> {
        let mut pos = 0i64;
        let mut out = Vec::new();
        for e in m.children().filter(Node::is_element) {
            match e.tag_name().name() {
                "backup" => pos -= int(e, "duration"),
                "forward" => pos += int(e, "duration"),
                "note" if child(e, "chord").is_none() => pos += int(e, "duration"),
                "direction" => {
                    let Some(v) = direction_level(e) else {
                        continue;
                    };
                    let name = dynamics_name(e);
                    let momentary = MOMENTARY.iter().copied().find(|m| Some(*m) == name);
                    out.push((pos + int(e, "offset"), v, momentary));
                }
                _ => {}
            }
        }
        out.sort_by_key(|x| x.0);
        out
    }

    fn at(&self, marks: &[(i64, f64, Option<&str>)], t: i64) -> f64 {
        let mut level = self.carried;
        for (at, v, momentary) in marks {
            if *at > t {
                break;
            }
            level = match momentary {
                None => *v,
                Some(_) if *at == t => *v,
                Some("fp" | "sfp") => P_LEVEL,
                Some(_) => level,
            };
        }
        level
    }
}

fn voice_ranks(m: Node) -> BTreeMap<String, (String, usize)> {
    let mut staffs: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for n in m.children().filter(|n| n.has_tag_name("note")) {
        let hidden = n.attribute("print-object") == Some("no") && child(n, "rest").is_some();
        if hidden {
            continue;
        }
        *staffs
            .entry(text(n, "voice"))
            .or_default()
            .entry(text(n, "staff"))
            .or_default() += 1;
    }
    let home: BTreeMap<String, String> = staffs
        .iter()
        .map(|(v, s)| {
            let best = s.iter().max_by_key(|(_, c)| **c).map(|(s, _)| s.clone());
            (v.clone(), best.unwrap_or_default())
        })
        .collect();
    let mut voices: Vec<&String> = home.keys().collect();
    voices.sort_by_key(|v| v.parse::<u32>().unwrap_or(0));
    let mut out = BTreeMap::new();
    for v in voices {
        let staff = home[v].clone();
        let rank = out
            .values()
            .filter(|(s, _): &&(String, usize)| *s == staff)
            .count()
            + 1;
        out.insert(v.clone(), (staff, rank));
    }
    out
}

fn push_field(
    fields: &mut Vec<(&'static str, String)>,
    field: &'static str,
    t: &str,
    replace: bool,
) {
    let t = t.trim().to_string();
    if t.is_empty() {
        return;
    }
    match fields.iter_mut().find(|(f, _)| *f == field) {
        Some(slot) if replace => slot.1 = t,
        Some(_) => {}
        None => fields.push((field, t)),
    }
}

// The place rule of SPEC 3.8 for a page text with no type: (field, font size of a centered text).
fn page_text_place(words: Node, page_height: f64) -> (&'static str, Option<f64>) {
    let y: f64 = words
        .attribute("default-y")
        .and_then(|y| y.parse().ok())
        .unwrap_or(0.0);
    if y < page_height / 2.0 {
        return ("rights", None);
    }
    match words.attribute("justify").or(words.attribute("halign")) {
        Some("right") => ("composer", None),
        Some("center") => (
            "title",
            Some(
                words
                    .attribute("font-size")
                    .and_then(|f| f.parse().ok())
                    .unwrap_or(0.0),
            ),
        ),
        _ => ("lyricist", None),
    }
}

// One text for each header field, in the order of the import: the first stays, a movement
// title replaces a work title, and page texts with no type come last.
fn header(doc: &Document) -> BTreeSet<String> {
    let root = doc.root_element();
    let mut fields: Vec<(&'static str, String)> = Vec::new();
    let mut page_texts: Vec<((&'static str, Option<f64>), String)> = Vec::new();
    let page_height: f64 = root
        .descendants()
        .find(|n| n.has_tag_name("page-height"))
        .and_then(|n| n.text())
        .and_then(|t| t.parse().ok())
        .unwrap_or(0.0);
    for c in root.children().filter(|n| n.is_element()) {
        match c.tag_name().name() {
            "work" => {
                for w in c.children().filter(|n| n.is_element()) {
                    match w.tag_name().name() {
                        "work-title" => push_field(&mut fields, "title", &own_text(w), false),
                        "work-number" => push_field(&mut fields, "subtitle", &own_text(w), false),
                        _ => {}
                    }
                }
            }
            "movement-title" => push_field(&mut fields, "title", &own_text(c), true),
            "movement-number" => push_field(&mut fields, "subtitle", &own_text(c), true),
            "identification" => {
                for i in c.children().filter(|n| n.is_element()) {
                    let field = match (i.tag_name().name(), i.attribute("type")) {
                        ("creator", Some("composer")) => "composer",
                        ("creator", Some("arranger")) => "arranger",
                        ("creator", Some("lyricist" | "poet")) => "lyricist",
                        ("rights", _) => "rights",
                        _ => continue,
                    };
                    push_field(&mut fields, field, &own_text(i), false);
                }
            }
            "credit" => {
                let words: Vec<String> = c
                    .children()
                    .filter(|n| n.has_tag_name("credit-words"))
                    .map(|w| own_text(w).trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect();
                let joined = words.join(
                    "
",
                );
                let field = match text(c, "credit-type").as_str() {
                    "title" => "title",
                    "subtitle" => "subtitle",
                    "composer" => "composer",
                    "arranger" => "arranger",
                    "lyricist" => "lyricist",
                    "rights" => "rights",
                    "page number" | "part name" => continue,
                    _ if fields.iter().any(|(_, t)| *t == joined) => continue,
                    _ => {
                        if let Some(w) = child(c, "credit-words") {
                            page_texts.push((page_text_place(w, page_height), joined));
                        }
                        continue;
                    }
                };
                push_field(&mut fields, field, &joined, false);
            }
            _ => {}
        }
    }
    let largest = page_texts
        .iter()
        .filter_map(|((_, size), _)| *size)
        .fold(f64::MIN, f64::max);
    let mut joined: Vec<(&'static str, Vec<String>)> = Vec::new();
    for ((field, size), t) in page_texts {
        let field = match size {
            Some(s) if s < largest => "subtitle",
            _ => field,
        };
        match joined.iter_mut().find(|(f, _)| *f == field) {
            Some((_, texts)) => texts.push(t),
            None => joined.push((field, vec![t])),
        }
    }
    for (field, texts) in joined {
        push_field(
            &mut fields,
            field,
            &texts.join(
                "
",
            ),
            false,
        );
    }
    let mut out: BTreeSet<String> = fields
        .into_iter()
        .map(|(f, t)| format!("{f}={t}"))
        .collect();
    if let Some(mi) = root
        .descendants()
        .find(|n| n.has_tag_name("midi-instrument"))
    {
        let program = text(mi, "midi-program");
        if !program.is_empty() && program != "1" {
            out.insert(format!("program={program}"));
        }
        for tag in ["volume", "pan"] {
            let v: f64 = text(mi, tag).parse().unwrap_or(0.0);
            let default = if tag == "volume" {
                100.0 / 127.0 * 100.0
            } else {
                0.0
            };
            if !text(mi, tag).is_empty() && (v - default).abs() > 0.01 {
                out.insert(format!("{tag}={v}"));
            }
        }
    }
    out
}

fn is_lone_pedal_stop(direction: Node) -> bool {
    let marks: Vec<Node> = direction
        .children()
        .filter(|c| c.has_tag_name("direction-type"))
        .flat_map(|t| t.children().filter(Node::is_element))
        .collect();
    let [mark] = marks.as_slice() else {
        return false;
    };
    mark.has_tag_name("pedal") && mark.attribute("type") == Some("stop")
}

pub fn content(xml: &str, detail: Detail) -> Content {
    let doc = parse(xml);
    let rules = Rules {
        detail,
        span_ends: span_ends(&doc),
        pedal_types: pedal_types(&doc),
    };
    let mut level = Level {
        carried: DEFAULT_LEVEL,
    };
    let mut state: BTreeMap<String, String> = BTreeMap::new();
    let mut div = 1;
    let mut measures: Vec<Vec<String>> = Vec::new();
    let mut prev_end = String::new();
    for m in doc.descendants().filter(|n| n.has_tag_name("measure")) {
        div = divisions(m).unwrap_or(div);
        let times = note_times(m, div);
        let marks = level.marks(m);
        let ranks = voice_ranks(m);
        let mut items = Vec::new();
        let (mut pos, mut last, mut end) = (0i64, 0i64, 0i64);
        let mut small = false;
        for e in m.children().filter(Node::is_element) {
            let tag = e.tag_name().name();
            let at = |p: i64| frac(p, div * 4);
            match tag {
                "attributes" => {
                    for k in e.children().filter(Node::is_element) {
                        let name = k.tag_name().name();
                        // 4.1: page setup and engraving style.
                        if matches!(name, "divisions" | "staves" | "staff-details") {
                            continue;
                        }
                        let key = format!("{name}{}", k.attribute("number").unwrap_or(""));
                        let c = rules.canon(k, tag);
                        if state.get(&key) != Some(&c) {
                            items.push(format!("{} attr {c}", at(pos)));
                            state.insert(key, c);
                        }
                    }
                }
                "backup" => pos -= int(e, "duration"),
                "forward" => pos += int(e, "duration"),
                "note" => {
                    let chord = child(e, "chord").is_some();
                    let d = int(e, "duration");
                    // 4.1: `<cue/>` is a small note that sounds; pno sets the size for each chord.
                    // 4.1: the size of a note that is not drawn.
                    if !chord {
                        let cue = child(e, "cue").is_some()
                            || child(e, "type").and_then(|t| t.attribute("size")) == Some("cue");
                        small = cue && e.attribute("print-object") != Some("no");
                    }
                    let start = if chord { pos - last } else { pos };
                    if !chord {
                        pos += d;
                        last = d;
                    }
                    // 4.1: a hidden rest and a forward are the same empty place.
                    let hidden =
                        e.attribute("print-object") == Some("no") && child(e, "rest").is_some();
                    if hidden {
                        continue;
                    }
                    let (staff, rank) = ranks.get(&text(e, "voice")).cloned().unwrap_or_default();
                    let lvl = level.at(&marks, start);
                    let vel = e
                        .attribute("dynamics")
                        .and_then(|v| v.parse::<f64>().ok())
                        .filter(|v| (v - lvl).abs() > 0.01)
                        .map(|v| format!(" vel={v:.2}"))
                        .unwrap_or_default();
                    let size = if small { " small" } else { "" };
                    let (when, length) = times
                        .get(&e.id())
                        .map_or((at(start), at(d)), |t| (q_text(t.0), q_text(t.1)));
                    items.push(format!(
                        "{when} s{staff}v{rank} dur={length}{vel}{size} {}",
                        rules.canon(e, tag)
                    ));
                }
                "direction" => {
                    let t = pos + int(e, "offset");
                    let jump = child(e, "sound").is_some_and(|s| {
                        ["dacapo", "dalsegno", "fine", "tocoda"]
                            .iter()
                            .any(|a| s.attribute(*a).is_some())
                    });
                    let mut c = rules.canon(e, tag);
                    let marks = ["<direction-type", "<sound"];
                    if !marks.iter().any(|m| c.contains(m)) {
                        continue;
                    }
                    if let Some(v) = direction_level(e) {
                        c.push_str(&format!(" level={v:.2}"));
                    }
                    if jump {
                        // 4.1: the words of jump texts.
                        c = c.split("<words >").next().unwrap_or("").to_string();
                    }
                    // 4.1: a pedal stop at the start of a bar is at the end of the bar before.
                    if t == 0 && is_lone_pedal_stop(e) {
                        if let Some(prev) = measures.last_mut() {
                            prev.push(format!("{prev_end} dir {c}"));
                            prev.sort();
                            continue;
                        }
                    }
                    items.push(format!("{} dir {c}", at(t)));
                }
                "barline" => {
                    let mut c = rules.canon(e, tag);
                    for n in e.descendants().filter(|n| n.has_tag_name("ending")) {
                        // 4.1: the words of volta texts.
                        if let Some(t) = n.text().map(str::trim).filter(|t| !t.is_empty()) {
                            c = c.replace(&format!(">{t}</>"), "></>");
                        }
                    }
                    items.push(format!("barline {c}"));
                }
                "print" => {
                    for a in ["new-system", "new-page"] {
                        if e.attribute(a) == Some("yes") {
                            items.push(format!("print {a}"));
                        }
                    }
                }
                "sound" => items.push(format!("{} {}", at(pos), rules.canon(e, tag))),
                // 4.1: chord symbols.
                "harmony" => {}
                other => items.push(format!("{} unknown {other}", at(pos))),
            }
            end = end.max(pos);
        }
        prev_end = frac(end, div * 4);
        level.carried = level.at(&marks, i64::MAX);
        items.sort();
        measures.push(items);
    }
    Content {
        header: header(&doc),
        measures,
    }
}

// The measures at the same index with the same content, in any order.
pub fn equal_measures(want: &Content, got: &Content) -> usize {
    let sorted = |m: &Vec<String>| {
        let mut m = m.clone();
        m.sort();
        m
    };
    want.measures
        .iter()
        .zip(&got.measures)
        .filter(|(w, g)| sorted(w) == sorted(g))
        .count()
}

pub fn differences(want: &Content, got: &Content) -> Vec<String> {
    let mut out = Vec::new();
    for h in want.header.symmetric_difference(&got.header) {
        out.push(format!("header: {h}"));
    }
    if want.measures.len() != got.measures.len() {
        out.push(format!(
            "{} measures, expected {}",
            got.measures.len(),
            want.measures.len()
        ));
    }
    for (i, (w, g)) in want.measures.iter().zip(&got.measures).enumerate() {
        for item in w.iter().filter(|x| !g.contains(x)) {
            out.push(format!("measure index {i}: missing {item}"));
        }
        for item in g.iter().filter(|x| !w.contains(x)) {
            out.push(format!("measure index {i}: extra {item}"));
        }
    }
    out
}
