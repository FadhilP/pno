pub mod import;

use std::fmt::Write as _;
use std::ops::RangeInclusive;

use crate::analysis::{Analysis, Leaf, NoteRef, Part, PedalAction, Span, TupletGroup, flatten};
use crate::frac::{Frac, NoteValue, lcm};
use crate::model::{
    AccDisplay, ArpKind, Articulation, Attr, BarStyle, BracketEnd, Break, Clef, Dyn, Event,
    GlissLine, GraceNote, GroupHead, Hairpin, Head, Jump, Key, Lane, MarkKind, MetaField, Meter,
    Mode, Note, OrnamentKind, Ottava, PedalStyle, PhraseMark, Pitch, PitchMark, Place, Sign, Staff,
    Tempo, TempoMark, TextFrame, TimeSymbol, Tok, TokMark, TupletLook,
};
use crate::perform::TICKS_PER_QUARTER;

const MUSICXML_VELOCITY_BASE: f64 = 90.0;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn num(v: f64) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

fn type_name(log2: i32) -> Option<&'static str> {
    Some(match log2 {
        1 => "breve",
        0 => "whole",
        -1 => "half",
        -2 => "quarter",
        -3 => "eighth",
        -4 => "16th",
        -5 => "32nd",
        -6 => "64th",
        -7 => "128th",
        -8 => "256th",
        _ => return None,
    })
}

fn acc_name(alter: i8) -> &'static str {
    match alter {
        -2 => "flat-flat",
        -1 => "flat",
        1 => "sharp",
        2 => "double-sharp",
        _ => "natural",
    }
}

struct Ctx<'s, 'a> {
    a: &'s Analysis<'a>,
    div: i64,
    range: RangeInclusive<usize>,
    frame: Frame,
    pedal: PedalMarks,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Frame {
    Full,
    // An image of a bar range: no titles, part name or line breaks of the score.
    Excerpt,
}

impl Ctx<'_, '_> {
    fn ticks(&self, d: Frac) -> Result<i64, String> {
        let q = d * 4 * self.div;
        if !q.is_int() {
            return Err(format!(
                "internal: duration {d} is not a whole number of divisions"
            ));
        }
        Ok(q.num())
    }

    fn pos(&self, bar: usize, t: Frac) -> Result<i64, String> {
        self.ticks(t - self.a.score.bars[bar].layout.lead)
    }
}

#[derive(Clone, Debug)]
pub enum Scope {
    Full,
    // A bar range for an image: no title frame.
    Bars(RangeInclusive<usize>),
}

// PDF, PNG and SVG leave out the pedal marks; MusicXML keeps them, and MuseScore plays them.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PedalMarks {
    Show,
    Hide,
}

pub fn export(a: &Analysis, scope: Scope, pedal: PedalMarks) -> Result<String, String> {
    let score = a.score;
    if score.bars.is_empty() {
        return Err("the file has no bars".into());
    }
    let (range, frame) = match scope {
        Scope::Full => (0..=score.bars.len() - 1, Frame::Full),
        Scope::Bars(r) => (r, Frame::Excerpt),
    };
    let ctx = Ctx {
        a,
        div: divisions(a),
        range,
        frame,
        pedal,
    };
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<!DOCTYPE score-partwise PUBLIC \"-//Recordare//DTD MusicXML 4.0 Partwise//EN\" \"http://www.musicxml.org/dtds/partwise.dtd\">\n");
    out.push_str("<score-partwise version=\"4.0\">\n");
    header(&ctx, &mut out);
    out.push_str("  <part id=\"P1\">\n");
    let mut state = State::default();
    for b in ctx.range.clone() {
        measure(&ctx, b, &mut state, &mut out)?;
    }
    out.push_str("  </part>\n</score-partwise>\n");
    Ok(out)
}

fn divisions(a: &Analysis) -> i64 {
    let mut d = 1i64;
    let mut add = |f: Frac| d = lcm(d, (f * 4).den());
    for bar in &a.score.bars {
        add(bar.layout.lead);
        add(bar.layout.length());
        for line in &bar.lines {
            for (s, e, _) in crate::analysis::flatten(&line.els) {
                add(s);
                add(e);
            }
        }
    }
    d
}

fn meta(score: &crate::model::Score, f: MetaField) -> Option<&str> {
    score.header.iter().find_map(|a| match a {
        Attr::Meta(m, t) if *m == f => Some(t.as_str()),
        _ => None,
    })
}

fn header(ctx: &Ctx, out: &mut String) {
    let score = ctx.a.score;
    let titles = ctx.frame == Frame::Full;
    // With no credit elements, MuseScore 4 builds the title frame from these fields.
    if let Some(sub) = meta(score, MetaField::Subtitle).filter(|_| titles) {
        let _ = writeln!(out, "  <movement-number>{}</movement-number>", esc(sub));
    }
    if let Some(t) = meta(score, MetaField::Title).filter(|_| titles) {
        let _ = writeln!(out, "  <movement-title>{}</movement-title>", esc(t));
    }
    out.push_str("  <identification>\n");
    for (f, kind) in [
        (MetaField::Composer, "composer"),
        (MetaField::Arranger, "arranger"),
        (MetaField::Lyricist, "lyricist"),
    ] {
        if let Some(t) = meta(score, f).filter(|_| titles) {
            let _ = writeln!(out, "    <creator type=\"{kind}\">{}</creator>", esc(t));
        }
    }
    if let Some(t) = meta(score, MetaField::Copyright) {
        let _ = writeln!(out, "    <rights>{}</rights>", esc(t));
    }
    let _ = writeln!(
        out,
        "    <encoding>\n      <software>pno {}</software>\n      <supports element=\"accidental\" type=\"yes\"/>\n    </encoding>",
        env!("CARGO_PKG_VERSION")
    );
    let vel = vel_lines(score);
    let phrases = phrase_fields(score);
    if !vel.is_empty() || !phrases.is_empty() {
        out.push_str("    <miscellaneous>\n");
        // MuseScore keeps one field for each name, so each line has its own name.
        for (i, v) in vel.iter().enumerate() {
            let _ = writeln!(
                out,
                "      <miscellaneous-field name=\"pno-vel-{}\">{}</miscellaneous-field>",
                i + 1,
                esc(v)
            );
        }
        for (i, v) in phrases.iter().enumerate() {
            let _ = writeln!(
                out,
                "      <miscellaneous-field name=\"pno-phrase-{}\">{}</miscellaneous-field>",
                i + 1,
                esc(v)
            );
        }
        out.push_str("    </miscellaneous>\n");
    }
    out.push_str("  </identification>\n");
    let mut program = 1;
    let mut volume = None;
    let mut pan = None;
    for a in &score.header {
        match a {
            Attr::Program(p) => program = *p,
            Attr::Volume(v) => volume = Some(*v),
            Attr::Pan(v) => pan = Some(*v),
            _ => {}
        }
    }
    let shown = if titles { "" } else { " print-object=\"no\"" };
    let _ = write!(
        out,
        "  <part-list>\n    <score-part id=\"P1\">\n      <part-name{shown}>Piano</part-name>\n"
    );
    out.push_str("      <score-instrument id=\"P1-I1\">\n        <instrument-name>Piano</instrument-name>\n      </score-instrument>\n");
    let _ = write!(
        out,
        "      <midi-instrument id=\"P1-I1\">\n        <midi-channel>1</midi-channel>\n        <midi-program>{program}</midi-program>\n"
    );
    if let Some(v) = volume {
        let _ = writeln!(out, "        <volume>{v}</volume>");
    }
    if let Some(p) = pan {
        let _ = writeln!(out, "        <pan>{p}</pan>");
    }
    out.push_str("      </midi-instrument>\n    </score-part>\n  </part-list>\n");
}

