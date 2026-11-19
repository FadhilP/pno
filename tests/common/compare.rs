use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

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
const DROPPED_ATTRS: [&str; 23] = [
    "color",
    "end-length",
    "text-x",
    "text-y",
    "spread",
    "bezier-x",
    "bezier-y",
    "bezier-x2",
    "bezier-y2",
    "bezier-offset",
    "bezier-offset2",
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
const DEFAULT_LEVEL: f64 = 68.0 / 90.0 * 100.0;
const P_LEVEL: f64 = 48.0 / 90.0 * 100.0;

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
        ("strong-accent", "type", "up")
            | ("tuplet", "number", "1")
            | ("clef", "number", "1")
            | ("metronome", "parentheses", "no")
            // A clef at the start of a measure is drawn after the barline.
            | ("clef", "after-barline", "yes")
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

// 4.1: a text, tempo or hairpin with no placement is at its default place.
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
    } else if marks == ["wedge"] {
        "below"
    } else if marks.iter().all(|m| *m == "words") && !marks.is_empty() {
        if lower { "below" } else { "above" }
    } else {
        return None;
    };
    Some(format!("placement={place}"))
}

struct Rules {
    detail: Detail,
    span_ends: HashMap<NodeId, String>,
    pedal_types: HashMap<NodeId, &'static str>,
    grace_slash: HashMap<NodeId, bool>,
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
    len: Q,
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
            let (start, len) = match e.tag_name().name() {
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
                    times
                        .get(&e.id())
                        .copied()
                        .unwrap_or((q(start, div * 4), (0, 1)))
                }
                "direction" => (q(pos + int(e, "offset"), div * 4), (0, 1)),
                _ => continue,
            };
            out.push(Timed {
                order: out.len(),
                measure,
                start,
                len,
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
    at: (usize, Q),
    node: Node<'a, 'a>,
}

const SPANS: [&str; 7] = [
    "slur",
    "wedge",
    "dashes",
    "octave-shift",
    "glissando",
    "slide",
    "wavy-line",
];

// Span numbers are labels that the import may change: each end is named by the place of the other end.
fn span_ends(doc: &Document) -> HashMap<NodeId, String> {
    let mut marks: Vec<SpanMark> = Vec::new();
    let all = timed(doc);
    let last = all.iter().map(|t| t.measure).max().unwrap_or(0);
    let score_end = all
        .iter()
        .filter(|t| t.measure == last)
        .map(|t| q_add(t.start, t.len))
        .fold((0, 1), |a, b| if a.0 * b.1 < b.0 * a.1 { b } else { a });
    for t in all {
        let place = format!("{}@{}", t.measure, q_text(t.start));
        for s in t
            .node
            .descendants()
            .filter(|s| SPANS.contains(&s.tag_name().name()))
        {
            let Some(kind) = s.attribute("type").filter(|t| *t != "continue") else {
                continue;
            };
            // A trill line ends where the note with its stop ends, on any voice of the staff.
            let trill_end = kind == "stop" && s.has_tag_name("wavy-line");
            let place = if trill_end {
                format!("{}@{}", t.measure, q_text(q_add(t.start, t.len)))
            } else {
                place.clone()
            };
            marks.push(SpanMark {
                order: (t.order, kind != "stop"),
                place,
                at: (t.measure, t.start),
                node: s,
            });
        }
    }
    marks.sort_by_key(|m| m.order);
    // A stop closes the last open span of its type and number. As the import: a slur stop with no
    // open start waits for a start of its number earlier in its measure.
    let mut open: HashMap<(&str, &str), Vec<(String, NodeId)>> = HashMap::new();
    type EarlyStop = (String, (usize, Q), NodeId);
    let mut early_stops: HashMap<(&str, &str), Vec<EarlyStop>> = HashMap::new();
    let mut ends = HashMap::new();
    let before = |a: (usize, Q), b: (usize, Q)| a.0 == b.0 && a.1.0 * b.1.1 < b.1.0 * a.1.1;
    for SpanMark {
        order: (_, is_start),
        place,
        at,
        node: s,
    } in marks
    {
        let key = (s.tag_name().name(), s.attribute("number").unwrap_or("1"));
        if is_start {
            let waiting = early_stops.get_mut(&key).and_then(|stops| {
                let i = stops.iter().position(|(_, end, _)| before(at, *end))?;
                Some(stops.remove(i))
            });
            match waiting {
                Some((to, _, id)) => {
                    ends.insert(s.id(), format!("to={to}"));
                    ends.insert(id, format!("from={place}"));
                }
                None => open.entry(key).or_default().push((place, s.id())),
            }
        } else if let Some((from, id)) = open.get_mut(&key).and_then(Vec::pop) {
            ends.insert(id, format!("to={place}"));
            ends.insert(s.id(), format!("from={from}"));
        } else if key.0 == "slur" {
            early_stops
                .entry(key)
                .or_default()
                .push((place, at, s.id()));
        }
    }
    // As the import: a hairpin, dashed line or ottava with no end ends at the end of the score.
    let end_place = format!("{last}@{}", q_text(score_end));
    for ((tag, _), starts) in open {
        if matches!(tag, "wedge" | "dashes" | "octave-shift") {
            for (_, id) in starts {
                ends.insert(id, format!("to={end_place}"));
            }
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
            // 4.1: page setup and engraving style (the naturals that cancel the key before).
            ("key", "cancel") => true,
            _ if presentation => self.detail == Detail::NoStemsBeams,
            // 4.1: the vertical position of rests.
            ("rest", "display-step" | "display-octave") => true,
            // 4.1: the voice numbers of the source; the offset is in the time.
            ("direction", "voice" | "offset") => true,
            // 4.1: the unit note value of a tuplet; the lengths of its notes are compared.
            ("time-modification", "normal-type" | "normal-dot") => true,
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
            "number" => parent == "key",
            // An after-grace is a grace after its note; pno marks it with the time it takes.
            "steal-time-previous" => parent == "grace",
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
        // 4.1: a playback tempo that differs from a shown metronome mark; the mark is compared.
        let shown_metronome = |d: Node| {
            d.descendants()
                .any(|m| m.has_tag_name("metronome") && m.attribute("print-object") != Some("no"))
        };
        if tag == "sound" && n.parent().is_some_and(shown_metronome) {
            attrs.retain(|a| !a.starts_with("tempo="));
        }
        if tag == "wavy-line" && n.attribute("type") == Some("stop") {
            return String::new();
        }
        // A slur with no partner is an error in the source; the import drops it with a report row.
        if tag == "slur" && !self.span_ends.contains_key(&n.id()) {
            return String::new();
        }
        if let Some(slash) = self.grace_slash.get(&n.id()) {
            attrs.retain(|a| !a.starts_with("slash="));
            if *slash {
                attrs.push("slash=yes".into());
            }
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
        // The end of a line is drawn where its start is. 4.1: pno places pedal marks and jump texts.
        let has_words = n.descendants().any(|d| d.has_tag_name("words"));
        let fixed_place = is_jump(n) || (has_pedal(n) && !has_words);
        if tag == "direction" && (only_span_stops(n) || fixed_place) {
            attrs.retain(|a| !a.starts_with("placement="));
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
        let mut kids: Vec<String> = n
            .children()
            .filter(Node::is_element)
            .filter(|c| !self.skip_child(tag, c.tag_name().name()))
            .map(|c| self.canon(c, tag))
            .filter(|c| !c.is_empty())
            .collect();
        if tag == "direction" {
            // 4.1: the staff of a pedal mark or jump text; pno writes each pedal mark below the lower
            // staff and each jump text above the upper staff.
            let pedal = has_pedal(n);
            if pedal || is_jump(n) {
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
        if empty && matches!(tag, "sound" | "direction-type" | "notations" | "ornaments") {
            return String::new();
        }
        if tag == "note" {
            kids.retain(|k| k != "<staff >1</>");
            for kind in tie_types(n) {
                let tie = format!("<tie type={kind}></>");
                if kids.contains(&tie) {
                    continue;
                }
                let before = |k: &String| {
                    k.starts_with("<type") || (kind == "stop" && k.starts_with("<tie type=start"))
                };
                let at = kids.iter().position(before).unwrap_or(kids.len());
                kids.insert(at, tie);
            }
        }
        if tag == "articulations" {
            // `<detached-legato>` is the staccato and the tenuto sign in one element.
            if let Some(i) = kids.iter().position(|k| k == "<detached-legato ></>") {
                kids[i] = "<staccato ></>".into();
                kids.push("<tenuto ></>".into());
            }
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
        // A normal notehead is the default notehead.
        if tag == "notehead" && body == "normal" && attrs.is_empty() {
            return String::new();
        }
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

// Of the dynamic levels at one time on one staff, only the last one in document order sounds:
// each direction that shares its time, with the last one of its time.
// 4.1: of two marks at one time, the lasting one sets the level.
fn shared_levels(m: Node) -> HashMap<NodeId, NodeId> {
    let mut groups: BTreeMap<(i64, String), Vec<(NodeId, bool)>> = BTreeMap::new();
    let mut pos = 0i64;
    for e in m.children().filter(Node::is_element) {
        match e.tag_name().name() {
            "backup" => pos -= int(e, "duration"),
            "forward" => pos += int(e, "duration"),
            "note" if child(e, "chord").is_none() => pos += int(e, "duration"),
            "direction" if direction_level(e).is_some() => {
                let key = (pos + int(e, "offset"), text(e, "staff"));
                let momentary = dynamics_name(e).is_some_and(|n| MOMENTARY.contains(&n));
                groups.entry(key).or_default().push((e.id(), momentary));
            }
            _ => {}
        }
    }
    let mut out = HashMap::new();
    for ids in groups.values().filter(|ids| ids.len() > 1) {
        let lasting = ids.iter().rev().find(|(_, momentary)| !momentary);
        let (end, _) = *lasting.unwrap_or(&ids[ids.len() - 1]);
        for (id, _) in ids {
            out.insert(*id, end);
        }
    }
    out
}

// 4.1: the velocity of a second mark with the same name in one measure; pno keeps the first one.
fn repeated_marks(m: Node) -> HashSet<NodeId> {
    let mut marks: Vec<(i64, NodeId, &str)> = Vec::new();
    let mut pos = 0i64;
    for e in m.children().filter(Node::is_element) {
        match e.tag_name().name() {
            "backup" => pos -= int(e, "duration"),
            "forward" => pos += int(e, "duration"),
            "note" if child(e, "chord").is_none() => pos += int(e, "duration"),
            "direction" => {
                if let Some(name) = dynamics_name(e) {
                    marks.push((pos + int(e, "offset"), e.id(), name));
                }
            }
            _ => {}
        }
    }
    marks.sort_by_key(|(t, ..)| *t);
    let mut seen = HashSet::new();
    marks
        .into_iter()
        .filter(|(_, _, name)| !seen.insert(*name))
        .map(|(_, id, _)| id)
        .collect()
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
        "pppp" => 16.0,
        "ppp" => 26.0,
        "pp" => 38.0,
        "p" => 48.0,
        "mp" => 58.0,
        "mf" => 68.0,
        "f" | "fp" => 80.0,
        "ff" | "sf" | "sfz" | "sfp" | "fz" | "rf" | "rfz" => 92.0,
        "fff" | "sffz" => 106.0,
        "ffff" => 120.0,
        _ => return None,
    };
    Some(velocity / 90.0 * 100.0)
}

impl Level {
    fn marks(&self, m: Node, repeated: &HashSet<NodeId>) -> Vec<(i64, f64, Option<&'static str>)> {
        let mut pos = 0i64;
        let mut out = Vec::new();
        for e in m.children().filter(Node::is_element) {
            match e.tag_name().name() {
                "backup" => pos -= int(e, "duration"),
                "forward" => pos += int(e, "duration"),
                "note" if child(e, "chord").is_none() => pos += int(e, "duration"),
                "direction" if !repeated.contains(&e.id()) => {
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

// A note with no staff is on staff 1.
fn staff_of(n: Node) -> String {
    let s = text(n, "staff");
    if s.is_empty() { "1".into() } else { s }
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
            .entry(staff_of(n))
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

fn pitch_key(note: Node) -> Option<String> {
    let p = child(note, "pitch")?;
    Some(format!(
        "{}{}{}",
        text(p, "step"),
        text(p, "alter"),
        text(p, "octave")
    ))
}

fn tie_starts(note: Node) -> bool {
    tie_types(note).contains(&"start")
}

fn tie_stops(note: Node) -> bool {
    tie_types(note).contains(&"stop")
}

// 4.1: pno plays a tie that is only drawn (`<tied>` with no `<tie>`).
fn tie_types<'a>(note: Node<'a, 'a>) -> Vec<&'a str> {
    let mut kinds: Vec<&str> = note
        .descendants()
        .filter(|c| c.has_tag_name("tie") || c.has_tag_name("tied"))
        .filter_map(|c| c.attribute("type"))
        .filter(|k| matches!(*k, "start" | "stop"))
        .collect();
    kinds.sort_unstable();
    kinds.dedup();
    kinds
}

fn without_tie(canon: String, kind: &str) -> String {
    let mut c = canon.replace(&format!("<tie type={kind}></>"), "");
    let end = format!("type={kind}></>");
    let mut from = 0;
    while let Some(at) = c[from..].find("<tied ").map(|i| from + i) {
        let close = at + c[at..].find("</>").map_or(0, |i| i + 3);
        if c[at..close].ends_with(&end) {
            c.replace_range(at..close, "");
        } else {
            from = close.max(at + 1);
        }
    }
    c.replace("<notations ></>", "")
}

// 4.1 (an error in the source): a tie with no note of the same pitch at its other end. A tied
// grace note ends on its main note.
fn lone_ties(doc: &Document) -> HashMap<NodeId, Vec<&'static str>> {
    let notes: Vec<Timed> = timed(doc)
        .into_iter()
        .filter(|t| t.node.has_tag_name("note"))
        .collect();
    let mut offsets = Vec::new();
    for t in &notes {
        if offsets.len() <= t.measure {
            offsets.resize(t.measure + 1, (0, 1));
        }
        let end = q_add(t.start, t.len);
        if q_less(offsets[t.measure], end) {
            offsets[t.measure] = end;
        }
    }
    let mut at = (0, 1);
    for o in offsets.iter_mut() {
        let len = *o;
        *o = at;
        at = q_add(at, len);
    }
    let start = |t: &Timed| q_add(offsets[t.measure], t.start);
    let end = |t: &Timed| q_add(start(t), t.len);
    let is_grace = |t: &Timed| child(t.node, "grace").is_some();
    let mut begins = HashSet::new();
    let mut stopped_begins = HashSet::new();
    let mut tie_ends: HashMap<_, Vec<NodeId>> = HashMap::new();
    for t in &notes {
        let Some(p) = pitch_key(t.node) else { continue };
        if !is_grace(t) {
            begins.insert((p.clone(), start(t)));
            if tie_stops(t.node) {
                stopped_begins.insert((p.clone(), start(t)));
            }
        }
        if tie_starts(t.node) {
            tie_ends.entry((p, end(t))).or_default().push(t.node.id());
        }
    }
    let mut lone: HashMap<NodeId, Vec<&str>> = HashMap::new();
    for t in &notes {
        let Some(p) = pitch_key(t.node) else { continue };
        for kind in tie_types(t.node) {
            let other_end = match kind {
                // As the import: a tie ends on a note with a tie stop; MuseScore 3 writes none on the
                // main note of a tied grace note.
                "start" if is_grace(t) => begins.contains(&(p.clone(), end(t))),
                "start" => stopped_begins.contains(&(p.clone(), end(t))),
                // A grace note ends where it starts: its own tie start is not the start of its stop.
                _ => tie_ends
                    .get(&(p.clone(), start(t)))
                    .is_some_and(|ids| ids.iter().any(|id| *id != t.node.id())),
            };
            if !other_end {
                let kind = if kind == "start" { "start" } else { "stop" };
                lone.entry(t.node.id()).or_default().push(kind);
            }
        }
    }
    lone
}

fn only_span_stops(direction: Node) -> bool {
    let mut marks = direction
        .children()
        .filter(|c| c.has_tag_name("direction-type"))
        .flat_map(|t| t.children().filter(Node::is_element))
        .peekable();
    marks.peek().is_some()
        && marks.all(|m| {
            matches!(m.tag_name().name(), "wedge" | "dashes" | "octave-shift")
                && m.attribute("type") == Some("stop")
        })
}

fn has_pedal(direction: Node) -> bool {
    direction.descendants().any(|d| d.has_tag_name("pedal"))
}

fn is_jump(direction: Node) -> bool {
    let jump_attrs = ["dacapo", "dalsegno", "fine", "tocoda"];
    child(direction, "sound").is_some_and(|s| jump_attrs.iter().any(|a| s.has_attribute(*a)))
}

// 4.1: a group of grace notes has the slash of its first note.
fn grace_slash(doc: &Document) -> HashMap<NodeId, bool> {
    let mut out = HashMap::new();
    for m in doc.descendants().filter(|n| n.has_tag_name("measure")) {
        let mut groups: HashMap<String, bool> = HashMap::new();
        for note in m.children().filter(|n| n.has_tag_name("note")) {
            let voice = text(note, "voice");
            let Some(grace) = child(note, "grace") else {
                if child(note, "chord").is_none() {
                    groups.remove(&voice);
                }
                continue;
            };
            let own = grace.attribute("slash") == Some("yes");
            let slash = *groups.entry(voice).or_insert(own);
            out.insert(grace.id(), slash);
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
        grace_slash: grace_slash(&doc),
    };
    let lone = lone_ties(&doc);
    // `level` leaves out the repeated marks, as pno does; `sounding` is the level the source plays.
    let mut level = Level {
        carried: DEFAULT_LEVEL,
    };
    let mut sounding = Level {
        carried: DEFAULT_LEVEL,
    };
    let none = HashSet::new();
    let mut state: BTreeMap<String, String> = BTreeMap::new();
    let mut div = 1;
    let mut measures: Vec<Vec<String>> = Vec::new();
    let mut prev_end = String::new();
    let mut grace_ties: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut main_ties: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for m in doc.descendants().filter(|n| n.has_tag_name("measure")) {
        div = divisions(m).unwrap_or(div);
        let times = note_times(m, div);
        let repeated = repeated_marks(m);
        let mut marks = level.marks(m, &repeated);
        // 4.1: of two marks at one time, pno keeps the lasting one as the level.
        marks.sort_by_key(|x| (x.0, x.2.is_none()));
        let all_marks = sounding.marks(m, &none);
        let ranks = voice_ranks(m);
        let shared = shared_levels(m);
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
                        // pno has one key for both staves.
                        let staff = k.attribute("number").filter(|_| name != "key");
                        let key = format!("{name}{}", staff.unwrap_or(""));
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
                    let voice = text(e, "voice");
                    let grace = child(e, "grace").is_some();
                    if grace && tie_starts(e) {
                        grace_ties
                            .entry(voice.clone())
                            .or_default()
                            .extend(pitch_key(e));
                    }
                    if !grace && !chord {
                        let pending = grace_ties.remove(&voice).unwrap_or_default();
                        main_ties.insert(voice.clone(), pending);
                    }
                    let after_tied_grace = !grace
                        && pitch_key(e)
                            .is_some_and(|p| main_ties.get(&voice).is_some_and(|t| t.contains(&p)));
                    let (staff, rank) = ranks.get(&voice).cloned().unwrap_or_default();
                    let lvl = level.at(&marks, start);
                    let vel = e
                        .attribute("dynamics")
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or_else(|| sounding.at(&all_marks, start));
                    let is_rest = child(e, "rest").is_some();
                    let vel = Some(vel)
                        .filter(|v| !is_rest && (v - lvl).abs() > 0.01)
                        .map(|v| format!(" vel={v:.2}"))
                        .unwrap_or_default();
                    let size = if small { " small" } else { "" };
                    let (when, length) = times
                        .get(&e.id())
                        .map_or((at(start), at(d)), |t| (q_text(t.0), q_text(t.1)));
                    let mut c = rules.canon(e, tag);
                    // MuseScore 3 writes no tie stop on the main note of a tied grace note; pno writes one.
                    if after_tied_grace {
                        c = without_tie(c, "stop");
                    }
                    for kind in lone.get(&e.id()).into_iter().flatten() {
                        c = without_tie(c, kind);
                    }
                    items.push(format!(
                        "{when} s{staff}v{rank} dur={length}{vel}{size} {c}"
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
                    let only_sound = !c.contains("<direction-type");
                    if let (true, Some(s), None) =
                        (only_sound, child(e, "sound"), direction_level(e))
                    {
                        // A direction with only a sound is the same as a sound in the measure.
                        items.push(format!("{} {}", at(t), rules.canon(s, tag)));
                        continue;
                    }
                    let level = direction_level(e).filter(|_| !repeated.contains(&e.id()));
                    match (level, shared.get(&e.id())) {
                        (Some(v), None) => c.push_str(&format!(" level={v:.2}")),
                        (Some(v), Some(end)) if *end == e.id() => {
                            items.push(format!("{} level s{} {v:.2}", at(t), text(e, "staff")));
                        }
                        _ => {}
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
        sounding.carried = sounding.at(&all_marks, i64::MAX);
        items.sort();
        measures.push(items);
    }
    // A pedal, hairpin, dashed line or ottava that is open at the end of the score ends there.
    if let Some(items) = measures.last_mut() {
        let at_end = format!("{prev_end} dir ");
        let lines = ["<pedal ", "<wedge ", "<dashes ", "<octave-shift "];
        items.retain(|i| {
            let line_stop = lines.iter().any(|l| i.contains(l))
                && i.contains("type=stop")
                && !i.contains("type=start");
            !(i.starts_with(&at_end) && line_stop)
        });
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
