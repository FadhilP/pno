use roxmltree::Node;

use super::report::{Report, Row};
use crate::frac::{Frac, NoteValue};
use crate::model::{
    AccDisplay, ArpKind, Articulation, Attr, BarStyle, BracketEnd, Break, Clef, Dyn, DynMark,
    DynText, GlissLine, Hairpin, Jump, Key, MetaField, Meter, Metronome, Mode, OrnamentKind,
    Ottava, PedalStyle, PhraseMark, Pitch, PitchMark, Place, Sign, Staff, Tempo, TempoMark, Text,
    TextFrame, TimeSymbol, TokMark, TupletLook,
};

const POSITION_ATTRS: [&str; 12] = [
    "default-x",
    "default-y",
    "relative-x",
    "relative-y",
    "width",
    "bezier-x",
    "bezier-y",
    "bezier-x2",
    "bezier-y2",
    "bezier-offset",
    "bezier-offset2",
    "spread",
];
const FONT_ATTRS: [&str; 10] = [
    "font-family",
    "font-size",
    "font-style",
    "font-weight",
    "justify",
    "halign",
    "valign",
    "line-height",
    "lang",
    "space",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Start,
    Stop,
}

#[derive(Clone, Debug)]
pub struct SrcGrace {
    pub slash: bool,
    pub after: bool,
    pub value: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SrcHead {
    Pitch(Pitch),
    Rest,
    BarRest,
    Hidden,
}

#[derive(Clone, Copy, Debug)]
pub struct SrcSlur {
    pub edge: Edge,
    pub number: u32,
    pub place: Option<Place>,
    // MusicXML readers pair slur numbers in document order.
    pub note: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct SrcTuplet {
    pub edge: Edge,
    pub number: u32,
    pub look: TupletLook,
    pub ratio: Option<(u32, u32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeamState {
    Begin,
    Continue,
    End,
    ForwardHook,
    BackwardHook,
}

#[derive(Clone, Debug)]
pub struct SrcNote {
    pub voice: String,
    pub hidden: bool,
    pub staff: Staff,
    pub start: Frac,
    pub dur: Frac,
    pub chord: bool,
    pub grace: Option<SrcGrace>,
    pub head: SrcHead,
    pub written: Option<NoteValue>,
    pub time_mod: Option<(u32, u32)>,
    pub tie_start: bool,
    pub tie_stop: bool,
    pub acc: Option<AccDisplay>,
    pub pitch_marks: Vec<PitchMark>,
    pub tok_marks: Vec<TokMark>,
    pub slurs: Vec<SrcSlur>,
    pub tuplets: Vec<SrcTuplet>,
    pub tremolo: Option<(Edge, u8)>,
    pub beams: Vec<(u8, BeamState)>,
    pub stem: Option<TokMark>,
    pub velocity: Option<f64>,
    pub small: bool,
    pub pause: Option<PhraseMark>,
    pub trill_stop: bool,
    pub gliss_starts: Vec<u32>,
    pub gliss_stops: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PedalKind {
    Start,
    Stop,
    Change,
}

#[derive(Clone, Debug)]
pub enum DirKind {
    Dyn {
        mark: Dyn,
        velocity: f64,
    },
    DynText(DynText),
    Wedge {
        start: Option<Hairpin>,
        number: u32,
        place: Option<Place>,
    },
    Text {
        text: Text,
        dashes: Option<u32>,
    },
    Tempo {
        tempo: Tempo,
        dashes: Option<u32>,
    },
    DashesStop {
        number: u32,
    },
    Pedal(PedalKind, PedalStyle),
    Octave {
        start: Option<Ottava>,
        number: u32,
    },
    Trill(Edge),
}

impl DirKind {
    pub fn sets_level(&self) -> bool {
        match self {
            DirKind::Dyn { mark, .. } => !mark.mark.is_momentary(),
            DirKind::DynText(t) => t.level.is_some(),
            _ => false,
        }
    }

    pub fn is_span_stop(&self) -> bool {
        matches!(
            self,
            DirKind::Wedge { start: None, .. }
                | DirKind::DashesStop { .. }
                | DirKind::Octave { start: None, .. }
                | DirKind::Trill(Edge::Stop)
        )
    }
}

#[derive(Clone, Debug)]
pub struct SrcDir {
    pub t: Frac,
    pub staff: Staff,
    pub kind: DirKind,
}

#[derive(Clone, Debug, Default)]
pub struct SrcMeasure {
    pub number: String,
    pub attrs: Vec<Attr>,
    pub key: Option<Key>,
    pub meter: Option<Meter>,
    pub clefs: Vec<(Frac, Staff, Clef)>,
    pub notes: Vec<SrcNote>,
    pub dirs: Vec<SrcDir>,
    pub end: Frac,
    // A tuplet note in this measure has a length that the divisions round.
    pub rounded: bool,
    pub pedal_style: Option<PedalStyle>,
}

#[derive(Debug, Default)]
pub struct Source {
    pub meta: Vec<(MetaField, String)>,
    page_height: Option<f64>,
    page_texts: Vec<(TextPlace, String)>,
    pub vel_lines: Vec<String>,
    // (bar index, time in the bar, mark)
    pub phrase_marks: Vec<(usize, Frac, PhraseMark)>,
    pub program: Option<u32>,
    pub volume: Option<f64>,
    pub pan: Option<f64>,
    pub accidentals_stated: bool,
    pub one_staff: bool,
    pub measures: Vec<SrcMeasure>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TextPlace {
    Lower,
    Left,
    Right,
    Center { font_size: f64 },
}

fn text_place(words: Node, page_height: Option<f64>) -> Option<TextPlace> {
    let y: f64 = words.attribute("default-y")?.parse().ok()?;
    if y < page_height? / 2.0 {
        return Some(TextPlace::Lower);
    }
    let side = words.attribute("justify").or(words.attribute("halign"));
    Some(match side {
        Some("right") => TextPlace::Right,
        Some("center") => TextPlace::Center {
            font_size: words
                .attribute("font-size")
                .and_then(|f| f.parse().ok())
                .unwrap_or(0.0),
        },
        _ => TextPlace::Left,
    })
}

// A page text with no type goes to the field of its place on the first page (SPEC 3.8).
fn place_page_texts(src: &mut Source, rep: &mut Report) {
    let largest = src
        .page_texts
        .iter()
        .filter_map(|(p, _)| match p {
            TextPlace::Center { font_size } => Some(*font_size),
            _ => None,
        })
        .fold(f64::MIN, f64::max);
    let field_of = |p: TextPlace| match p {
        TextPlace::Lower => MetaField::Copyright,
        TextPlace::Right => MetaField::Composer,
        TextPlace::Left => MetaField::Lyricist,
        TextPlace::Center { font_size } if font_size >= largest => MetaField::Title,
        TextPlace::Center { .. } => MetaField::Subtitle,
    };
    let mut joined: Vec<(MetaField, Vec<String>)> = Vec::new();
    for (place, text) in std::mem::take(&mut src.page_texts) {
        let f = field_of(place);
        rep.note(format!(
            "the page text \"{}\" goes to `{}` by its place",
            text.replace('\n', " "),
            f.name()
        ));
        match joined.iter_mut().find(|(g, _)| *g == f) {
            Some((_, texts)) => texts.push(text),
            None => joined.push((f, vec![text])),
        }
    }
    for (f, texts) in joined {
        push_meta(src, rep, f, texts.join("\n"));
    }
}

fn phrase_field(text: &str) -> Option<(usize, Frac, PhraseMark)> {
    let mut parts = text.split_whitespace();
    let bar = parts.next()?.parse().ok()?;
    let t = parts.next()?;
    let (num, den) = t.split_once('/').unwrap_or((t, "1"));
    let num: i64 = num.parse().ok()?;
    let den: i64 = den.parse().ok().filter(|d: &i64| *d > 0)?;
    let mark = PhraseMark::parse(parts.next()?).filter(|m| !m.is_drawn())?;
    parts
        .next()
        .is_none()
        .then_some((bar, Frac::new(num, den), mark))
}

fn vel_field_number(f: Node) -> Option<u32> {
    f.attribute("name")?.strip_prefix("pno-vel-")?.parse().ok()
}

pub fn stop(what: &str, measure: &str) -> String {
    if measure.is_empty() {
        format!("the import stops: {what} is not supported")
    } else {
        format!("the import stops at measure {measure}: {what} is not supported")
    }
}

fn kids<'a, 'i>(n: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children().filter(Node::is_element)
}

fn child<'a, 'i>(n: Node<'a, 'i>, tag: &str) -> Option<Node<'a, 'i>> {
    kids(n).find(|c| c.has_tag_name(tag))
}

fn text_of(n: Node) -> String {
    n.text().unwrap_or("").trim().to_string()
}

fn child_text(n: Node, tag: &str) -> Option<String> {
    child(n, tag).map(text_of)
}

fn number<T: std::str::FromStr>(n: Node, what: &str, m: &str) -> Result<T, String> {
    text_of(n)
        .parse()
        .map_err(|_| stop(&format!("the value `{}` of <{what}>", text_of(n)), m))
}

fn yes(n: Node, attr: &str) -> bool {
    n.attribute(attr) == Some("yes")
}

struct Reader<'r> {
    rep: &'r mut Report,
    measure: String,
}

impl Reader<'_> {
    fn stop(&self, what: &str) -> String {
        stop(what, &self.measure)
    }

    fn unknown(&self, n: Node, parent: &str) -> String {
        self.stop(&format!("<{}> in <{parent}>", n.tag_name().name()))
    }

    fn attrs(&mut self, n: Node, handled: &[&str]) -> Result<(), String> {
        for a in n.attributes() {
            let name = a.name();
            if handled.contains(&name) {
                continue;
            }
            if POSITION_ATTRS.contains(&name) {
                self.rep.drop(Row::Position);
            } else if FONT_ATTRS.contains(&name) {
                self.rep.drop(Row::Font);
            } else if name == "color" {
                self.rep.drop(Row::Color);
            } else if name != "id" {
                return Err(self.stop(&format!(
                    "the attribute {name}=\"{}\" of <{}>",
                    a.value(),
                    n.tag_name().name()
                )));
            }
        }
        Ok(())
    }

    fn only(&mut self, n: Node, handled: &[&str]) -> Result<(), String> {
        self.attrs(n, handled)?;
        if let Some(c) = kids(n).next() {
            return Err(self.unknown(c, n.tag_name().name()));
        }
        Ok(())
    }
}

pub fn read(root: Node, rep: &mut Report) -> Result<Source, String> {
    if !root.has_tag_name("score-partwise") {
        return Err(stop(
            &format!("a <{}> file (only score-partwise)", root.tag_name().name()),
            "",
        ));
    }
    let mut src = Source {
        accidentals_stated: true,
        ..Source::default()
    };
    let mut r = Reader {
        rep,
        measure: String::new(),
    };
    let mut parts = Vec::new();
    for c in kids(root) {
        match c.tag_name().name() {
            "work" => {
                for w in kids(c) {
                    match w.tag_name().name() {
                        "work-title" => push_meta(&mut src, r.rep, MetaField::Title, text_of(w)),
                        "work-number" => {
                            push_meta(&mut src, r.rep, MetaField::Subtitle, text_of(w));
                        }
                        _ => r.rep.drop(Row::Metadata),
                    }
                }
            }
            "movement-title" => set_meta(&mut src, r.rep, MetaField::Title, text_of(c)),
            "movement-number" => set_meta(&mut src, r.rep, MetaField::Subtitle, text_of(c)),
            "identification" => identification(&mut r, c, &mut src)?,
            "defaults" => {
                src.page_height = child(c, "page-layout")
                    .and_then(|l| child_text(l, "page-height"))
                    .and_then(|h| h.parse().ok());
                r.rep.drop(Row::PageSetup);
            }
            "credit" => credit(&mut r, c, &mut src)?,
            "part-list" => part_list(&mut r, c, &mut src)?,
            "part" => parts.push(c),
            _ => return Err(r.unknown(c, "score-partwise")),
        }
    }
    place_page_texts(&mut src, r.rep);
    let [part] = parts.as_slice() else {
        return Err(stop(&format!("a score with {} parts", parts.len()), ""));
    };
    let mut state = PartState::default();
    for m in kids(*part) {
        if !m.has_tag_name("measure") {
            return Err(r.unknown(m, "part"));
        }
        let measure = read_measure(&mut r, m, &mut state)?;
        src.measures.push(measure);
    }
    src.one_staff = !state.two_staves;
    if state.volta.is_some() {
        return Err(stop("a volta bracket with no end", ""));
    }
    trill_lines(&mut src.measures)?;
    glissandi_end_on_the_next_chord(&src.measures)?;
    Ok(src)
}

// A glissando ends on the next chord of its voice, as SPEC 3.7 places it.
fn glissandi_end_on_the_next_chord(measures: &[SrcMeasure]) -> Result<(), String> {
    let mut chords: std::collections::BTreeMap<&str, Vec<(&str, Vec<&SrcNote>)>> =
        std::collections::BTreeMap::new();
    for m in measures {
        for n in m.notes.iter().filter(|n| n.grace.is_none()) {
            let list = chords.entry(n.voice.as_str()).or_default();
            match list.last_mut() {
                Some((_, notes)) if n.chord => notes.push(n),
                _ => list.push((m.number.as_str(), vec![n])),
            }
        }
    }
    let midi = |n: &&SrcNote| match &n.head {
        SrcHead::Pitch(p) => p.midi(),
        _ => i32::MIN,
    };
    let bad = |number: &str| {
        stop(
            "a glissando that does not end on the next chord of its voice",
            number,
        )
    };
    for list in chords.values_mut() {
        for (_, notes) in list.iter_mut() {
            notes.sort_by_key(midi);
        }
        let mut expected: Vec<Vec<u32>> = vec![Vec::new(); list.first().map_or(0, |c| c.1.len())];
        for (ci, (number, notes)) in list.iter().enumerate() {
            let stops: Vec<&[u32]> = notes.iter().map(|n| n.gliss_stops.as_slice()).collect();
            let has_stops = stops.iter().any(|s| !s.is_empty());
            let wanted = expected.iter().any(|s| !s.is_empty());
            let other_numbers = has_stops && !same_numbers(&stops, &expected);
            if has_stops != wanted || other_numbers {
                return Err(bad(number));
            }
            let starts: Vec<(u32, i32)> = notes
                .iter()
                .flat_map(|n| n.gliss_starts.iter().map(move |s| (*s, midi(n))))
                .collect();
            let next: &[&SrcNote] = list.get(ci + 1).map_or(&[], |c| &c.1);
            expected = vec![Vec::new(); next.len()];
            match (starts.as_slice(), next.len()) {
                ([], _) => {}
                (_, 0) => return Err(bad(number)),
                ([(s, from)], n) => {
                    let up = midi(&next[n - 1]) >= *from;
                    expected[if up { n - 1 } else { 0 }].push(*s);
                }
                (_, n) if n == starts.len() => {
                    for (slot, (s, _)) in expected.iter_mut().zip(starts) {
                        slot.push(s);
                    }
                }
                _ => return Err(bad(number)),
            }
        }
    }
    Ok(())
}

fn same_numbers(a: &[&[u32]], b: &[Vec<u32>]) -> bool {
    let sorted = |v: &[u32]| {
        let mut v = v.to_vec();
        v.sort_unstable();
        v
    };
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| sorted(x) == sorted(y))
}

// A trill line that covers its note and the notes tied to it is `:trl` (SPEC 3.7). Each other one is
// a span of the `trl` lane, from its first note to the end of the note that carries its stop, and
// its first note keeps the trill sign.
fn trill_lines(measures: &mut [SrcMeasure]) -> Result<(), String> {
    let is_line = |t: &TokMark| {
        matches!(
            t,
            TokMark::Ornament {
                kind: OrnamentKind::TrillLine,
                ..
            }
        )
    };
    let mut open: std::collections::BTreeMap<Staff, (usize, usize)> =
        std::collections::BTreeMap::new();
    let mut lane_lines = Vec::new();
    for (mi, m) in measures.iter().enumerate() {
        for (ni, n) in m.notes.iter().enumerate() {
            let starts = n.tok_marks.iter().any(is_line);
            if n.trill_stop {
                match open.remove(&n.staff) {
                    Some(at) => {
                        let first = &measures[at.0].notes[at.1];
                        let tied_on = n.tie_stop && n.voice == first.voice && n.head == first.head;
                        if at != (mi, ni) && !tied_on {
                            lane_lines.push((at, (mi, ni)));
                        }
                    }
                    // A line that starts and stops on this note covers only this note.
                    None if starts => {}
                    None => return Err(stop("a trill line end with no start", &m.number)),
                }
            }
            if starts && !n.trill_stop {
                if open.contains_key(&n.staff) {
                    return Err(stop("two trill lines on one staff", &m.number));
                }
                open.insert(n.staff, (mi, ni));
            }
        }
    }
    if let Some((m, _)) = open.values().next() {
        return Err(stop("a trill line with no end", &measures[*m].number));
    }
    for ((sm, sn), (em, en)) in lane_lines {
        let first = &mut measures[sm].notes[sn];
        for t in &mut first.tok_marks {
            if let TokMark::Ornament { kind, .. } = t {
                if *kind == OrnamentKind::TrillLine {
                    *kind = OrnamentKind::Trill;
                }
            }
        }
        let (staff, t) = (first.staff, first.start);
        measures[sm].dirs.push(SrcDir {
            t,
            staff,
            kind: DirKind::Trill(Edge::Start),
        });
        let last = &measures[em].notes[en];
        let end = last.start + last.dur;
        measures[em].dirs.push(SrcDir {
            t: end,
            staff,
            kind: DirKind::Trill(Edge::Stop),
        });
    }
    Ok(())
}

// `pno` has one text for each header field; the report counts a second, different text.
fn set_meta(src: &mut Source, rep: &mut Report, f: MetaField, text: String) {
    if text.is_empty() {
        return;
    }
    if src.meta.iter().any(|(g, t)| *g == f && *t != text) {
        rep.drop(Row::SecondHeaderText);
    }
    src.meta.retain(|(g, _)| *g != f);
    src.meta.push((f, text));
}

fn push_meta(src: &mut Source, rep: &mut Report, f: MetaField, text: String) {
    if text.is_empty() {
        return;
    }
    match src.meta.iter().find(|(g, _)| *g == f) {
        Some((_, t)) if *t != text => rep.drop(Row::SecondHeaderText),
        Some(_) => {}
        None => src.meta.push((f, text)),
    }
}

fn identification(r: &mut Reader, n: Node, src: &mut Source) -> Result<(), String> {
    for c in kids(n) {
        match c.tag_name().name() {
            "creator" => {
                let field = match c.attribute("type") {
                    Some("composer") => MetaField::Composer,
                    Some("arranger") => MetaField::Arranger,
                    Some("lyricist" | "poet") => MetaField::Lyricist,
                    _ => {
                        r.rep.drop(Row::Metadata);
                        continue;
                    }
                };
                push_meta(src, r.rep, field, text_of(c));
            }
            "rights" => push_meta(src, r.rep, MetaField::Copyright, text_of(c)),
            "encoding" => {
                for e in kids(c).filter(|e| e.has_tag_name("supports")) {
                    if e.attribute("element") == Some("accidental") {
                        src.accidentals_stated = e.attribute("type") == Some("yes");
                    }
                }
                r.rep.drop(Row::Metadata);
            }
            "miscellaneous" => {
                let mut lines: Vec<(u32, String)> = Vec::new();
                for f in kids(c) {
                    let is_phrase = f
                        .attribute("name")
                        .is_some_and(|n| n.starts_with("pno-phrase-"));
                    if is_phrase {
                        match phrase_field(&text_of(f)) {
                            Some(m) => src.phrase_marks.push(m),
                            None => r.rep.drop(Row::Metadata),
                        }
                        continue;
                    }
                    match vel_field_number(f) {
                        Some(n) => lines.push((n, text_of(f))),
                        None => r.rep.drop(Row::Metadata),
                    }
                }
                // MuseScore writes the fields in the order of their names.
                lines.sort_by_key(|(n, _)| *n);
                src.vel_lines.extend(lines.into_iter().map(|(_, t)| t));
            }
            "source" | "relation" => r.rep.drop(Row::Metadata),
            _ => return Err(r.unknown(c, "identification")),
        }
    }
    Ok(())
}

fn credit(r: &mut Reader, n: Node, src: &mut Source) -> Result<(), String> {
    let kind = child_text(n, "credit-type");
    let words: Vec<String> = kids(n)
        .filter(|c| c.has_tag_name("credit-words"))
        .map(text_of)
        .filter(|t| !t.is_empty())
        .collect();
    let text = words.join("\n");
    let field = match kind.as_deref() {
        Some("title") => Some(MetaField::Title),
        Some("subtitle") => Some(MetaField::Subtitle),
        Some("composer") => Some(MetaField::Composer),
        Some("arranger") => Some(MetaField::Arranger),
        Some("lyricist") => Some(MetaField::Lyricist),
        Some("rights") => Some(MetaField::Copyright),
        Some("page number" | "part name") | None if text.is_empty() => None,
        Some("page number" | "part name") => {
            r.rep.drop(Row::PageSetup);
            return Ok(());
        }
        _ => None,
    };
    match field {
        Some(f) => push_meta(src, r.rep, f, text),
        None if text.is_empty() => {}
        None if src.meta.iter().any(|(_, t)| *t == text) => r.rep.drop(Row::CreditCopy),
        None => {
            let place = (n.attribute("page").unwrap_or("1") == "1")
                .then(|| child(n, "credit-words"))
                .flatten()
                .and_then(|w| text_place(w, src.page_height))
                .ok_or_else(|| {
                    stop(
                        &format!("a page text with no type, not placed on page 1 (\"{text}\")"),
                        "",
                    )
                })?;
            src.page_texts.push((place, text));
        }
    }
    Ok(())
}

fn part_list(r: &mut Reader, n: Node, src: &mut Source) -> Result<(), String> {
    for c in kids(n) {
        match c.tag_name().name() {
            "score-part" => {
                for p in kids(c) {
                    match p.tag_name().name() {
                        "part-name"
                        | "part-abbreviation"
                        | "part-name-display"
                        | "part-abbreviation-display"
                        | "group" => r.rep.drop(Row::PageSetup),
                        "score-instrument" | "midi-device" => r.rep.drop(Row::Metadata),
                        "midi-instrument" => {
                            for mi in kids(p) {
                                match mi.tag_name().name() {
                                    "midi-program" => {
                                        src.program = Some(number(mi, "midi-program", "")?);
                                    }
                                    "volume" => src.volume = Some(number(mi, "volume", "")?),
                                    "pan" => src.pan = Some(number(mi, "pan", "")?),
                                    _ => r.rep.drop(Row::Metadata),
                                }
                            }
                        }
                        _ => return Err(r.unknown(p, "score-part")),
                    }
                }
            }
            "part-group" => r.rep.drop(Row::PageSetup),
            _ => return Err(r.unknown(c, "part-list")),
        }
    }
    Ok(())
}

#[derive(Default)]
struct PartState {
    div: i64,
    volta: Option<Vec<u32>>,
    two_staves: bool,
}

struct Cursor {
    pos: i64,
    last_start: i64,
    end: Frac,
    // The exact end of the note before. After rounded tuplet notes the divisions, and the
    // <forward> and <backup> elements that MuseScore adds to correct them, can be off by up to
    // one division for each rounded note (`slack`).
    exact_end: Option<Frac>,
    slack: i64,
    last_exact_start: Frac,
    rounded: bool,
    rounded_notes: i64,
}

fn read_measure(r: &mut Reader, m: Node, state: &mut PartState) -> Result<SrcMeasure, String> {
    let label = m.attribute("number").unwrap_or("").to_string();
    r.measure.clone_from(&label);
    r.attrs(m, &["number", "implicit"])?;
    let mut out = SrcMeasure {
        number: label,
        ..SrcMeasure::default()
    };
    if let Some(v) = &state.volta {
        out.attrs.push(Attr::Volta {
            numbers: v.clone(),
            end: None,
        });
    }
    let mut cur = Cursor {
        pos: 0,
        last_start: 0,
        end: Frac::ZERO,
        exact_end: None,
        slack: 0,
        last_exact_start: Frac::ZERO,
        rounded: false,
        rounded_notes: 0,
    };
    let mut repeat_end = None;
    let mut ending_stop: Option<BracketEnd> = None;
    let mut last_cue_size = false;
    for e in kids(m) {
        let t = |pos: i64| Frac::new(pos, 4 * state.div.max(1));
        match e.tag_name().name() {
            "attributes" => attributes(r, e, t(cur.pos), state, &mut out)?,
            "note" => {
                // `:small` is on the token, so all notes of a chord have one size.
                let cue_size = child(e, "type").and_then(|t| t.attribute("size")) == Some("cue");
                if child(e, "chord").is_some() && cue_size != last_cue_size {
                    return Err(r.stop("a chord with notes of different sizes"));
                }
                last_cue_size = cue_size;
                let note = read_note(r, e, state.div, &mut cur)?;
                out.notes.push(note);
            }
            "backup" | "forward" => {
                let d: i64 = child(e, "duration")
                    .map(|d| number(d, "duration", &r.measure))
                    .transpose()?
                    .unwrap_or(0);
                for c in kids(e) {
                    if !matches!(c.tag_name().name(), "duration" | "voice" | "staff") {
                        return Err(r.unknown(c, e.tag_name().name()));
                    }
                }
                cur.pos += if e.has_tag_name("backup") { -d } else { d };
                // A backup to the start of the measure after rounded tuplet notes can go past it
                // by up to one division for each of them.
                if cur.pos < 0 && -cur.pos <= cur.rounded_notes {
                    cur.pos = 0;
                }
                if e.has_tag_name("backup") {
                    cur.rounded_notes = 0;
                }
                if cur.pos < 0 {
                    return Err(r.stop("a <backup> before the start of the measure"));
                }
            }
            "direction" => direction(r, e, state.div, cur.pos, &mut out)?,
            "barline" => barline(r, e, state, &mut out, &mut repeat_end, &mut ending_stop)?,
            "print" => {
                r.attrs(
                    e,
                    &[
                        "new-system",
                        "new-page",
                        "page-number",
                        "blank-page",
                        "staff-spacing",
                    ],
                )?;
                if e.attribute("blank-page").is_some() {
                    return Err(r.stop("a blank page"));
                }
                if yes(e, "new-page") {
                    out.attrs.push(Attr::Break(Break::Page));
                } else if yes(e, "new-system") {
                    out.attrs.push(Attr::Break(Break::System));
                }
                if kids(e).next().is_some() {
                    r.rep.drop(Row::PageSetup);
                }
            }
            "sound" => {
                if e.has_attribute("dynamics") {
                    return Err(r.stop("a playback dynamic level with no mark"));
                }
                r.only(e, &["tempo"])?;
                let Some(tempo) = e.attribute("tempo") else {
                    continue;
                };
                let bpm: f64 = tempo
                    .parse()
                    .map_err(|_| r.stop(&format!("the tempo `{tempo}`")))?;
                out.dirs.push(SrcDir {
                    t: t(cur.pos),
                    staff: Staff::Upper,
                    kind: DirKind::Tempo {
                        tempo: Tempo {
                            text: None,
                            mark: Some(TempoMark::Sound(bpm)),
                            ext: false,
                            place: None,
                        },
                        dashes: None,
                    },
                });
            }
            "harmony" => r.rep.drop(Row::ChordSymbols),
            "figured-bass" if kids(e).next().is_none() => {}
            _ => return Err(r.unknown(e, "measure")),
        }
        let at = exact_start(&cur, 4 * state.div.max(1));
        cur.end = cur.end.max(at);
    }
    out.end = cur.end;
    out.rounded = cur.rounded;
    if cur.rounded {
        r.rep.drop(Row::RoundedTuplets);
    }
    // MuseScore 2 and 3 write a bar rest as a rest with no type that fills the measure.
    for n in &mut out.notes {
        let fills = n.start.is_zero() && n.dur == out.end;
        if matches!(n.head, SrcHead::Rest) && n.written.is_none() && fills {
            n.head = SrcHead::BarRest;
        }
    }
    if let Some(stop_end) = ending_stop {
        let default_end = if repeat_end.is_some() {
            BracketEnd::Closed
        } else {
            BracketEnd::Open
        };
        let end = (stop_end != default_end).then_some(stop_end);
        for a in &mut out.attrs {
            if let Attr::Volta { end: e, .. } = a {
                *e = end;
            }
        }
    }
    if let Some(times) = repeat_end {
        merge_repeat(&mut out.attrs, RepeatSide::End(times));
    }
    Ok(out)
}

#[derive(Clone, Copy)]
enum RepeatSide {
    Start,
    End(u32),
}

fn merge_repeat(attrs: &mut Vec<Attr>, side: RepeatSide) {
    let (start, end) = match side {
        RepeatSide::Start => (true, None),
        RepeatSide::End(times) => (false, Some(times)),
    };
    for a in attrs.iter_mut() {
        if let Attr::Repeat { start: s, end: e } = a {
            *s |= start;
            *e = e.or(end);
            return;
        }
    }
    attrs.push(Attr::Repeat { start, end });
}

fn attributes(
    r: &mut Reader,
    e: Node,
    t: Frac,
    state: &mut PartState,
    out: &mut SrcMeasure,
) -> Result<(), String> {
    r.attrs(e, &[])?;
    let mid = !t.is_zero();
    let mut keys: Vec<Key> = Vec::new();
    let mut times: Vec<Meter> = Vec::new();
    for c in kids(e) {
        match c.tag_name().name() {
            "divisions" => state.div = number(c, "divisions", &r.measure)?,
            "key" => keys.push(key(r, c)?),
            "time" => times.push(time(r, c)?),
            "staves" => {
                let n: u32 = number(c, "staves", &r.measure)?;
                if n != 2 {
                    return Err(r.stop(&format!("a part with {n} staves")));
                }
                state.two_staves = true;
            }
            "clef" => {
                let (staff, clef) = clef(r, c)?;
                out.clefs.push((t, staff, clef));
            }
            "part-symbol" | "staff-details" => {
                let lines = child_text(c, "staff-lines").filter(|l| l != "5");
                if let Some(l) = lines {
                    return Err(r.stop(&format!("a staff with {l} lines")));
                }
                r.rep.drop(Row::PageSetup);
            }
            "instruments" => {}
            "transpose" => return Err(r.stop("a transposing part")),
            "measure-style" => {
                let kind = kids(c).next().map_or("", |k| k.tag_name().name());
                let what = match kind {
                    "multiple-rest" => "a multi-bar rest".to_string(),
                    k => format!("the measure style <{k}>"),
                };
                return Err(r.stop(&what));
            }
            _ => return Err(r.unknown(c, "attributes")),
        }
    }
    if let Some(k) = keys.first() {
        if keys.iter().any(|x| x != k) {
            return Err(r.stop("different key signatures on the two staves"));
        }
        if mid {
            return Err(r.stop("a key change inside a measure"));
        }
        out.key = Some(*k);
    }
    if let Some(m) = times.first() {
        if times.iter().any(|x| x != m) {
            return Err(r.stop("different time signatures on the two staves"));
        }
        if mid {
            return Err(r.stop("a time signature change inside a measure"));
        }
        out.meter = Some(m.clone());
    }
    Ok(())
}

fn key(r: &mut Reader, c: Node) -> Result<Key, String> {
    r.attrs(c, &["number"])?;
    if c.attribute("print-object") == Some("no") {
        return Err(r.stop("a hidden key signature"));
    }
    let mut k = Key {
        fifths: 0,
        mode: None,
    };
    for x in kids(c) {
        match x.tag_name().name() {
            "fifths" => k.fifths = number(x, "fifths", &r.measure)?,
            "mode" => {
                k.mode = match text_of(x).as_str() {
                    "major" => Some(Mode::Major),
                    "minor" => Some(Mode::Minor),
                    "none" | "" => None,
                    other => return Err(r.stop(&format!("the key mode {other}"))),
                }
            }
            "cancel" => r.rep.drop(Row::PageSetup),
            _ => return Err(r.stop("a key signature that is not standard")),
        }
    }
    if !(-7..=7).contains(&k.fifths) {
        return Err(r.stop("a key signature that is not standard"));
    }
    Ok(k)
}

fn time(r: &mut Reader, c: Node) -> Result<Meter, String> {
    if c.attribute("print-object") == Some("no") {
        return Err(r.stop("a hidden time signature"));
    }
    r.attrs(c, &["number", "symbol"])?;
    let symbol = match c.attribute("symbol") {
        Some("common") => Some(TimeSymbol::Common),
        Some("cut") => Some(TimeSymbol::Cut),
        None | Some("normal") => None,
        Some(s) => return Err(r.stop(&format!("the time signature symbol {s}"))),
    };
    let mut beats = Vec::new();
    let mut beat_type = Vec::new();
    for x in kids(c) {
        match x.tag_name().name() {
            "beats" => beats.push(text_of(x)),
            "beat-type" => beat_type.push(text_of(x)),
            "senza-misura" => return Err(r.stop("a bar with no time signature (senza misura)")),
            _ => return Err(r.unknown(x, "time")),
        }
    }
    let ([b], [bt]) = (beats.as_slice(), beat_type.as_slice()) else {
        return Err(r.stop("a time signature with more than one fraction"));
    };
    let parts: Result<Vec<u32>, _> = b.split('+').map(str::parse).collect();
    let parts = parts.map_err(|_| r.stop(&format!("the time signature {b}/{bt}")))?;
    let beat_type: u32 = bt
        .parse()
        .map_err(|_| r.stop(&format!("the time signature {b}/{bt}")))?;
    let meter = Meter {
        beats: parts,
        beat_type,
        pattern: None,
        symbol,
    };
    Ok(meter)
}

fn clef(r: &mut Reader, c: Node) -> Result<(Staff, Clef), String> {
    r.attrs(c, &["number", "after-barline"])?;
    if c.attribute("print-object") == Some("no") {
        return Err(r.stop("a hidden clef"));
    }
    let staff = staff_of(r, c.attribute("number").unwrap_or("1"))?;
    let sign = child_text(c, "sign").unwrap_or_default();
    let line = child_text(c, "line").unwrap_or_default();
    let octave = child_text(c, "clef-octave-change").unwrap_or_default();
    for x in kids(c) {
        if !matches!(x.tag_name().name(), "sign" | "line" | "clef-octave-change") {
            return Err(r.unknown(x, "clef"));
        }
    }
    let clef = match (sign.as_str(), line.as_str(), octave.as_str()) {
        ("G", "2" | "", "" | "0") => Clef::Treble,
        ("F", "4" | "", "" | "0") => Clef::Bass,
        ("C", "3", "" | "0") => Clef::Alto,
        ("C", "4", "" | "0") => Clef::Tenor,
        ("G", "2" | "", "1") => Clef::Treble8va,
        ("G", "2" | "", "-1") => Clef::Treble8vb,
        ("F", "4" | "", "1") => Clef::Bass8va,
        ("F", "4" | "", "-1") => Clef::Bass8vb,
        (_, _, "" | "0") => return Err(r.stop(&format!("the clef {sign}{line}"))),
        _ => {
            return Err(r.stop(&format!(
                "the clef {sign}{line} with the octave number {octave}"
            )));
        }
    };
    Ok((staff, clef))
}

fn staff_of(r: &Reader, s: &str) -> Result<Staff, String> {
    match s.trim() {
        "1" => Ok(Staff::Upper),
        "2" => Ok(Staff::Lower),
        other => Err(r.stop(&format!("staff {other}"))),
    }
}

fn note_value(name: &str) -> Option<i32> {
    Some(match name {
        "breve" => 1,
        "whole" => 0,
        "half" => -1,
        "quarter" => -2,
        "eighth" => -3,
        "16th" => -4,
        "32nd" => -5,
        "64th" => -6,
        "128th" => -7,
        "256th" => -8,
        _ => return None,
    })
}

fn alter_of(name: &str) -> Option<i8> {
    Some(match name {
        "sharp" => 1,
        "flat" => -1,
        "natural" => 0,
        "double-sharp" | "sharp-sharp" => 2,
        "flat-flat" | "double-flat" => -2,
        _ => return None,
    })
}

// MusicXML durations are whole divisions, so a tuplet such as 7:4 can be off by less than one.
fn exact_length(
    written: Option<NoteValue>,
    time_mod: Option<(u32, u32)>,
    ticks: Frac,
    whole: i64,
) -> Option<Frac> {
    let (actual, normal) = time_mod?;
    let exact = written?.frac() * Frac::new(i64::from(normal), i64::from(actual).max(1));
    let one = Frac::new(1, whole);
    let near = exact < ticks + one && ticks < exact + one;
    near.then_some(exact)
}

fn exact_start(cur: &Cursor, whole: i64) -> Frac {
    let at = Frac::new(cur.pos, whole);
    let slack = Frac::new(cur.slack, whole);
    match cur.exact_end {
        Some(end) if end < at + slack && at < end + slack => end,
        _ => at,
    }
}

// MuseScore can correct the rounding with the <backup> before a tuplet, so the first rounded
// note starts on a multiple of its plain written value, up to one division for each note.
fn tuplet_start(pos: i64, written: NoteValue, actual: i64, whole: i64) -> Frac {
    let at = Frac::new(pos, whole);
    let unit = NoteValue { dots: 0, ..written }.frac() * whole;
    if !unit.is_int() || unit.is_zero() {
        return at;
    }
    let u = unit.num();
    let nearest = (pos + u / 2) / u * u;
    if (nearest - pos).abs() > actual {
        return at;
    }
    Frac::new(nearest, whole)
}

fn read_note(r: &mut Reader, e: Node, div: i64, cur: &mut Cursor) -> Result<SrcNote, String> {
    if e.attribute("print-dot") == Some("no") {
        return Err(r.stop("a dot that is not shown"));
    }
    r.attrs(
        e,
        &["dynamics", "attack", "release", "print-object", "print-dot"],
    )?;
    let hidden = e.attribute("print-object") == Some("no");
    let mut n = SrcNote {
        voice: "1".into(),
        hidden: false,
        staff: Staff::Upper,
        start: Frac::ZERO,
        dur: Frac::ZERO,
        chord: false,
        grace: None,
        head: SrcHead::Rest,
        written: None,
        time_mod: None,
        tie_start: false,
        tie_stop: false,
        acc: None,
        pitch_marks: Vec::new(),
        tok_marks: Vec::new(),
        slurs: Vec::new(),
        tuplets: Vec::new(),
        tremolo: None,
        beams: Vec::new(),
        stem: None,
        velocity: None,
        small: false,
        pause: None,
        trill_stop: false,
        gliss_starts: Vec::new(),
        gliss_stops: Vec::new(),
    };
    let mut tie_start_played = false;
    let mut tie_stop_played = false;
    if let Some(v) = e.attribute("dynamics") {
        let pct: f64 = v
            .parse()
            .map_err(|_| r.stop(&format!("dynamics=\"{v}\"")))?;
        let velocity = velocity_of(pct);
        let silent = velocity == 0.0;
        if !silent && !(1.0..=127.0).contains(&velocity) {
            return Err(r.stop("a note velocity that is not 0 to 127"));
        }
        n.velocity = Some(velocity);
    }
    let marks = [
        ("attack", PitchMark::Attack as fn(f64) -> PitchMark),
        ("release", PitchMark::Release),
    ];
    for (attr, mark) in marks {
        if let Some(v) = e.attribute(attr) {
            let d: f64 = v.parse().map_err(|_| r.stop(&format!("{attr}=\"{v}\"")))?;
            let divisions = u32::try_from(div.max(1))
                .map_err(|_| r.stop(&format!("{attr}=\"{v}\" with {div} divisions")))?;
            let ticks = d * 480.0 / f64::from(divisions);
            n.pitch_marks.push(mark((ticks * 100.0).round() / 100.0));
        }
    }
    let mut dur_ticks = 0i64;
    let mut dots = 0u8;
    let mut log2 = None;
    for c in kids(e) {
        match c.tag_name().name() {
            "grace" => {
                r.attrs(
                    c,
                    &[
                        "slash",
                        "steal-time-previous",
                        "steal-time-following",
                        "make-time",
                    ],
                )?;
                if c.attribute("steal-time-following").is_some()
                    || c.attribute("make-time").is_some()
                {
                    return Err(r.stop("a grace note that takes time from the next note"));
                }
                n.grace = Some(SrcGrace {
                    slash: yes(c, "slash"),
                    after: c.attribute("steal-time-previous").is_some(),
                    value: 8,
                });
            }
            "chord" => n.chord = true,
            "cue" => {
                r.rep.drop(Row::CueElement);
                n.small = true;
            }
            "pitch" => n.head = SrcHead::Pitch(pitch(r, c)?),
            "unpitched" => return Err(r.stop("an unpitched note")),
            "rest" => {
                r.attrs(c, &["measure"])?;
                n.head = if yes(c, "measure") {
                    SrcHead::BarRest
                } else {
                    SrcHead::Rest
                };
                if kids(c).next().is_some() {
                    r.rep.drop(Row::RestPosition);
                }
            }
            "duration" => dur_ticks = number(c, "duration", &r.measure)?,
            "tie" => match c.attribute("type") {
                Some("start") => {
                    n.tie_start = true;
                    tie_start_played = true;
                }
                Some("stop") => {
                    n.tie_stop = true;
                    tie_stop_played = true;
                }
                _ => {}
            },
            "instrument" => {}
            "voice" => n.voice = text_of(c),
            "type" => {
                r.attrs(c, &["size"])?;
                match c.attribute("size") {
                    Some("cue") => n.small = true,
                    None | Some("full") => {}
                    Some(s) => return Err(r.stop(&format!("a note of size {s}"))),
                }
                let name = text_of(c);
                log2 = Some(
                    note_value(&name).ok_or_else(|| r.stop(&format!("the note value {name}")))?,
                );
            }
            "dot" => {
                r.attrs(c, &["placement"])?;
                dots += 1;
            }
            "accidental" => {
                r.attrs(
                    c,
                    &["parentheses", "cautionary", "editorial", "bracket", "size"],
                )?;
                if yes(c, "bracket") {
                    return Err(r.stop("an accidental sign in square brackets"));
                }
                if yes(c, "editorial") {
                    return Err(r.stop("an editorial accidental"));
                }
                let name = text_of(c);
                if alter_of(&name).is_none() {
                    return Err(r.stop(&format!("the accidental {name}")));
                }
                n.acc = Some(if yes(c, "parentheses") {
                    AccDisplay::Paren
                } else {
                    AccDisplay::Force
                });
            }
            "time-modification" => {
                let actual: u32 = child(c, "actual-notes")
                    .map(|x| number(x, "actual-notes", &r.measure))
                    .transpose()?
                    .unwrap_or(1);
                let normal: u32 = child(c, "normal-notes")
                    .map(|x| number(x, "normal-notes", &r.measure))
                    .transpose()?
                    .unwrap_or(1);
                n.time_mod = Some((actual, normal));
                let dotted = |x: Node, dot: &str| kids(x).filter(|k| k.has_tag_name(dot)).count();
                let unit = child(c, "normal-type").map(|u| (text_of(u), dotted(c, "normal-dot")));
                let own = child(e, "type").map(|t| (text_of(t), dotted(e, "dot")));
                if unit.is_some() && unit != own {
                    r.rep.drop(Row::TupletUnit);
                }
            }
            "stem" => {
                r.attrs(c, &[])?;
                n.stem = match text_of(c).as_str() {
                    "up" => Some(TokMark::StemUp),
                    "down" => Some(TokMark::StemDown),
                    _ => None,
                };
            }
            "notehead" => {
                r.attrs(c, &["filled", "parentheses"])?;
                if yes(c, "parentheses") {
                    return Err(r.stop("parentheses on a notehead"));
                }
                let shape = text_of(c);
                if shape != "normal" {
                    return Err(r.stop(&format!("the notehead shape {shape}")));
                }
                let hollow_value = matches!(
                    child_text(e, "type").as_deref(),
                    Some("half" | "whole" | "breve" | "long" | "maxima")
                );
                let filled = match c.attribute("filled") {
                    Some(v) => v == "yes",
                    None => !hollow_value,
                };
                if filled == hollow_value {
                    return Err(r.stop("a notehead fill that is not the fill of its note value"));
                }
            }
            "staff" => n.staff = staff_of(r, &text_of(c))?,
            "beam" => {
                r.attrs(c, &["number", "repeater", "fan"])?;
                let level: u8 = c.attribute("number").unwrap_or("1").parse().unwrap_or(1);
                let state = match text_of(c).as_str() {
                    "begin" => BeamState::Begin,
                    "continue" => BeamState::Continue,
                    "end" => BeamState::End,
                    "forward hook" => BeamState::ForwardHook,
                    "backward hook" => BeamState::BackwardHook,
                    other => return Err(r.stop(&format!("the beam value {other}"))),
                };
                n.beams.push((level, state));
            }
            "notations" => notations(r, c, &mut n)?,
            "lyric" => return Err(r.stop("lyrics")),
            "notehead-text" => return Err(r.stop("text in a notehead")),
            "play" | "listen" => {
                return Err(r.stop(&format!("<{}> on a note", c.tag_name().name())));
            }
            "footnote" | "level" => return Err(r.stop("an editorial footnote")),
            _ => return Err(r.unknown(c, "note")),
        }
    }
    let start_not_played = n.tie_start && !tie_start_played;
    let stop_not_played = n.tie_stop && !tie_stop_played;
    if start_not_played || stop_not_played {
        r.rep.drop(Row::TiedNotPlayed);
    }
    match (hidden, &n.head) {
        (false, _) => {}
        (true, SrcHead::Rest | SrcHead::BarRest) => n.head = SrcHead::Hidden,
        (true, _) => {
            n.hidden = true;
            if n.small {
                n.small = false;
                r.rep.drop(Row::HiddenSize);
            }
        }
    }
    if let Some(g) = &mut n.grace {
        if dots > 0 {
            return Err(r.stop("a dotted grace note"));
        }
        let l = log2.unwrap_or(-3);
        g.value = 1u32 << (-l).clamp(0, 30);
        n.start = Frac::new(cur.pos, 4 * div.max(1));
    } else {
        let whole = 4 * div.max(1);
        let ticks = Frac::new(dur_ticks, whole);
        n.written = log2.map(|l| NoteValue { log2: l, dots });
        n.dur = exact_length(n.written, n.time_mod, ticks, whole).unwrap_or(ticks);
        n.start = if n.chord {
            cur.last_exact_start
        } else {
            let start = exact_start(cur, whole);
            let rounded = n.dur != ticks;
            let alone = start == Frac::new(cur.pos, whole);
            match n.written.zip(n.time_mod) {
                Some((w, (actual, _))) if rounded && alone => {
                    tuplet_start(cur.pos, w, i64::from(actual), whole)
                }
                _ => start,
            }
        };
        if !n.chord {
            cur.last_start = cur.pos;
            cur.last_exact_start = n.start;
            let on_grid = n.start == Frac::new(cur.pos, whole);
            cur.pos += dur_ticks;
            if n.dur != ticks {
                cur.rounded = true;
                cur.rounded_notes += 1;
                // The rounding of one tuplet is less than one division for each of its notes.
                let actual = n.time_mod.map_or(1, |(a, _)| i64::from(a));
                cur.slack = (cur.slack + 1).min(actual);
            } else if on_grid {
                cur.slack = 0;
            }
            cur.exact_end = Some(n.start + n.dur);
        }
    }
    Ok(n)
}

fn pitch(r: &Reader, c: Node) -> Result<Pitch, String> {
    let step = child_text(c, "step").unwrap_or_default();
    let step = crate::model::STEPS
        .iter()
        .position(|s| s.to_string() == step)
        .ok_or_else(|| r.stop(&format!("the step {step}")))?;
    let alter = match child_text(c, "alter") {
        Some(a) => {
            let v: f64 = a.parse().map_err(|_| r.stop(&format!("the alter {a}")))?;
            if v.fract() != 0.0 || v.abs() > 2.0 {
                return Err(r.stop("a microtone"));
            }
            v as i8
        }
        None => 0,
    };
    let octave: i8 = child(c, "octave")
        .map(|o| number(o, "octave", &r.measure))
        .transpose()?
        .unwrap_or(4);
    Ok(Pitch {
        step: u8::try_from(step).expect("STEPS has seven entries"),
        alter,
        octave,
    })
}

pub fn velocity_of(pct: f64) -> f64 {
    (pct * 0.9 * 100.0).round() / 100.0
}

fn notations(r: &mut Reader, n: Node, note: &mut SrcNote) -> Result<(), String> {
    r.attrs(n, &["print-object"])?;
    for c in kids(n) {
        match c.tag_name().name() {
            "tied" => {
                r.attrs(c, &["type", "orientation", "placement", "number"])?;
                match c.attribute("type") {
                    Some("start") => {
                        note.tie_start = true;
                        let dir = match (c.attribute("orientation"), c.attribute("placement")) {
                            (Some("over"), _) | (_, Some("above")) => Some(PitchMark::TieUp),
                            (Some("under"), _) | (_, Some("below")) => Some(PitchMark::TieDown),
                            _ => None,
                        };
                        note.pitch_marks.extend(dir);
                    }
                    Some("stop") => note.tie_stop = true,
                    Some("let-ring") => note.pitch_marks.push(PitchMark::LetRing),
                    _ => {}
                }
            }
            "slur" => {
                r.attrs(
                    c,
                    &["type", "number", "placement", "orientation", "line-type"],
                )?;
                if c.attribute("line-type").is_some_and(|l| l != "solid") {
                    return Err(r.stop("a dashed or dotted slur"));
                }
                let number: u32 = c.attribute("number").unwrap_or("1").parse().unwrap_or(1);
                let place = match (c.attribute("placement"), c.attribute("orientation")) {
                    (Some("above"), _) | (_, Some("over")) => Some(Place::Above),
                    (Some("below"), _) | (_, Some("under")) => Some(Place::Below),
                    _ => None,
                };
                let edge = match c.attribute("type") {
                    Some("start") => Edge::Start,
                    Some("stop") => Edge::Stop,
                    _ => continue,
                };
                note.slurs.push(SrcSlur {
                    edge,
                    number,
                    place,
                    note: n.parent().map_or(0, |p| p.id().get()),
                });
            }
            "tuplet" => {
                r.attrs(
                    c,
                    &[
                        "type",
                        "number",
                        "placement",
                        "show-number",
                        "show-type",
                        "line-shape",
                        "bracket",
                    ],
                )?;
                if c.attribute("bracket").is_some() {
                    r.rep.drop(Row::TupletBracket);
                }
                let number: u32 = c.attribute("number").unwrap_or("1").parse().unwrap_or(1);
                let look = match (c.attribute("show-number"), c.attribute("placement")) {
                    (Some("none"), _) => TupletLook::Hidden,
                    (_, Some("above")) => TupletLook::Up,
                    (_, Some("below")) => TupletLook::Down,
                    _ => TupletLook::Default,
                };
                let ratio = tuplet_ratio(c);
                let edge = match c.attribute("type") {
                    Some("start") => Edge::Start,
                    _ => Edge::Stop,
                };
                note.tuplets.push(SrcTuplet {
                    edge,
                    number,
                    look,
                    ratio,
                });
            }
            "articulations" => {
                for a in kids(c) {
                    r.attrs(a, &["placement", "type"])?;
                    let marks: &[Articulation] = match a.tag_name().name() {
                        "staccato" => &[Articulation::Staccato],
                        "staccatissimo" => &[Articulation::Staccatissimo],
                        "tenuto" => &[Articulation::Tenuto],
                        "accent" => &[Articulation::Accent],
                        "strong-accent" if a.attribute("type") == Some("down") => {
                            &[Articulation::MarcatoDown]
                        }
                        "strong-accent" => &[Articulation::Marcato],
                        "detached-legato" => &[Articulation::Staccato, Articulation::Tenuto],
                        "breath-mark" | "caesura" if matches!(note.head, SrcHead::Rest) => {
                            r.rep.note(format!(
                                "measure {}: a {} on a rest is dropped",
                                r.measure,
                                a.tag_name().name()
                            ));
                            &[]
                        }
                        "breath-mark" => {
                            note.pause = Some(PhraseMark::Breath);
                            &[]
                        }
                        "caesura" => {
                            note.pause = Some(PhraseMark::Caesura);
                            &[]
                        }
                        other => return Err(r.stop(&format!("the articulation {other}"))),
                    };
                    for m in marks {
                        let mark = TokMark::Art(*m);
                        if note.tok_marks.contains(&mark) {
                            r.rep.drop(Row::Duplicate);
                        } else {
                            note.tok_marks.push(mark);
                        }
                    }
                }
            }
            "fermata" => {
                r.attrs(c, &["type"])?;
                let shape = text_of(c);
                if !matches!(shape.as_str(), "" | "normal") {
                    return Err(r.stop(&format!("a fermata of shape {shape}")));
                }
                let mark = TokMark::Fermata {
                    inverted: c.attribute("type") == Some("inverted"),
                };
                if note.tok_marks.contains(&mark) {
                    r.rep.drop(Row::Duplicate);
                } else {
                    note.tok_marks.push(mark);
                }
            }
            "ornaments" => ornaments(r, c, note)?,
            "technical" => {
                for x in kids(c) {
                    if !x.has_tag_name("fingering") {
                        return Err(r.stop(&format!("the technical mark {}", x.tag_name().name())));
                    }
                    r.attrs(x, &["placement", "substitution", "alternate"])?;
                    if yes(x, "substitution") || yes(x, "alternate") {
                        return Err(r.stop("finger substitution"));
                    }
                    let f: u8 = text_of(x)
                        .parse()
                        .map_err(|_| r.stop(&format!("the fingering `{}`", text_of(x))))?;
                    if !(1..=5).contains(&f) {
                        return Err(r.stop(&format!("the fingering {f}")));
                    }
                    note.pitch_marks.push(PitchMark::Finger(f));
                }
            }
            "arpeggiate" => {
                r.attrs(c, &["direction", "number"])?;
                let kind = match (c.attribute("direction"), c.attribute("number")) {
                    (Some("up"), _) => ArpKind::Up,
                    (Some("down"), _) => ArpKind::Down,
                    (_, Some(_)) => ArpKind::CrossStaff,
                    _ => ArpKind::Plain,
                };
                let mark = TokMark::Arp(kind);
                if !note.tok_marks.contains(&mark) {
                    note.tok_marks.push(mark);
                }
            }
            "non-arpeggiate" => return Err(r.stop("a bracket for chords that are not arpeggios")),
            "glissando" | "slide" => {
                r.attrs(c, &["type", "number", "line-type"])?;
                let number: u32 = c.attribute("number").unwrap_or("1").parse().unwrap_or(1);
                if c.attribute("type") == Some("stop") {
                    note.gliss_stops.push(number);
                    continue;
                }
                let text = match text_of(c).trim() {
                    "" => false,
                    "gliss." => true,
                    other => return Err(r.stop(&format!("a glissando with the text {other}"))),
                };
                let line = match (c.tag_name().name(), c.attribute("line-type"), text) {
                    ("glissando", Some("wavy"), false) => GlissLine::Wavy,
                    ("glissando", Some("wavy"), true) => GlissLine::WavyText,
                    ("slide", None | Some("solid"), false) => GlissLine::Straight,
                    ("slide", None | Some("solid"), true) => GlissLine::StraightText,
                    _ => return Err(r.stop("a glissando line of another type")),
                };
                note.gliss_starts.push(number);
                note.pitch_marks.push(PitchMark::Gliss(line));
            }
            "dynamics" => return Err(r.stop("a dynamic mark on a note")),
            other => return Err(r.stop(&format!("the notation {other}"))),
        }
    }
    Ok(())
}

fn tuplet_ratio(c: Node) -> Option<(u32, u32)> {
    let num = |tag: &str| {
        child(c, tag)
            .and_then(|x| child_text(x, "tuplet-number"))
            .and_then(|s| s.parse().ok())
    };
    Some((num("tuplet-actual")?, num("tuplet-normal")?))
}

fn ornaments(r: &mut Reader, c: Node, note: &mut SrcNote) -> Result<(), String> {
    let mut last: Option<usize> = None;
    let mut trill_line = false;
    for x in kids(c) {
        let name = x.tag_name().name();
        let kind = match name {
            "trill-mark" => Some(OrnamentKind::Trill),
            "mordent" if yes(x, "long") => Some(OrnamentKind::LongMordent),
            "mordent" => Some(OrnamentKind::Mordent),
            "inverted-mordent" if yes(x, "long") => Some(OrnamentKind::LongPralltriller),
            "inverted-mordent" => Some(OrnamentKind::Pralltriller),
            "turn" => Some(OrnamentKind::Turn),
            "inverted-turn" => Some(OrnamentKind::InvertedTurn),
            "delayed-turn" => Some(OrnamentKind::DelayedTurn),
            "delayed-inverted-turn" => Some(OrnamentKind::DelayedInvertedTurn),
            _ => None,
        };
        if let Some(kind) = kind {
            if x.has_attribute("approach") || x.has_attribute("departure") {
                return Err(r.stop("an ornament with an approach or departure note"));
            }
            r.attrs(x, &["placement", "long", "slash"])?;
            if yes(x, "slash") {
                return Err(r.stop("a turn with a slash"));
            }
            note.tok_marks.push(TokMark::Ornament {
                kind,
                above: None,
                below: None,
            });
            last = Some(note.tok_marks.len() - 1);
            continue;
        }
        match name {
            "wavy-line" => {
                r.attrs(x, &["type", "number", "placement"])?;
                match x.attribute("type") {
                    Some("start") => trill_line = true,
                    Some("stop") => note.trill_stop = true,
                    _ => {}
                }
            }
            "accidental-mark" => {
                r.attrs(x, &["placement"])?;
                let v = alter_of(&text_of(x))
                    .ok_or_else(|| r.stop(&format!("the ornament accidental {}", text_of(x))))?;
                let Some(TokMark::Ornament { above, below, .. }) =
                    last.and_then(|i| note.tok_marks.get_mut(i))
                else {
                    return Err(r.stop("an accidental mark with no ornament"));
                };
                if x.attribute("placement") == Some("below") {
                    *below = Some(v);
                } else {
                    *above = Some(v);
                }
            }
            "tremolo" => {
                r.attrs(x, &["type", "placement"])?;
                let strokes: u8 = number(x, "tremolo", &r.measure)?;
                match x.attribute("type") {
                    Some("start") => note.tremolo = Some((Edge::Start, strokes)),
                    Some("stop") => note.tremolo = Some((Edge::Stop, strokes)),
                    Some("unmeasured") => return Err(r.stop("an unmeasured tremolo")),
                    _ => note.tok_marks.push(TokMark::Tremolo(strokes)),
                }
            }
            other => return Err(r.stop(&format!("the ornament {other}"))),
        }
    }
    if trill_line {
        let trill = note.tok_marks.iter_mut().find(|m| {
            matches!(
                m,
                TokMark::Ornament {
                    kind: OrnamentKind::Trill,
                    ..
                }
            )
        });
        match trill {
            Some(TokMark::Ornament { kind, .. }) => *kind = OrnamentKind::TrillLine,
            _ => return Err(r.stop("a wavy line with no trill mark")),
        }
    }
    Ok(())
}

fn place_of(n: Node) -> Option<Place> {
    match n.attribute("placement") {
        Some("above") => Some(Place::Above),
        Some("below") => Some(Place::Below),
        _ => None,
    }
}

enum Piece {
    Words(String),
    Dyn(DynMark),
    DynText(String),
    Other,
}

fn direction(
    r: &mut Reader,
    e: Node,
    div: i64,
    pos: i64,
    out: &mut SrcMeasure,
) -> Result<(), String> {
    r.attrs(e, &["placement", "system", "directive"])?;
    let place = place_of(e);
    let system = e.attribute("system") == Some("only-top") || yes(e, "directive");
    let as_exported = e.attribute("system") == Some("only-top") && !e.has_attribute("directive");
    let mut offset = 0i64;
    let mut staff = Staff::Upper;
    let mut sound: Option<Node> = None;
    let mut types = Vec::new();
    for c in kids(e) {
        match c.tag_name().name() {
            "direction-type" => types.push(c),
            "offset" => {
                r.attrs(c, &["sound"])?;
                offset = number(c, "offset", &r.measure)?;
            }
            "staff" => staff = staff_of(r, &text_of(c))?,
            "sound" => sound = Some(c),
            "voice" => {}
            "listening" => return Err(r.stop("<listening>")),
            _ => return Err(r.unknown(c, "direction")),
        }
    }
    let t = Frac::new(pos + offset, 4 * div.max(1));
    if (pos + offset) < 0 {
        return Err(r.stop("a direction before the start of the measure"));
    }
    let mut tempo_attr = None;
    let mut dyn_attr = None;
    let mut jump = None;
    if let Some(s) = sound {
        r.attrs(
            s,
            &[
                "tempo",
                "dynamics",
                "dacapo",
                "dalsegno",
                "fine",
                "tocoda",
                "segno",
                "coda",
                "damper-pedal",
            ],
        )?;
        if let Some(k) = kids(s).next() {
            return Err(r.unknown(k, "sound"));
        }
        tempo_attr = s
            .attribute("tempo")
            .map(str::parse::<f64>)
            .transpose()
            .map_err(|_| r.stop("a tempo value"))?;
        dyn_attr = s
            .attribute("dynamics")
            .map(str::parse::<f64>)
            .transpose()
            .map_err(|_| r.stop("a dynamics value"))?;
        jump = if s.attribute("dacapo").is_some() {
            Some("dc")
        } else if s.attribute("dalsegno").is_some() {
            Some("ds")
        } else if s.attribute("tocoda").is_some() {
            Some("tocoda")
        } else if s.attribute("fine").is_some() {
            Some("fine")
        } else {
            None
        };
    }
    let mut pieces: Vec<Piece> = Vec::new();
    let mut metronome: Option<(Metronome, bool, bool)> = None;
    let mut dashes_start = None;
    let mut words_place = place;
    let mut frame = TextFrame::None;
    for dt in &types {
        r.attrs(*dt, &[])?;
        for x in kids(*dt) {
            let name = x.tag_name().name();
            match name {
                "words" => {
                    r.attrs(x, &["enclosure", "dir"])?;
                    match x.attribute("enclosure") {
                        None | Some("none") => {}
                        Some("rectangle") if frame == TextFrame::Rehearsal => {
                            return Err(
                                r.stop("a rehearsal mark and a boxed text in one direction")
                            );
                        }
                        Some("rectangle") => frame = TextFrame::Box,
                        Some(shape) => {
                            return Err(r.stop(&format!("a text with a frame of shape {shape}")));
                        }
                    }
                    let text = x.text().unwrap_or("").to_string();
                    match pieces.last_mut() {
                        Some(Piece::Words(w)) => w.push_str(&text),
                        _ => pieces.push(Piece::Words(text)),
                    }
                }
                "dynamics" => {
                    r.attrs(x, &["placement"])?;
                    words_place = words_place.or(place_of(x));
                    let marks: Vec<Node> = kids(x).collect();
                    let [m] = marks.as_slice() else {
                        return Err(r.stop("a dynamic mark with more than one sign"));
                    };
                    let name = m.tag_name().name();
                    if name == "other-dynamics" {
                        pieces.push(Piece::DynText(text_of(*m)));
                        continue;
                    }
                    let mark = DynMark::parse(name)
                        .ok_or_else(|| r.stop(&format!("the dynamic mark {name}")))?;
                    pieces.push(Piece::Dyn(mark));
                }
                "wedge" => {
                    r.attrs(x, &["type", "number", "niente", "line-type"])?;
                    if yes(x, "niente") {
                        return Err(r.stop("a hairpin with niente"));
                    }
                    let number: u32 = x.attribute("number").unwrap_or("1").parse().unwrap_or(1);
                    let start = match x.attribute("type") {
                        Some("crescendo") => Some(Hairpin::Cresc),
                        Some("diminuendo") => Some(Hairpin::Dim),
                        Some("stop") => None,
                        _ => continue,
                    };
                    out.dirs.push(SrcDir {
                        t,
                        staff,
                        kind: DirKind::Wedge {
                            start,
                            number,
                            place,
                        },
                    });
                    pieces.push(Piece::Other);
                }
                "dashes" => {
                    r.attrs(x, &["type", "number", "dash-length", "space-length"])?;
                    let number: u32 = x.attribute("number").unwrap_or("1").parse().unwrap_or(1);
                    match x.attribute("type") {
                        Some("start") => dashes_start = Some(number),
                        Some("stop") => out.dirs.push(SrcDir {
                            t,
                            staff,
                            kind: DirKind::DashesStop { number },
                        }),
                        _ => {}
                    }
                }
                "pedal" => {
                    r.attrs(x, &["type", "line", "sign", "abbreviated"])?;
                    if staff == Staff::Upper || place == Some(Place::Above) {
                        r.rep.drop(Row::PedalStaff);
                    }
                    let style = if yes(x, "line") {
                        PedalStyle::Line
                    } else {
                        PedalStyle::Sign
                    };
                    let kind = match x.attribute("type") {
                        Some("start") => PedalKind::Start,
                        Some("stop") => PedalKind::Stop,
                        Some("change") => PedalKind::Change,
                        Some("continue") => continue,
                        Some(other) => return Err(r.stop(&format!("a pedal of type {other}"))),
                        None => return Err(r.stop("a pedal with no type")),
                    };
                    if kind == PedalKind::Start {
                        out.pedal_style = out.pedal_style.or(Some(style));
                    }
                    out.dirs.push(SrcDir {
                        t,
                        staff,
                        kind: DirKind::Pedal(kind, style),
                    });
                }
                "octave-shift" => {
                    r.attrs(
                        x,
                        &["type", "size", "number", "dash-length", "space-length"],
                    )?;
                    let size = x.attribute("size").unwrap_or("8");
                    let number: u32 = x.attribute("number").unwrap_or("1").parse().unwrap_or(1);
                    let start = match (x.attribute("type"), size) {
                        (Some("down"), "8") => Some(Ottava::Up8),
                        (Some("up"), "8") => Some(Ottava::Down8),
                        (Some("down"), "15") => Some(Ottava::Up15),
                        (Some("up"), "15") => Some(Ottava::Down15),
                        (Some("stop"), _) => None,
                        (Some("continue"), _) => continue,
                        _ => return Err(r.stop(&format!("an ottava of size {size}"))),
                    };
                    out.dirs.push(SrcDir {
                        t,
                        staff,
                        kind: DirKind::Octave { start, number },
                    });
                }
                "metronome" => {
                    r.attrs(x, &["parentheses", "print-object", "justify"])?;
                    metronome = Some((
                        metronome_of(r, x)?,
                        yes(x, "parentheses"),
                        x.attribute("print-object") != Some("no"),
                    ));
                }
                "segno" | "coda" => {
                    r.attrs(x, &["smufl"])?;
                    if !t.is_zero() {
                        return Err(r.stop("a segno or coda inside a measure"));
                    }
                    out.attrs.push(Attr::Sign(if name == "segno" {
                        Sign::Segno
                    } else {
                        Sign::Coda
                    }));
                    pieces.push(Piece::Other);
                }
                "bracket" => {
                    r.rep.drop(Row::TextBracket);
                    pieces.push(Piece::Other);
                }
                "rehearsal" => {
                    r.attrs(x, &[])?;
                    if frame == TextFrame::Box {
                        return Err(r.stop("a rehearsal mark and a boxed text in one direction"));
                    }
                    frame = TextFrame::Rehearsal;
                    pieces.push(Piece::Words(x.text().unwrap_or("").to_string()));
                }
                other => return Err(r.stop(&format!("the direction {other}"))),
            }
        }
    }
    if let Some(j) = jump {
        let words: String = pieces
            .iter()
            .filter_map(|p| match p {
                Piece::Words(w) => Some(w.as_str()),
                _ => None,
            })
            .collect();
        let lower = words.to_lowercase();
        let kind = match j {
            "dc" if lower.contains("fine") => Jump::DcFine,
            "dc" if lower.contains("coda") => Jump::DcCoda,
            "dc" => Jump::Dc,
            "ds" if lower.contains("fine") => Jump::DsFine,
            "ds" if lower.contains("coda") => Jump::DsCoda,
            "ds" => Jump::Ds,
            "tocoda" => Jump::ToCoda,
            _ => Jump::Fine,
        };
        if !words.is_empty() {
            r.rep.drop(Row::JumpText);
        }
        if staff == Staff::Lower || place == Some(Place::Below) {
            r.rep.drop(Row::JumpPlace);
        }
        out.attrs.push(Attr::Jump(kind));
        return Ok(());
    }
    if let Some(text) = pieces.iter().find_map(|p| match p {
        Piece::DynText(t) => Some(t.clone()),
        _ => None,
    }) {
        let words = pieces
            .iter()
            .any(|p| matches!(p, Piece::Words(w) if !w.trim().is_empty()));
        let mark = pieces.iter().any(|p| matches!(p, Piece::Dyn(_)));
        if words || mark {
            return Err(r.stop("a dynamic text next to a dynamic mark or a text"));
        }
        out.dirs.push(SrcDir {
            t,
            staff,
            kind: DirKind::DynText(DynText {
                text,
                level: dyn_attr.map(velocity_of),
                place: words_place.filter(|p| *p != Place::Below),
            }),
        });
        return Ok(());
    }
    let dyn_at = pieces.iter().position(|p| matches!(p, Piece::Dyn(_)));
    if let Some(i) = dyn_at {
        let Piece::Dyn(mark) = pieces[i] else {
            unreachable!("position found a dynamic mark")
        };
        let word = |p: Option<&Piece>| match p {
            Some(Piece::Words(w)) if !w.trim().is_empty() => Some(w.trim().to_string()),
            _ => None,
        };
        let before = i.checked_sub(1).and_then(|j| word(pieces.get(j)));
        let after = word(pieces.get(i + 1));
        let words = pieces
            .iter()
            .filter(|p| matches!(p, Piece::Words(_)))
            .count();
        let used = usize::from(before.is_some()) + usize::from(after.is_some());
        let marks = pieces.iter().filter(|p| matches!(p, Piece::Dyn(_))).count();
        if marks > 1 || words > used {
            return Err(r.stop("a direction with more than one dynamic mark or text"));
        }
        out.dirs.push(SrcDir {
            t,
            staff,
            kind: DirKind::Dyn {
                mark: Dyn {
                    before,
                    mark,
                    after,
                    place: words_place.filter(|p| *p != Place::Below),
                },
                velocity: if let Some(v) = dyn_attr {
                    velocity_of(v)
                } else {
                    r.rep.drop(Row::TableVelocity);
                    mark.default_velocity()
                },
            },
        });
        return Ok(());
    }
    if dyn_attr.is_some() {
        return Err(r.stop("a playback dynamic with no dynamic mark"));
    }
    let words: Option<String> = pieces.iter().find_map(|p| match p {
        Piece::Words(w) if !w.trim().is_empty() => Some(w.trim().to_string()),
        _ => None,
    });
    let is_tempo = system || metronome.is_some() || tempo_attr.is_some();
    if is_tempo {
        let mark = match (metronome, tempo_attr) {
            (Some((m, parens, true)), sound) => {
                let value = tempo_value(&m);
                if sound.is_some_and(|s| (s - value).abs() > 0.01) {
                    r.rep.drop(Row::TempoSound);
                }
                Some(TempoMark::Shown {
                    metronome: m,
                    parens,
                })
            }
            (Some((m, _, false)), sound) => {
                let value = tempo_value(&m);
                Some(match sound {
                    Some(s) if (s - value).abs() > 0.01 => TempoMark::Sound(s),
                    _ => TempoMark::hidden(m),
                })
            }
            (None, Some(s)) => Some(TempoMark::Sound(s)),
            (None, None) => None,
        };
        if words.is_none() && mark.is_none() {
            return Ok(());
        }
        if staff != Staff::Upper && !system {
            return Err(r.stop("a tempo mark on the lower staff"));
        }
        if !as_exported {
            r.rep.drop(Row::TempoStaves);
        }
        out.dirs.push(SrcDir {
            t,
            staff: Staff::Upper,
            kind: DirKind::Tempo {
                tempo: Tempo {
                    text: words,
                    mark,
                    ext: false,
                    place: place.filter(|p| *p != Place::Above),
                },
                dashes: dashes_start,
            },
        });
        return Ok(());
    }
    if let Some(text) = words {
        let default = if staff == Staff::Upper {
            Place::Above
        } else {
            Place::Below
        };
        out.dirs.push(SrcDir {
            t,
            staff,
            kind: DirKind::Text {
                text: Text {
                    text,
                    ext: false,
                    place: place.filter(|p| *p != default),
                    frame,
                },
                dashes: dashes_start,
            },
        });
    } else if dashes_start.is_some() {
        return Err(r.stop("a dashed line with no text"));
    }
    Ok(())
}

fn metronome_of(r: &Reader, x: Node) -> Result<Metronome, String> {
    let mut unit = None;
    let mut dotted = false;
    let mut bpm = None;
    for k in kids(x) {
        match k.tag_name().name() {
            "beat-unit" if unit.is_none() => unit = Some(text_of(k)),
            "beat-unit-dot" => dotted = true,
            "per-minute" if text_of(k).is_empty() => {
                return Err(r.stop("a metronome mark with no number"));
            }
            "per-minute" => {
                let v: f64 = text_of(k)
                    .parse()
                    .map_err(|_| r.stop(&format!("the tempo `{}`", text_of(k))))?;
                bpm = Some(v);
            }
            _ => return Err(r.stop("a metric modulation mark")),
        }
    }
    let unit = match unit.as_deref() {
        Some("whole") => 'w',
        Some("half") => 'h',
        Some("quarter") => 'q',
        Some("eighth") => 'e',
        Some("16th") => 's',
        other => {
            return Err(r.stop(&format!("the beat unit {}", other.unwrap_or(""))));
        }
    };
    let bpm = bpm.ok_or_else(|| r.stop("a metronome mark with no tempo"))?;
    Ok(Metronome { unit, dotted, bpm })
}

pub fn tempo_value(m: &Metronome) -> f64 {
    let unit = match m.unit {
        'w' => 4.0,
        'h' => 2.0,
        'e' => 0.5,
        's' => 0.25,
        _ => 1.0,
    };
    let dotted = if m.dotted { 1.5 } else { 1.0 };
    m.bpm * unit * dotted
}

fn volta_numbers(s: &str) -> Option<Vec<u32>> {
    let nums: Option<Vec<u32>> = s
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|x| !x.is_empty())
        .map(|x| x.trim_end_matches('.').parse().ok())
        .collect();
    nums.filter(|n| !n.is_empty())
}

fn barline(
    r: &mut Reader,
    e: Node,
    state: &mut PartState,
    out: &mut SrcMeasure,
    repeat_end: &mut Option<u32>,
    ending_stop: &mut Option<BracketEnd>,
) -> Result<(), String> {
    r.attrs(e, &["location", "segno", "coda"])?;
    let left = match e.attribute("location") {
        Some("left") => true,
        Some("right") | None => false,
        Some(_) => return Err(r.stop("a barline in the middle of a measure")),
    };
    let mut style = None;
    let mut fermata = false;
    for c in kids(e) {
        match c.tag_name().name() {
            "bar-style" => style = Some(text_of(c)),
            "repeat" => {
                r.attrs(c, &["direction", "times", "winged"])?;
                if c.attribute("direction") == Some("forward") {
                    merge_repeat(&mut out.attrs, RepeatSide::Start);
                } else {
                    let times = c
                        .attribute("times")
                        .and_then(|t| t.parse().ok())
                        .unwrap_or(2);
                    *repeat_end = Some(times);
                }
            }
            "ending" => {
                r.attrs(
                    c,
                    &[
                        "number",
                        "type",
                        "print-object",
                        "end-length",
                        "text-x",
                        "text-y",
                    ],
                )?;
                if !text_of(c).is_empty() {
                    r.rep.drop(Row::JumpText);
                }
                let numbers = volta_numbers(c.attribute("number").unwrap_or(""))
                    .ok_or_else(|| r.stop("a volta with no number"))?;
                match c.attribute("type") {
                    Some("start") => {
                        if !out.attrs.iter().any(|a| matches!(a, Attr::Volta { .. })) {
                            out.attrs.push(Attr::Volta {
                                numbers: numbers.clone(),
                                end: None,
                            });
                        }
                        state.volta = Some(numbers);
                    }
                    Some("stop") => {
                        *ending_stop = Some(BracketEnd::Closed);
                        state.volta = None;
                    }
                    Some("discontinue") => {
                        *ending_stop = Some(BracketEnd::Open);
                        state.volta = None;
                    }
                    _ => return Err(r.stop("a volta of unknown type")),
                }
            }
            "fermata" => {
                r.attrs(c, &["type"])?;
                fermata = true;
            }
            "segno" => out.attrs.push(Attr::Sign(Sign::Segno)),
            "coda" => out.attrs.push(Attr::Sign(Sign::Coda)),
            "wavy-line" => return Err(r.stop("a wavy line on a barline")),
            _ => return Err(r.unknown(c, "barline")),
        }
    }
    let style = match (left, style.as_deref()) {
        (true, Some("heavy-light" | "regular") | None) => None,
        (true, Some(s)) => return Err(r.stop(&format!("the left barline {s}"))),
        (false, None | Some("regular")) => fermata.then_some(BarStyle::Normal),
        (false, Some("light-light")) => Some(BarStyle::Double),
        (false, Some("light-heavy")) if repeat_end.is_some() && !fermata => None,
        (false, Some("light-heavy")) => Some(BarStyle::Final),
        (false, Some("dashed")) => Some(BarStyle::Dashed),
        (false, Some("dotted")) => Some(BarStyle::Dotted),
        (false, Some("none")) => Some(BarStyle::None),
        (false, Some(s)) => return Err(r.stop(&format!("the barline {s}"))),
    };
    if let Some(style) = style {
        out.attrs.push(Attr::Barline { style, fermata });
    } else if fermata {
        return Err(r.stop("a fermata on a left barline"));
    }
    Ok(())
}