// The phrase marks that are not drawn: "<bar index> <time in the bar> <mark>".
fn phrase_fields(score: &crate::model::Score) -> Vec<String> {
    let mut out = Vec::new();
    for (b, bar) in score.bars.iter().enumerate() {
        for line in bar.lines.iter().filter(|l| l.lane == Lane::Phrase) {
            for (start, _, tok) in flatten(&line.els) {
                let Tok::Phrase(m) = tok else { continue };
                if !m.is_drawn() {
                    out.push(format!("{b} {start} {}", m.text()));
                }
            }
        }
    }
    out
}

fn vel_lines(score: &crate::model::Score) -> Vec<String> {
    let is_vel = |a: &Attr| matches!(a, Attr::VelMark(..) | Attr::VelLane(..));
    let mut out: Vec<String> = score
        .header
        .iter()
        .filter(|a| is_vel(a))
        .map(|a| format!("@ {a}"))
        .collect();
    for bar in &score.bars {
        out.extend(
            bar.attrs
                .iter()
                .filter(|a| is_vel(a))
                .map(|a| format!("{} @ {a}", bar.label)),
        );
    }
    out
}

#[derive(Default)]
struct State {
    key: Option<Key>,
    meter: Option<Meter>,
    clefs: [Option<Clef>; 2],
    first_done: bool,
}

struct Item {
    t: i64,
    order: u8,
    xml: String,
    advance: i64,
}

fn measure(ctx: &Ctx, b: usize, state: &mut State, out: &mut String) -> Result<(), String> {
    let a = ctx.a;
    let bar = &a.score.bars[b];
    let implicit = if bar.label == "0" || bar.label.ends_with('b') {
        " implicit=\"yes\""
    } else {
        ""
    };
    let _ = writeln!(
        out,
        "    <measure number=\"{}\"{implicit}>",
        esc(&bar.label)
    );
    for at in &bar.attrs {
        match at {
            Attr::Break(Break::System) if ctx.frame == Frame::Full && b != 0 => {
                out.push_str("      <print new-system=\"yes\"/>\n");
            }
            Attr::Break(Break::Page) if ctx.frame == Frame::Full && b != 0 => {
                out.push_str("      <print new-page=\"yes\"/>\n");
            }
            _ => {}
        }
    }
    left_barline(a, b, out);
    attributes(ctx, b, state, out);
    let mut streams: Vec<(Lane, Vec<Item>)> = Vec::new();
    let mut lines: Vec<&crate::model::Line> =
        bar.lines.iter().filter(|l| l.lane.is_voice()).collect();
    lines.sort_by_key(|l| l.lane.order());
    for line in &lines {
        let mut items = Vec::new();
        for (id, leaf) in a
            .leaves
            .iter()
            .enumerate()
            .filter(|(_, l)| l.bar == b && l.lane == line.lane)
        {
            leaf_items(ctx, id, leaf, &mut items)?;
        }
        streams.push((line.lane, items));
    }
    let mut free = directions(ctx, b)?;
    for (lane, items) in &mut streams {
        let Lane::Voice(staff, 1) = *lane else {
            continue;
        };
        // In time order in one stream, a span stop cannot come before its start (MusicXML
        // readers pair them in document order). A direction goes before the note, gap or
        // later direction-free item at its time; directions of one time keep their order.
        let mut keep = Vec::new();
        for d in free.drain(..) {
            if d.1 != staff {
                keep.push(d);
                continue;
            }
            let after = |n: &Item| {
                let sounds_at_same_time = n.t == d.0.t && n.advance > 0;
                n.t > d.0.t || sounds_at_same_time
            };
            let i = items.iter().position(after).unwrap_or(items.len());
            items.insert(i, d.0);
        }
        free = keep;
    }
    let mut cursor = 0i64;
    for (_, items) in &streams {
        for it in items {
            move_to(out, &mut cursor, it.t);
            out.push_str(&it.xml);
            cursor += it.advance;
        }
    }
    free.sort_by_key(|(d, _)| (d.t, d.order));
    for (d, _) in free {
        move_to(out, &mut cursor, d.t);
        out.push_str(&d.xml);
    }
    right_barline(a, b, out);
    out.push_str("    </measure>\n");
    Ok(())
}

fn move_to(out: &mut String, cursor: &mut i64, t: i64) {
    if t < *cursor {
        let _ = writeln!(
            out,
            "      <backup>\n        <duration>{}</duration>\n      </backup>",
            *cursor - t
        );
    } else if t > *cursor {
        let _ = writeln!(
            out,
            "      <forward>\n        <duration>{}</duration>\n      </forward>",
            t - *cursor
        );
    }
    *cursor = t;
}

fn volta(bar: &crate::model::Bar) -> Option<(&[u32], Option<BracketEnd>)> {
    bar.attrs.iter().find_map(|a| match a {
        Attr::Volta { numbers, end } => Some((numbers.as_slice(), *end)),
        _ => None,
    })
}

fn repeat(bar: &crate::model::Bar) -> (bool, Option<u32>) {
    bar.attrs
        .iter()
        .find_map(|a| match a {
            Attr::Repeat { start, end } => Some((*start, *end)),
            _ => None,
        })
        .unwrap_or((false, None))
}

fn volta_numbers(n: &[u32]) -> (String, String) {
    let list: Vec<String> = n.iter().map(u32::to_string).collect();
    (list.join(","), format!("{}.", list.join(", ")))
}

fn left_barline(a: &Analysis, b: usize, out: &mut String) {
    let bar = &a.score.bars[b];
    let (start, _) = repeat(bar);
    let prev = b.checked_sub(1).map(|p| &a.score.bars[p]);
    let volta_start = volta(bar).filter(|(n, _)| prev.and_then(volta).map(|(p, _)| p) != Some(*n));
    if !start && volta_start.is_none() {
        return;
    }
    out.push_str("      <barline location=\"left\">\n");
    if start {
        out.push_str("        <bar-style>heavy-light</bar-style>\n");
    }
    if let Some((n, _)) = volta_start {
        let (number, text) = volta_numbers(n);
        let _ = writeln!(
            out,
            "        <ending number=\"{number}\" type=\"start\">{text}</ending>"
        );
    }
    if start {
        out.push_str("        <repeat direction=\"forward\"/>\n");
    }
    out.push_str("      </barline>\n");
}

fn right_barline(a: &Analysis, b: usize, out: &mut String) {
    let bar = &a.score.bars[b];
    let (_, end) = repeat(bar);
    let style = bar.attrs.iter().find_map(|a| match a {
        Attr::Barline { style, fermata } => Some((*style, *fermata)),
        _ => None,
    });
    let next = a.score.bars.get(b + 1);
    let volta_end = volta(bar).filter(|(n, _)| next.and_then(volta).map(|(x, _)| x) != Some(*n));
    if end.is_none() && style.is_none() && volta_end.is_none() {
        return;
    }
    out.push_str("      <barline location=\"right\">\n");
    let bar_style = match (style.map(|s| s.0), end) {
        (Some(s), _) => Some(match s {
            BarStyle::Normal => "regular",
            BarStyle::Double => "light-light",
            BarStyle::Final => "light-heavy",
            BarStyle::Dashed => "dashed",
            BarStyle::Dotted => "dotted",
            BarStyle::None => "none",
        }),
        (None, Some(_)) => Some("light-heavy"),
        (None, None) => None,
    };
    if let Some(s) = bar_style {
        let _ = writeln!(out, "        <bar-style>{s}</bar-style>");
    }
    if style.is_some_and(|s| s.1) {
        out.push_str("        <fermata type=\"upright\"/>\n");
    }
    if let Some((n, close)) = volta_end {
        let closed = close.map_or(end.is_some(), |c| c == BracketEnd::Closed);
        let kind = if closed { "stop" } else { "discontinue" };
        let _ = writeln!(
            out,
            "        <ending number=\"{}\" type=\"{kind}\"/>",
            volta_numbers(n).0
        );
    }
    if let Some(times) = end {
        let t = if times == 2 {
            String::new()
        } else {
            format!(" times=\"{times}\"")
        };
        let _ = writeln!(out, "        <repeat direction=\"backward\"{t}/>");
    }
    out.push_str("      </barline>\n");
}

fn key_xml(k: Key) -> String {
    let mode = match k.mode {
        Some(Mode::Major) => "\n          <mode>major</mode>",
        Some(Mode::Minor) => "\n          <mode>minor</mode>",
        None => "",
    };
    format!(
        "        <key>\n          <fifths>{}</fifths>{mode}\n        </key>\n",
        k.fifths
    )
}

fn time_xml(m: &Meter) -> String {
    let symbol = match m.symbol {
        Some(TimeSymbol::Common) => " symbol=\"common\"",
        Some(TimeSymbol::Cut) => " symbol=\"cut\"",
        None => "",
    };
    let beats: Vec<String> = m.beats.iter().map(u32::to_string).collect();
    format!(
        "        <time{symbol}>\n          <beats>{}</beats>\n          <beat-type>{}</beat-type>\n        </time>\n",
        beats.join("+"),
        m.beat_type
    )
}

fn gliss_look(line: GlissLine) -> (&'static str, &'static str) {
    match line {
        GlissLine::Wavy | GlissLine::WavyText => ("glissando", "wavy"),
        GlissLine::Straight | GlissLine::StraightText => ("slide", "solid"),
    }
}

fn clef_xml(staff: Staff, c: Clef) -> String {
    let (sign, line, octave) = match c {
        Clef::Treble => ("G", 2, 0),
        Clef::Bass => ("F", 4, 0),
        Clef::Alto => ("C", 3, 0),
        Clef::Tenor => ("C", 4, 0),
        Clef::Treble8va => ("G", 2, 1),
        Clef::Treble8vb => ("G", 2, -1),
        Clef::Bass8va => ("F", 4, 1),
        Clef::Bass8vb => ("F", 4, -1),
    };
    let octave = if octave == 0 {
        String::new()
    } else {
        format!("          <clef-octave-change>{octave}</clef-octave-change>\n")
    };
    format!(
        "        <clef number=\"{}\">\n          <sign>{sign}</sign>\n          <line>{line}</line>\n{octave}        </clef>\n",
        staff.number()
    )
}

fn clef_tokens(a: &Analysis, b: usize, staff: Staff) -> Vec<(Frac, Clef)> {
    a.spans
        .iter()
        .filter(|s| s.start.0 == b && s.lane == Lane::Mark(staff, MarkKind::Clef, 1))
        .filter_map(|s| match s.tok {
            Tok::Clef(c) => Some((s.start.1, *c)),
            _ => None,
        })
        .collect()
}

fn attributes(ctx: &Ctx, b: usize, state: &mut State, out: &mut String) {
    let a = ctx.a;
    let bar = &a.score.bars[b];
    let first = !state.first_done;
    if first {
        for p in 0..b {
            for staff in [Staff::Upper, Staff::Lower] {
                if let Some((_, c)) = clef_tokens(a, p, staff).last() {
                    state.clefs[usize::from(staff.number() - 1)] = Some(*c);
                }
            }
        }
    }
    let key = a.bars[b].key;
    let mut body = String::new();
    if first {
        let _ = writeln!(body, "        <divisions>{}</divisions>", ctx.div);
    }
    if state.key != Some(key) {
        body.push_str(&key_xml(key));
        state.key = Some(key);
    }
    if state.meter.as_ref() != Some(&bar.layout.meter) {
        body.push_str(&time_xml(&bar.layout.meter));
        state.meter = Some(bar.layout.meter.clone());
    }
    let one = a.score.one_staff();
    if first && !one {
        body.push_str("        <staves>2</staves>\n");
    }
    let staves: &[Staff] = if one {
        &[Staff::Upper]
    } else {
        &[Staff::Upper, Staff::Lower]
    };
    for &staff in staves {
        let k = usize::from(staff.number() - 1);
        let at_start = clef_tokens(a, b, staff)
            .into_iter()
            .find(|(t, _)| *t == bar.layout.lead)
            .map(|(_, c)| c);
        let default = if staff == Staff::Upper {
            Clef::Treble
        } else {
            Clef::Bass
        };
        let want = at_start.or(if first {
            Some(state.clefs[k].unwrap_or(default))
        } else {
            None
        });
        if let Some(c) = want {
            if first || state.clefs[k] != Some(c) {
                body.push_str(&clef_xml(staff, c));
            }
            state.clefs[k] = Some(c);
        }
        if let Some((_, c)) = clef_tokens(a, b, staff).last() {
            state.clefs[k] = Some(*c);
        }
    }
    state.first_done = true;
    if !body.is_empty() {
        let _ = write!(out, "      <attributes>\n{body}      </attributes>\n");
    }
}

fn voice_number(lane: Lane) -> u8 {
    match lane {
        Lane::Voice(Staff::Upper, n) => n,
        Lane::Voice(Staff::Lower, n) => n + 4,
        _ => 1,
    }
}

// The note values of a length, longest first: one if it is a note value, else plain values.
fn note_values(len: Frac) -> Option<Vec<Frac>> {
    if NoteValue::of(len).is_some() {
        return Some(vec![len]);
    }
    let mut out = Vec::new();
    let mut rest = len;
    while rest > Frac::ZERO && out.len() < 8 {
        let mut v = Frac::int(1);
        while v > rest {
            v = v / 2;
        }
        while v * 2 <= rest {
            v = v * 2;
        }
        out.push(v);
        rest = rest - v;
    }
    rest.is_zero().then_some(out)
}

fn at_leaf(a: &Analysis, leaf: &Leaf, e: &str) -> String {
    format!("{e} (bar {}, {})", a.score.bars[leaf.bar].label, leaf.lane)
}

fn written_type(d: Frac) -> Result<(&'static str, u8), String> {
    let v = NoteValue::of(d).ok_or_else(|| format!("internal: {d} is not a note value"))?;
    let name = type_name(v.log2)
        .ok_or_else(|| format!("the note value {d} is not supported by MusicXML"))?;
    Ok((name, v.dots))
}

fn grace_type_value(g: &crate::model::GraceNote, count: usize) -> u32 {
    let v = g.marks.iter().find_map(|m| match m {
        TokMark::GraceValue(v) => Some(*v),
        _ => None,
    });
    v.unwrap_or(if count == 1 { 8 } else { 16 })
}

fn grace_type(g: &crate::model::GraceNote, count: usize) -> &'static str {
    let zeros = grace_type_value(g, count).trailing_zeros();
    let log2 = -i32::try_from(zeros).expect("a u32 has at most 32 trailing zeros");
    type_name(log2).unwrap_or("eighth")
}

struct NoteCtx<'e> {
    leaf: usize,
    part: Part,
    notes: &'e [Note],
    marks: &'e [TokMark],
    dur: i64,
    type_xml: String,
    grace: Option<String>,
}

fn leaf_items(ctx: &Ctx, id: usize, leaf: &Leaf, items: &mut Vec<Item>) -> Result<(), String> {
    let a = ctx.a;
    let t = ctx.pos(leaf.bar, leaf.start)?;
    let dur = ctx.ticks(leaf.dur)?;
    let voice = voice_number(leaf.lane);
    let staff = Analysis::home_staff(leaf.lane);
    let in_group = !leaf.groups.is_empty();
    let ev = match leaf.tok {
        Tok::Event(ev) => ev,
        Tok::Empty if !in_group => {
            let xml = format!(
                "      <forward>\n        <duration>{dur}</duration>\n        <voice>{voice}</voice>\n        <staff>{}</staff>\n      </forward>\n",
                staff.number()
            );
            items.push(Item {
                t,
                order: 2,
                xml,
                advance: dur,
            });
            return Ok(());
        }
        Tok::Empty => return hidden_rests(a, id, leaf, t, dur, items),
        _ => return Ok(()),
    };
    let mut xml = String::new();
    if let Some(g) = &ev.grace {
        let slash = if g.slash { " slash=\"yes\"" } else { "" };
        for (gi, gn) in g.notes.iter().enumerate() {
            let nc = NoteCtx {
                leaf: id,
                part: Part::Grace(gi),
                notes: &gn.notes,
                marks: &gn.marks,
                dur: 0,
                type_xml: grace_type_xml(gn, g.notes.len()),
                grace: Some(format!("        <grace{slash}/>\n")),
            };
            chord_xml(ctx, leaf, ev, &nc, &mut xml)?;
        }
    }
    let type_xml = if ev.head == Head::BarRest {
        String::new()
    } else {
        let (ty, dots) = written_type(leaf.written).map_err(|e| at_leaf(a, leaf, &e))?;
        let size = size_attr(&ev.marks);
        format!(
            "        <type{size}>{ty}</type>\n{}",
            "        <dot/>\n".repeat(usize::from(dots))
        )
    };
    let nc = NoteCtx {
        leaf: id,
        part: Part::Main,
        notes: ev.notes(),
        marks: &ev.marks,
        dur,
        type_xml,
        grace: None,
    };
    chord_xml(ctx, leaf, ev, &nc, &mut xml)?;
    if let Some(g) = &ev.after {
        for (gi, gn) in g.notes.iter().enumerate() {
            let nc = NoteCtx {
                leaf: id,
                part: Part::After(gi),
                notes: &gn.notes,
                marks: &gn.marks,
                dur: 0,
                type_xml: grace_type_xml(gn, g.notes.len()),
                grace: Some("        <grace steal-time-previous=\"50\"/>\n".to_string()),
            };
            chord_xml(ctx, leaf, ev, &nc, &mut xml)?;
        }
    }
    items.push(Item {
        t,
        order: 1,
        xml,
        advance: dur,
    });
    Ok(())
}

// An empty run in a tuplet is hidden rests: one for each note value of its length.
fn hidden_rests(
    a: &Analysis,
    id: usize,
    leaf: &Leaf,
    t: i64,
    dur: i64,
    items: &mut Vec<Item>,
) -> Result<(), String> {
    let voice = voice_number(leaf.lane);
    let staff = Analysis::home_staff(leaf.lane);
    let not_a_value = || {
        at_leaf(
            a,
            leaf,
            &format!("internal: {} is not a note value", leaf.written),
        )
    };
    let pieces = note_values(leaf.written).ok_or_else(not_a_value)?;
    let n = tuplet_notations(a, id);
    let mut at = t;
    for (i, piece) in pieces.iter().enumerate() {
        let share = Frac::int(dur) * *piece / leaf.written;
        if !share.is_int() {
            return Err(not_a_value());
        }
        let (ty, dots) = written_type(*piece).map_err(|e| at_leaf(a, leaf, &e))?;
        let mut xml = format!(
            "      <note print-object=\"no\">\n        <rest/>\n        <duration>{}</duration>\n",
            share.num()
        );
        let _ = writeln!(
            xml,
            "        <voice>{voice}</voice>\n        <type>{ty}</type>"
        );
        xml.push_str(&"        <dot/>\n".repeat(usize::from(dots)));
        xml.push_str(&time_mod(a, leaf));
        let _ = writeln!(xml, "        <staff>{}</staff>", staff.number());
        let first = i == 0;
        let last = i + 1 == pieces.len();
        let mut ends = String::new();
        for l in n.lines() {
            let starts_here = first && l.contains("\"start\"");
            let stops_here = last && l.contains("\"stop\"");
            if starts_here || stops_here {
                let _ = writeln!(ends, "{l}");
            }
        }
        if !ends.is_empty() {
            let _ = write!(xml, "        <notations>\n{ends}        </notations>\n");
        }
        xml.push_str("      </note>\n");
        items.push(Item {
            t: at,
            order: 1,
            xml,
            advance: share.num(),
        });
        at += share.num();
    }
    Ok(())
}

fn time_mod(a: &Analysis, leaf: &Leaf) -> String {
    if leaf.groups.is_empty() {
        return String::new();
    }
    let (mut actual, mut normal) = (1u32, 1u32);
    for g in &leaf.groups {
        actual *= a.groups[*g].actual;
        normal *= a.groups[*g].normal;
    }
    let unit = match leaf.groups.as_slice() {
        [g] => normal_type(a, &a.groups[*g], leaf),
        _ => String::new(),
    };
    format!(
        "        <time-modification>\n          <actual-notes>{actual}</actual-notes>\n          <normal-notes>{normal}</normal-notes>\n{unit}        </time-modification>\n"
    )
}

// With no `normal-type`, MusicXML takes the type of the note as the unit of the tuplet.
fn normal_type(a: &Analysis, g: &TupletGroup, leaf: &Leaf) -> String {
    let (first, last) = (&a.leaves[g.first_leaf], &a.leaves[g.last_leaf]);
    let unit = (last.start + last.dur - first.start) / i64::from(g.normal);
    let Ok((ty, dots)) = written_type(unit) else {
        return String::new();
    };
    if written_type(leaf.written).ok() == Some((ty, dots)) {
        return String::new();
    }
    let dots = "          <normal-dot/>\n".repeat(dots.into());
    format!("          <normal-type>{ty}</normal-type>\n{dots}")
}

fn tuplet_notations(a: &Analysis, id: usize) -> String {
    let mut s = String::new();
    let leaf = &a.leaves[id];
    for g in &leaf.groups {
        let group = &a.groups[*g];
        let GroupHead::Tuplet { .. } = group.head else {
            continue;
        };
        let number = group.depth + 1;
        if group.first_leaf == id {
            let look = match a.tuplet_look(*g) {
                TupletLook::Default => String::new(),
                TupletLook::Up => " placement=\"above\"".into(),
                TupletLook::Down => " placement=\"below\"".into(),
                TupletLook::Hidden => " bracket=\"no\" show-number=\"none\"".into(),
            };
            let _ = writeln!(
                s,
                "          <tuplet type=\"start\" number=\"{number}\"{look}/>"
            );
        }
        if group.last_leaf == id {
            let _ = writeln!(s, "          <tuplet type=\"stop\" number=\"{number}\"/>");
        }
    }
    s
}

fn tremolo_ornament(a: &Analysis, id: usize) -> Option<String> {
    let leaf = &a.leaves[id];
    let g = leaf.groups.last()?;
    let group = &a.groups[*g];
    let GroupHead::Tremolo { strokes } = group.head else {
        return None;
    };
    let kind = if group.first_leaf == id {
        "start"
    } else {
        "stop"
    };
    Some(format!(
        "            <tremolo type=\"{kind}\">{strokes}</tremolo>\n"
    ))
}

fn stem(a: &Analysis, leaf: &Leaf, marks: &[TokMark]) -> Option<&'static str> {
    if marks.contains(&TokMark::StemUp) {
        return Some("up");
    }
    if marks.contains(&TokMark::StemDown) {
        return Some("down");
    }
    let Lane::Voice(staff, n) = leaf.lane else {
        return None;
    };
    if a.voice_count(leaf.bar, staff) > 1 {
        return Some(if n % 2 == 1 { "up" } else { "down" });
    }
    None
}

fn pitch_xml(p: Pitch) -> String {
    let alter = if p.alter != 0 {
        format!("\n          <alter>{}</alter>", p.alter)
    } else {
        String::new()
    };
    format!(
        "        <pitch>\n          <step>{}</step>{alter}\n          <octave>{}</octave>\n        </pitch>\n",
        p.letter(),
        p.octave
    )
}

fn chord_xml(
    ctx: &Ctx,
    leaf: &Leaf,
    ev: &Event,
    nc: &NoteCtx,
    out: &mut String,
) -> Result<(), String> {
    let a = ctx.a;
    let voice = voice_number(leaf.lane);
    let home = Analysis::home_staff(leaf.lane);
    let is_main = nc.part == Part::Main;
    let rest_count = if nc.notes.is_empty() {
        1
    } else {
        nc.notes.len()
    };
    for i in 0..rest_count {
        let note = nc.notes.get(i);
        let r = NoteRef {
            leaf: nc.leaf,
            part: nc.part,
            note: i,
        };
        let attrs = match note {
            Some(n) => note_attrs(ctx, r, n)?,
            None => String::new(),
        };
        let _ = writeln!(out, "      <note{attrs}>");
        if let Some(g) = &nc.grace {
            out.push_str(g);
        }
        if i > 0 {
            out.push_str("        <chord/>\n");
        }
        let staff = match (note, &ev.head) {
            (Some(n), _) => n.cross_staff().unwrap_or(home),
            (None, Head::Rest(Some(s))) => *s,
            _ => home,
        };
        match note {
            Some(n) => out.push_str(&pitch_xml(n.pitch)),
            None if ev.head == Head::BarRest => out.push_str("        <rest measure=\"yes\"/>\n"),
            None => out.push_str("        <rest/>\n"),
        }
        if is_main {
            let _ = writeln!(out, "        <duration>{}</duration>", nc.dur);
        }
        let tie_start = a.tie_starts.contains_key(&r);
        let tied_end = note.is_some_and(|n| n.tied);
        if tied_end {
            out.push_str("        <tie type=\"stop\"/>\n");
        }
        if tie_start {
            out.push_str("        <tie type=\"start\"/>\n");
        }
        let _ = writeln!(out, "        <voice>{voice}</voice>");
        out.push_str(&nc.type_xml);
        if let Some(n) = note {
            let sign = a.signs.contains(&r) && !n.has(PitchMark::Hide);
            let shown = sign || n.acc != AccDisplay::Default;
            if shown {
                let paren = if n.acc == AccDisplay::Paren {
                    " parentheses=\"yes\""
                } else {
                    ""
                };
                let _ = writeln!(
                    out,
                    "        <accidental{paren}>{}</accidental>",
                    acc_name(n.pitch.alter)
                );
            }
        }
        if is_main {
            out.push_str(&time_mod(a, leaf));
        }
        if let Some(s) =
            stem(a, leaf, nc.marks).filter(|_| ev.head != Head::BarRest && note.is_some())
        {
            let _ = writeln!(out, "        <stem>{s}</stem>");
        }
        let _ = writeln!(out, "        <staff>{}</staff>", staff.number());
        if i == 0 {
            out.push_str(&match nc.part {
                Part::Main => beams(a, nc.leaf),
                Part::Grace(k) => ev
                    .grace
                    .as_ref()
                    .map(|g| grace_beams(g, k))
                    .unwrap_or_default(),
                Part::After(k) => ev
                    .after
                    .as_ref()
                    .map(|g| grace_beams(g, k))
                    .unwrap_or_default(),
            });
        }
        let position = if i == 0 {
            ChordNote::First
        } else {
            ChordNote::Other
        };
        let notations = notations(ctx, nc, note, r, position);
        if !notations.is_empty() {
            let _ = write!(
                out,
                "        <notations>\n{notations}        </notations>\n"
            );
        }
        out.push_str("      </note>\n");
    }
    Ok(())
}

fn note_attrs(ctx: &Ctx, r: NoteRef, n: &Note) -> Result<String, String> {
    let a = ctx.a;
    let mut attrs = String::new();
    let vel = a.velocity(r);
    let leaf = &a.leaves[r.leaf];
    let reader = a.reader_level_at(Analysis::home_staff(leaf.lane), leaf.bar, leaf.start);
    if vel.note != reader {
        let _ = write!(
            attrs,
            " dynamics=\"{}\"",
            num(vel.note / MUSICXML_VELOCITY_BASE * 100.0)
        );
    }
    for m in &n.marks {
        let (name, v) = match m {
            PitchMark::Attack(v) => ("attack", v),
            PitchMark::Release(v) => ("release", v),
            _ => continue,
        };
        let div = i32::try_from(ctx.div)
            .map_err(|_| format!("internal: {} divisions do not fit in i32", ctx.div))?;
        let _ = write!(
            attrs,
            " {name}=\"{}\"",
            num(v * f64::from(div) / TICKS_PER_QUARTER)
        );
    }
    if n.hidden {
        attrs.push_str(" print-object=\"no\"");
    }
    Ok(attrs)
}

#[derive(Clone, Copy)]
struct BeamSlot {
    begin: bool,
    end: bool,
    split: bool,
    count: i32,
}

impl BeamSlot {
    fn of(marks: &[TokMark], written: Frac) -> BeamSlot {
        BeamSlot {
            begin: marks.contains(&TokMark::BeamBegin),
            end: marks.contains(&TokMark::BeamEnd),
            split: marks.contains(&TokMark::BeamBreak),
            count: NoteValue::of(written).map_or(0, |v| -v.log2 - 2),
        }
    }
}

// The group runs from `:bb` to `:be`; deeper levels follow the note values and `:b1`.
fn beam_xml(slots: &[BeamSlot], k: usize) -> String {
    let Some(begin) = slots[..=k].iter().rposition(|s| s.begin) else {
        return String::new();
    };
    if slots[begin..k].iter().any(|s| s.end) {
        return String::new();
    }
    let Some(len) = slots[k..].iter().position(|s| s.end) else {
        return String::new();
    };
    let group = &slots[begin..=k + len];
    let j = k - begin;
    let last = group.len() - 1;
    let level1 = match j {
        0 => "begin",
        _ if j == last => "end",
        _ => "continue",
    };
    let mut out = format!(
        "        <beam number=\"1\">{level1}</beam>
"
    );
    for level in 2..=group[j].count {
        let left = j > 0 && group[j - 1].count >= level && !group[j - 1].split;
        let right = j < last && group[j + 1].count >= level && !group[j].split;
        let state = match (left, right) {
            (false, true) => "begin",
            (true, true) => "continue",
            (true, false) => "end",
            (false, false) if j == 0 => "forward hook",
            (false, false) => "backward hook",
        };
        let _ = writeln!(out, "        <beam number=\"{level}\">{state}</beam>");
    }
    out
}

fn beams(a: &Analysis, id: usize) -> String {
    let leaf = &a.leaves[id];
    let Some(ids) = a.lane_leaves.get(&leaf.lane) else {
        return String::new();
    };
    let in_bar: Vec<usize> = ids
        .iter()
        .copied()
        .filter(|i| a.leaves[*i].bar == leaf.bar && a.leaves[*i].event().is_some())
        .collect();
    let Some(k) = in_bar.iter().position(|i| *i == id) else {
        return String::new();
    };
    let slots: Vec<BeamSlot> = in_bar
        .iter()
        .map(|i| {
            let l = &a.leaves[*i];
            BeamSlot::of(l.event().map_or(&[], |e| e.marks.as_slice()), l.written)
        })
        .collect();
    beam_xml(&slots, k)
}

fn grace_beams(g: &crate::model::Grace, k: usize) -> String {
    let slots: Vec<BeamSlot> = g
        .notes
        .iter()
        .map(|gn| {
            let v = grace_type_value(gn, g.notes.len());
            BeamSlot::of(&gn.marks, Frac::new(1, i64::from(v)))
        })
        .collect();
    beam_xml(&slots, k)
}

fn ornament_xml(kind: OrnamentKind) -> &'static str {
    match kind {
        OrnamentKind::Trill | OrnamentKind::TrillLine => "<trill-mark/>",
        OrnamentKind::Mordent => "<mordent/>",
        OrnamentKind::Pralltriller => "<inverted-mordent/>",
        OrnamentKind::LongMordent => "<mordent long=\"yes\"/>",
        OrnamentKind::LongPralltriller => "<inverted-mordent long=\"yes\"/>",
        OrnamentKind::Turn => "<turn/>",
        OrnamentKind::InvertedTurn => "<inverted-turn/>",
        OrnamentKind::DelayedTurn => "<delayed-turn/>",
        OrnamentKind::DelayedInvertedTurn => "<delayed-inverted-turn/>",
    }
}

fn tie_chain_end(a: &Analysis, mut r: NoteRef) -> NoteRef {
    let mut guard = 0;
    while let Some(next) = a.tie_starts.get(&r) {
        r = *next;
        guard += 1;
        if guard > 10_000 {
            break;
        }
    }
    r
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChordNote {
    First,
    Other,
}

fn notations(
    ctx: &Ctx,
    nc: &NoteCtx,
    note: Option<&Note>,
    r: NoteRef,
    position: ChordNote,
) -> String {
    let a = ctx.a;
    let first = position == ChordNote::First;
    let mut s = String::new();
    if let Some(n) = note {
        s.push_str(&ties_and_glissandi(a, n, r));
    }
    if first {
        for sl in &a.slurs {
            if sl.end == r {
                let _ = writeln!(
                    s,
                    "          <slur type=\"stop\" number=\"{}\"/>",
                    sl.number
                );
            }
        }
        for sl in &a.slurs {
            if sl.start == r {
                let place = match sl.above {
                    Some(Place::Above) => " placement=\"above\"",
                    Some(Place::Below) => " placement=\"below\"",
                    None => "",
                };
                let _ = writeln!(
                    s,
                    "          <slur type=\"start\" number=\"{}\"{place}/>",
                    sl.number
                );
            }
        }
        if nc.part == Part::Main {
            s.push_str(&tuplet_notations(a, nc.leaf));
        }
        let orn = ornaments(a, nc, r);
        if !orn.is_empty() {
            let _ = write!(s, "          <ornaments>\n{orn}          </ornaments>\n");
        }
    }
    if let Some(n) = note {
        let fingers: Vec<u8> = n
            .marks
            .iter()
            .filter_map(|m| {
                if let PitchMark::Finger(f) = m {
                    Some(*f)
                } else {
                    None
                }
            })
            .collect();
        if !fingers.is_empty() {
            s.push_str("          <technical>\n");
            for f in fingers {
                let _ = writeln!(s, "            <fingering>{f}</fingering>");
            }
            s.push_str("          </technical>\n");
        }
    }
    if first {
        s.push_str(&articulations(a, nc, r));
        for m in nc.marks {
            if let TokMark::Fermata { inverted } = m {
                let ty = if *inverted { "inverted" } else { "upright" };
                let _ = writeln!(s, "          <fermata type=\"{ty}\"/>");
            }
        }
    }
    for m in nc.marks {
        if let TokMark::Arp(k) = m {
            let attrs = match k {
                ArpKind::Plain => "",
                ArpKind::Up => " direction=\"up\"",
                ArpKind::Down => " direction=\"down\"",
                ArpKind::CrossStaff => " number=\"1\"",
            };
            let _ = writeln!(s, "          <arpeggiate{attrs}/>");
        }
    }
    s
}

fn ties_and_glissandi(a: &Analysis, note: &Note, r: NoteRef) -> String {
    let mut s = String::new();
    if note.tied {
        s.push_str("          <tied type=\"stop\"/>\n");
    }
    if a.tie_starts.contains_key(&r) {
        let o = if note.has(PitchMark::TieUp) {
            " orientation=\"over\""
        } else if note.has(PitchMark::TieDown) {
            " orientation=\"under\""
        } else {
            ""
        };
        let _ = writeln!(s, "          <tied type=\"start\"{o}/>");
    }
    if note.has(PitchMark::LetRing) {
        s.push_str("          <tied type=\"let-ring\"/>\n");
    }
    for g in a.glissandi.iter().filter(|g| g.end == r) {
        let (tag, line) = gliss_look(g.line);
        let _ = writeln!(
            s,
            "          <{tag} line-type=\"{line}\" number=\"{}\" type=\"stop\"/>",
            g.number
        );
    }
    for g in a.glissandi.iter().filter(|g| g.start == r) {
        let (tag, line) = gliss_look(g.line);
        let text = matches!(g.line, GlissLine::WavyText | GlissLine::StraightText);
        let body = if text { "gliss." } else { "" };
        let _ = writeln!(
            s,
            "          <{tag} line-type=\"{line}\" number=\"{}\" type=\"start\">{body}</{tag}>",
            g.number
        );
    }
    s
}

fn ornaments(a: &Analysis, nc: &NoteCtx, r: NoteRef) -> String {
    let mut orn = String::new();
    // Overlapping trill lines of the two staves need two numbers.
    let wavy = match Analysis::home_staff(a.leaves[nc.leaf].lane) {
        Staff::Upper => 1,
        Staff::Lower => 2,
    };
    for m in nc.marks {
        match m {
            TokMark::Ornament { kind, above, below } => {
                let _ = writeln!(orn, "            {}", ornament_xml(*kind));
                if *kind == OrnamentKind::TrillLine {
                    let _ = writeln!(
                        orn,
                        "            <wavy-line type=\"start\" number=\"{wavy}\"/>"
                    );
                }
                if let Some(x) = above {
                    let _ = writeln!(
                        orn,
                        "            <accidental-mark placement=\"above\">{}</accidental-mark>",
                        acc_name(*x)
                    );
                }
                if let Some(x) = below {
                    let _ = writeln!(
                        orn,
                        "            <accidental-mark placement=\"below\">{}</accidental-mark>",
                        acc_name(*x)
                    );
                }
            }
            TokMark::Tremolo(n) => {
                let _ = writeln!(orn, "            <tremolo type=\"single\">{n}</tremolo>");
            }
            _ => {}
        }
    }
    if nc.part == Part::Main {
        if let Some(t) = tremolo_ornament(a, nc.leaf) {
            orn.push_str(&t);
        }
    }
    if a.trill_notes.iter().any(|(start, _)| *start == r) {
        let _ = writeln!(
            orn,
            "            <wavy-line type=\"start\" number=\"{wavy}\"/>"
        );
    }
    let lane_line_ends = a.trill_notes.iter().any(|(_, end)| *end == r);
    if wavy_line_ends_here(a, r) || lane_line_ends {
        let _ = writeln!(
            orn,
            "            <wavy-line type=\"stop\" number=\"{wavy}\"/>"
        );
    }
    orn
}

fn articulations(a: &Analysis, nc: &NoteCtx, r: NoteRef) -> String {
    let mut arts: Vec<&str> = nc
        .marks
        .iter()
        .filter_map(|m| match m {
            TokMark::Art(x) => Some(match x {
                Articulation::Staccato => "<staccato/>",
                Articulation::Staccatissimo => "<staccatissimo/>",
                Articulation::Tenuto => "<tenuto/>",
                Articulation::Accent => "<accent/>",
                Articulation::Marcato => "<strong-accent/>",
                Articulation::MarcatoDown => "<strong-accent type=\"down\"/>",
            }),
            _ => None,
        })
        .collect();
    let pause = a.pauses.get(&nc.leaf).filter(|_| r.part == Part::Main);
    match pause {
        Some(PhraseMark::Breath) => arts.push("<breath-mark/>"),
        Some(PhraseMark::Caesura) => arts.push("<caesura/>"),
        _ => {}
    }
    if arts.is_empty() {
        return String::new();
    }
    let mut s = String::from("          <articulations>\n");
    for x in arts {
        let _ = writeln!(s, "            {x}");
    }
    s.push_str("          </articulations>\n");
    s
}

fn wavy_line_ends_here(a: &Analysis, r: NoteRef) -> bool {
    a.leaves.iter().enumerate().any(|(id, l)| {
        let Some(ev) = l.event() else { return false };
        let has_line = ev.marks.iter().any(|m| {
            matches!(
                m,
                TokMark::Ornament {
                    kind: OrnamentKind::TrillLine,
                    ..
                }
            )
        });
        if !has_line {
            return false;
        }
        let start = NoteRef {
            leaf: id,
            part: Part::Main,
            note: 0,
        };
        tie_chain_end(a, start) == r
    })
}

type Direction = (Item, Staff);

fn size_attr(marks: &[TokMark]) -> &'static str {
    if marks.contains(&TokMark::Small) {
        " size=\"cue\""
    } else {
        ""
    }
}

fn grace_type_xml(gn: &GraceNote, count: usize) -> String {
    let size = size_attr(&gn.marks);
    format!("        <type{size}>{}</type>\n", grace_type(gn, count))
}

fn placement(p: Option<Place>, default: Place) -> &'static str {
    match p.unwrap_or(default) {
        Place::Above => "above",
        Place::Below => "below",
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Layer {
    Staff,
    System,
}

fn direction(
    t: i64,
    staff: Staff,
    place: &str,
    types: &[String],
    sound: Option<String>,
    layer: Layer,
) -> Item {
    let system = match layer {
        Layer::Staff => "",
        Layer::System => " system=\"only-top\"",
    };
    let mut xml = format!("      <direction placement=\"{place}\"{system}>\n");
    for ty in types {
        let _ = write!(
            xml,
            "        <direction-type>\n          {ty}\n        </direction-type>\n"
        );
    }
    let _ = writeln!(xml, "        <staff>{}</staff>", staff.number());
    if let Some(s) = sound {
        let _ = writeln!(xml, "        <sound {s}/>");
    }
    xml.push_str("      </direction>\n");
    Item {
        t,
        order: 0,
        xml,
        advance: 0,
    }
}

// The lowest number that no open span of the same MusicXML element has. A span that ends where
// another starts frees its number only in its own lane: another lane can be written first.
fn span_number(a: &Analysis, span: &Span) -> usize {
    let element = |t: &Tok| match t {
        Tok::Hairpin(..) => Some(0),
        Tok::Text(x) if x.ext => Some(1),
        Tok::Tempo(x) if x.ext => Some(1),
        Tok::Ottava(_) => Some(2),
        _ => None,
    };
    let Some(kind) = element(span.tok) else {
        return 1;
    };
    let mut same: Vec<&Span> = a
        .spans
        .iter()
        .filter(|s| element(s.tok) == Some(kind))
        .collect();
    same.sort_by_key(|s| (a.abs(s.start.0, s.start.1), s.lane));
    let mut open: Vec<(Frac, Lane, usize)> = Vec::new();
    for s in same {
        let start = a.abs(s.start.0, s.start.1);
        open.retain(|(end, lane, _)| {
            let ends_here_in_other_lane = *end == start && *lane != s.lane;
            *end > start || ends_here_in_other_lane
        });
        let n = (1..)
            .find(|n| open.iter().all(|(_, _, m)| m != n))
            .unwrap_or(1);
        if s.lane == span.lane && s.start == span.start {
            return n;
        }
        open.push((a.abs(s.end.0, s.end.1), s.lane, n));
    }
    1
}

fn directions(ctx: &Ctx, b: usize) -> Result<Vec<Direction>, String> {
    let a = ctx.a;
    let bar = &a.score.bars[b];
    let mut out: Vec<Direction> = Vec::new();
    let first_bar = *ctx.range.start();
    let last_bar = *ctx.range.end();
    let bar_end = a.bar_end(b);
    // MusicXML readers pair a stop with an open span in document order: at one time, the stops
    // of ending spans go before the starts of new ones.
    let mut starts: Vec<(Direction, bool)> = Vec::new();
    for span in &a.spans {
        let carried_in = b == first_bar && span.start.0 < b && span.end.0 >= b;
        let entry = if span.start.0 == b {
            Some(SpanEntry::Starts)
        } else if carried_in {
            Some(SpanEntry::CarriedIn)
        } else {
            None
        };
        let runs_past_excerpt = b == last_bar && span.end.0 > b && span.start.0 <= b;
        let ends_here = span.end.0 == b || runs_past_excerpt;
        if entry.is_none() && !ends_here {
            continue;
        }
        let staff = span.lane.staff().unwrap_or(Staff::Upper);
        let start_t = match entry {
            Some(SpanEntry::CarriedIn) => bar.layout.lead,
            _ => span.start.1,
        };
        let end_t = if span.end.0 == b { span.end.1 } else { bar_end };
        let n = span_number(a, span);
        if let Some(entry) = entry {
            if let Some(d) = span_start(ctx, b, span, staff, ctx.pos(b, start_t)?, n, entry) {
                let later_dyn_lane = matches!(span.lane, Lane::Mark(_, MarkKind::Dyn, n) if n > 1);
                starts.push(((d, staff), later_dyn_lane));
            }
        }
        if ends_here {
            if let Some(d) = span_stop(span, staff, ctx.pos(b, end_t)?, n) {
                out.push((d, staff));
            }
        }
    }
    // MusicXML readers keep the level of the last mark at a time; the first dynamics lane sets
    // the level in pno, so its mark goes last.
    starts.sort_by_key(|((d, _), later_dyn_lane)| (d.t, !later_dyn_lane));
    out.extend(starts.into_iter().map(|(d, _)| d));
    if ctx.pedal == PedalMarks::Show {
        out.extend(pedal_directions(ctx, b)?);
    }
    for at in &bar.attrs {
        match at {
            Attr::Sign(s) => {
                let (ty, sound) = match s {
                    Sign::Segno => ("<segno/>", "segno=\"segno\""),
                    Sign::Coda => ("<coda/>", "coda=\"coda\""),
                };
                out.push((
                    direction(
                        0,
                        Staff::Upper,
                        "above",
                        &[ty.to_string()],
                        Some(sound.to_string()),
                        Layer::Staff,
                    ),
                    Staff::Upper,
                ));
            }
            Attr::Jump(j) => {
                let (text, sound) = match j {
                    Jump::Dc => ("D.C.", "dacapo=\"yes\""),
                    Jump::Ds => ("D.S.", "dalsegno=\"segno\""),
                    Jump::DcFine => ("D.C. al Fine", "dacapo=\"yes\""),
                    Jump::DsFine => ("D.S. al Fine", "dalsegno=\"segno\""),
                    Jump::DcCoda => ("D.C. al Coda", "dacapo=\"yes\""),
                    Jump::DsCoda => ("D.S. al Coda", "dalsegno=\"segno\""),
                    Jump::ToCoda => ("To Coda", "tocoda=\"coda\""),
                    Jump::Fine => ("Fine", "fine=\"yes\""),
                };
                let ty = format!("<words>{text}</words>");
                out.push((
                    direction(
                        ctx.pos(b, bar_end)?,
                        Staff::Upper,
                        "above",
                        &[ty],
                        Some(sound.to_string()),
                        Layer::Staff,
                    ),
                    Staff::Upper,
                ));
            }
            _ => {}
        }
    }
    Ok(out)
}

// The pedal marks of bar `b`, below the lower staff (the only staff with staves=1).
fn pedal_directions(ctx: &Ctx, b: usize) -> Result<Vec<Direction>, String> {
    let a = ctx.a;
    let staff = if a.score.one_staff() {
        Staff::Upper
    } else {
        Staff::Lower
    };
    let first_bar = *ctx.range.start();
    let last_bar = *ctx.range.end();
    let bar_end = a.bar_end(b);
    let mut out: Vec<Direction> = Vec::new();
    let style = match a.pedal_style {
        PedalStyle::Line => "line=\"yes\" sign=\"no\"",
        PedalStyle::Sign => "line=\"no\" sign=\"yes\"",
    };
    let held_in = pedal_held_before(a, first_bar);
    if b == first_bar && held_in {
        out.push((
            direction(
                0,
                staff,
                "below",
                &[format!("<pedal type=\"start\" {style}/>")],
                None,
                Layer::Staff,
            ),
            staff,
        ));
    }
    for p in a.pedals.iter().filter(|p| p.bar == b) {
        let kind = match p.action {
            PedalAction::Start => "start",
            PedalAction::Stop => "stop",
            PedalAction::Change => "change",
        };
        let mut d = direction(
            ctx.pos(b, p.at)?,
            staff,
            "below",
            &[format!("<pedal type=\"{kind}\" {style}/>")],
            None,
            Layer::Staff,
        );
        d.order = u8::from(p.action != PedalAction::Stop);
        out.push((d, staff));
    }
    if b == last_bar && b + 1 < a.score.bars.len() && pedal_held_before(a, b + 1) {
        out.push((
            direction(
                ctx.pos(b, bar_end)?,
                staff,
                "below",
                &[format!("<pedal type=\"stop\" {style}/>")],
                None,
                Layer::Staff,
            ),
            staff,
        ));
    }
    Ok(out)
}

fn pedal_held_before(a: &Analysis, b: usize) -> bool {
    let mut held = false;
    for p in a.pedals.iter().filter(|p| p.bar < b) {
        held = p.action != PedalAction::Stop;
    }
    held
}

#[derive(Clone, Copy)]
enum MetronomeLook {
    Shown { parens: bool },
    Hidden,
}

fn metronome_xml(m: &crate::model::Metronome, look: MetronomeLook) -> String {
    let unit = match m.unit {
        'w' => "whole",
        'h' => "half",
        'e' => "eighth",
        's' => "16th",
        _ => "quarter",
    };
    let dot = if m.dotted { "<beat-unit-dot/>" } else { "" };
    let (p, print) = match look {
        MetronomeLook::Shown { parens: true } => ("yes", ""),
        MetronomeLook::Shown { parens: false } => ("no", ""),
        MetronomeLook::Hidden => ("no", " print-object=\"no\""),
    };
    format!(
        "<metronome parentheses=\"{p}\"{print}><beat-unit>{unit}</beat-unit>{dot}<per-minute>{}</per-minute></metronome>",
        num(m.bpm)
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpanEntry {
    Starts,
    CarriedIn,
}

fn span_start(
    ctx: &Ctx,
    b: usize,
    span: &Span,
    staff: Staff,
    t: i64,
    n: usize,
    entry: SpanEntry,
) -> Option<Item> {
    let a = ctx.a;
    let starts = entry == SpanEntry::Starts;
    match span.tok {
        Tok::Dyn(d) if starts => Some(dyn_direction(a, b, span, d, staff, t)),
        Tok::DynText(d) if starts => {
            let types = [format!(
                "<dynamics><other-dynamics>{}</other-dynamics></dynamics>",
                esc(&d.text)
            )];
            let sound = d
                .level
                .map(|v| format!("dynamics=\"{}\"", num(v / MUSICXML_VELOCITY_BASE * 100.0)));
            Some(direction(
                t,
                staff,
                placement(d.place, Place::Below),
                &types,
                sound,
                Layer::Staff,
            ))
        }
        Tok::Hairpin(h, _) => {
            let ty = match h {
                Hairpin::Cresc => "crescendo",
                Hairpin::Dim => "diminuendo",
            };
            Some(direction(
                t,
                staff,
                span_place(span.tok, staff),
                &[format!("<wedge type=\"{ty}\" number=\"{n}\"/>")],
                None,
                Layer::Staff,
            ))
        }
        Tok::Text(text) => {
            let mut types = Vec::new();
            if starts {
                let body = esc(&text.text);
                types.push(match text.frame {
                    TextFrame::None => format!("<words>{body}</words>"),
                    TextFrame::Box => format!("<words enclosure=\"rectangle\">{body}</words>"),
                    TextFrame::Rehearsal => format!("<rehearsal>{body}</rehearsal>"),
                });
            }
            if text.ext {
                types.push(format!("<dashes type=\"start\" number=\"{n}\"/>"));
            }
            (!types.is_empty()).then(|| {
                direction(
                    t,
                    staff,
                    span_place(span.tok, staff),
                    &types,
                    None,
                    Layer::Staff,
                )
            })
        }
        Tok::Tempo(tempo) => tempo_direction(tempo, t, n, entry),
        Tok::Ottava(o) => {
            let (ty, size) = match o {
                Ottava::Up8 => ("down", 8),
                Ottava::Down8 => ("up", 8),
                Ottava::Up15 => ("down", 15),
                Ottava::Down15 => ("up", 15),
            };
            Some(direction(
                t,
                staff,
                span_place(span.tok, staff),
                &[format!(
                    "<octave-shift type=\"{ty}\" size=\"{size}\" number=\"{n}\"/>"
                )],
                None,
                Layer::Staff,
            ))
        }
        Tok::Clef(c) if span.start.1 != ctx.a.score.bars[b].layout.lead => Some(Item {
            t,
            order: 0,
            xml: format!(
                "      <attributes>\n{}      </attributes>\n",
                clef_xml(staff, *c)
            ),
            advance: 0,
        }),
        _ => None,
    }
}

fn dyn_direction(a: &Analysis, b: usize, span: &Span, d: &Dyn, staff: Staff, t: i64) -> Item {
    let mut types = Vec::new();
    if let Some(x) = &d.before {
        types.push(format!("<words>{}</words>", esc(x)));
    }
    types.push(format!("<dynamics><{}/></dynamics>", d.mark.name()));
    if let Some(x) = &d.after {
        types.push(format!("<words>{}</words>", esc(x)));
    }
    let vel = match span.lane {
        Lane::Mark(_, _, 1) => a.mark_velocity(b, d.mark),
        _ => a.level_at(staff, b, span.start.1),
    };
    let sound = format!("dynamics=\"{}\"", num(vel / MUSICXML_VELOCITY_BASE * 100.0));
    direction(
        t,
        staff,
        placement(d.place, Place::Below),
        &types,
        Some(sound),
        Layer::Staff,
    )
}

fn tempo_direction(tempo: &Tempo, t: i64, n: usize, entry: SpanEntry) -> Option<Item> {
    let mut types = Vec::new();
    let mut sound = None;
    if entry == SpanEntry::Starts {
        if let Some(text) = &tempo.text {
            types.push(format!("<words>{}</words>", esc(text)));
        }
        match &tempo.mark {
            Some(TempoMark::Shown { metronome, parens }) => {
                types.push(metronome_xml(
                    metronome,
                    MetronomeLook::Shown { parens: *parens },
                ));
                sound = Some(format!(
                    "tempo=\"{}\"",
                    num(metronome.quarters_per_minute())
                ));
            }
            Some(TempoMark::Hidden(metronome)) => {
                types.push(metronome_xml(metronome, MetronomeLook::Hidden));
                sound = Some(format!(
                    "tempo=\"{}\"",
                    num(metronome.quarters_per_minute())
                ));
            }
            Some(TempoMark::Sound(bpm)) => {
                if tempo.text.is_none() {
                    let m = crate::model::Metronome {
                        unit: 'q',
                        dotted: false,
                        bpm: *bpm,
                    };
                    types.push(metronome_xml(&m, MetronomeLook::Hidden));
                }
                sound = Some(format!("tempo=\"{bpm}\""));
            }
            None => {}
        }
    }
    if tempo.ext {
        types.push(format!("<dashes type=\"start\" number=\"{n}\"/>"));
    }
    if types.is_empty() {
        return None;
    }
    Some(direction(
        t,
        Staff::Upper,
        placement(tempo.place, Place::Above),
        &types,
        sound,
        Layer::System,
    ))
}

fn span_stop(span: &Span, staff: Staff, t: i64, n: usize) -> Option<Item> {
    let ty = match span.tok {
        Tok::Hairpin(..) => format!("<wedge type=\"stop\" number=\"{n}\"/>"),
        Tok::Text(x) if x.ext => format!("<dashes type=\"stop\" number=\"{n}\"/>"),
        Tok::Tempo(x) if x.ext => format!("<dashes type=\"stop\" number=\"{n}\"/>"),
        Tok::Ottava(o) => {
            let size = match o {
                Ottava::Up8 | Ottava::Down8 => 8,
                Ottava::Up15 | Ottava::Down15 => 15,
            };
            format!("<octave-shift type=\"stop\" size=\"{size}\" number=\"{n}\"/>")
        }
        _ => return None,
    };
    Some(direction(
        t,
        staff,
        span_place(span.tok, staff),
        &[ty],
        None,
        Layer::Staff,
    ))
}

fn span_place(tok: &Tok, staff: Staff) -> &'static str {
    let staff_place = if staff == Staff::Upper {
        Place::Above
    } else {
        Place::Below
    };
    match tok {
        Tok::Hairpin(_, p) => placement(*p, Place::Below),
        Tok::Text(x) => placement(x.place, staff_place),
        Tok::Tempo(x) => placement(x.place, Place::Above),
        Tok::Ottava(Ottava::Up8 | Ottava::Up15) => "above",
        _ => "below",
    }
}
