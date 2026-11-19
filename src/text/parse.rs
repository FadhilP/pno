use crate::diag::Diag;
use crate::frac::Frac;
use crate::model::{
    Attr, Bar, BarLayout, El, ElKind, Event, GroupHead, Head, Lane, Line, Meter, Score, Tok,
    TupletLook,
};
use crate::text::attr::{parse_attr, split_attrs};
use crate::text::token::{Word, parse_word};
use crate::text::write::cell_counts;

#[derive(Debug)]
enum Raw {
    Bar,
    Word(String),
    Group(GroupHead, Vec<Raw>),
}

struct SrcLine {
    src: usize,
    label: String,
    body: Body,
}

enum Body {
    Attrs(Vec<Attr>),
    Grid(Lane, Vec<Raw>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Nesting {
    Top,
    InGroup,
}

pub struct Parsed {
    pub score: Score,
    pub diags: Vec<Diag>,
}

pub fn parse(text: &str) -> Parsed {
    let mut diags = Vec::new();
    let mut header = Vec::new();
    let mut lines: Vec<SrcLine> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let src = i + 1;
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('@') {
            if !lines.is_empty() {
                diags.push(Diag::error(
                    src,
                    "",
                    "a header line (no bar number) must come before the first bar",
                ));
                continue;
            }
            match header_attr(rest) {
                Ok(a) => header.push(a),
                Err(e) => diags.push(Diag::error(src, "header", e)),
            }
            continue;
        }
        match source_line(line) {
            Ok((label, body)) => lines.push(SrcLine { src, label, body }),
            Err((at, e)) => diags.push(Diag::error(src, at, e)),
        }
    }
    if !header.iter().any(|a| matches!(a, Attr::Format(1))) {
        diags.push(Diag::error(
            1,
            "header",
            "the first line of a file is `@ format=1`",
        ));
    }
    let bars = assemble(lines, &header, &mut diags);
    Parsed {
        score: Score { header, bars },
        diags,
    }
}

fn header_attr(rest: &str) -> Result<Attr, String> {
    let pairs = split_attrs(rest)?;
    let [(name, value)] = pairs.as_slice() else {
        return Err("a header line has one attribute".into());
    };
    let attr = parse_attr(name, value)?;
    if attr.bar_only() {
        return Err(format!("{name}= needs a bar number"));
    }
    Ok(attr)
}

fn source_line(line: &str) -> Result<(String, Body), (String, String)> {
    let (label, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let label_ok = {
        let digits = label.strip_suffix('b').unwrap_or(label);
        !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
    };
    if !label_ok {
        return Err((String::new(), format!("`{label}` is not a bar number")));
    }
    let rest = rest.trim_start();
    let (lane_name, body) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let at = format!("{label} {lane_name}");
    if lane_name == "@" {
        let attrs = split_attrs(body).map_err(|e| (at.clone(), e))?;
        let mut out = Vec::new();
        for (name, value) in attrs {
            let attr = parse_attr(&name, &value).map_err(|e| (at.clone(), e))?;
            if attr.header_only() {
                return Err((
                    at,
                    format!("{name}= is a header attribute; write it with no bar number"),
                ));
            }
            out.push(attr);
        }
        return Ok((label.to_string(), Body::Attrs(out)));
    }
    let Some(lane) = Lane::parse(lane_name) else {
        return Err((at, format!("unknown lane `{lane_name}`")));
    };
    let raw = lex(body.trim()).map_err(|e| (at, e))?;
    Ok((label.to_string(), Body::Grid(lane, raw)))
}

fn lex(s: &str) -> Result<Vec<Raw>, String> {
    if !s.starts_with('|') || !s.ends_with('|') || s.len() < 2 {
        return Err("a grid line is `| cell | cell |`".into());
    }
    let mut chars: Vec<char> = s.chars().collect();
    chars.push(' ');
    let mut i = 0;
    let items = lex_items(&chars, &mut i, Nesting::Top)?;
    Ok(items)
}

fn lex_items(c: &[char], i: &mut usize, nesting: Nesting) -> Result<Vec<Raw>, String> {
    let mut items = Vec::new();
    loop {
        while *i < c.len() && c[*i].is_whitespace() {
            *i += 1;
        }
        let Some(&ch) = c.get(*i) else {
            if nesting == Nesting::InGroup {
                return Err("a group has no closing `]`".into());
            }
            return Ok(items);
        };
        match ch {
            ']' if nesting == Nesting::InGroup => {
                *i += 1;
                return Ok(items);
            }
            ']' => return Err("`]` with no group".into()),
            '|' => {
                *i += 1;
                items.push(Raw::Bar);
            }
            _ => {
                if let Some(head) = group_head(c, i)? {
                    let inner = lex_items(c, i, Nesting::InGroup)?;
                    items.push(Raw::Group(head, inner));
                    if c.get(*i)
                        .is_some_and(|ch| !ch.is_whitespace() && *ch != '|' && *ch != ']')
                    {
                        return Err("a group ends with `]` and a space".into());
                    }
                } else {
                    items.push(Raw::Word(word(c, i)?));
                }
            }
        }
    }
}

fn word(c: &[char], i: &mut usize) -> Result<String, String> {
    let mut out = String::new();
    let mut in_quote = false;
    let mut braces = 0;
    while let Some(&ch) = c.get(*i) {
        if in_quote {
            out.push(ch);
            *i += 1;
            if ch == '\\' {
                if let Some(&n) = c.get(*i) {
                    out.push(n);
                    *i += 1;
                }
            } else if ch == '"' {
                in_quote = false;
            }
            continue;
        }
        let ends = ch.is_whitespace() || ch == '|' || ch == ']';
        if ends && braces == 0 {
            break;
        }
        match ch {
            '"' => in_quote = true,
            '{' => braces += 1,
            '}' => braces -= 1,
            _ => {}
        }
        out.push(ch);
        *i += 1;
    }
    if in_quote {
        return Err(format!("`{out}`: the text has no closing quote"));
    }
    if braces != 0 {
        return Err(format!("`{out}`: a grace group has no closing `}}`"));
    }
    Ok(out)
}

fn group_head(c: &[char], i: &mut usize) -> Result<Option<GroupHead>, String> {
    let start = *i;
    let mut j = *i;
    let tremolo = c.get(j) == Some(&'t');
    if tremolo {
        j += 1;
    }
    let digits = |j: &mut usize| {
        let s = *j;
        while c.get(*j).is_some_and(char::is_ascii_digit) {
            *j += 1;
        }
        c[s..*j].iter().collect::<String>()
    };
    let num = digits(&mut j);
    if num.is_empty() {
        return Ok(None);
    }
    let mut ratio = None;
    if !tremolo && c.get(j) == Some(&':') {
        j += 1;
        let b = digits(&mut j);
        if b.is_empty() {
            return Ok(None);
        }
        ratio = Some(b);
    }
    let look = match c.get(j) {
        Some('u') if !tremolo => TupletLook::Up,
        Some('d') if !tremolo => TupletLook::Down,
        Some('h') if !tremolo => TupletLook::Hidden,
        _ => TupletLook::Default,
    };
    if look != TupletLook::Default {
        j += 1;
    }
    if c.get(j) != Some(&'[') {
        *i = start;
        return Ok(None);
    }
    *i = j + 1;
    let n: u32 = num.parse().map_err(|_| format!("`{num}` is too large"))?;
    let head = if tremolo {
        let strokes = u8::try_from(n)
            .ok()
            .filter(|s| (1..=4).contains(s))
            .ok_or("a tremolo has 1 to 4 strokes")?;
        GroupHead::Tremolo { strokes }
    } else {
        let ratio = match ratio {
            Some(b) => Some(b.parse().map_err(|_| format!("`{b}` is too large"))?),
            None => None,
        };
        if n < 2 {
            return Err("a tuplet number is 2 or more".into());
        }
        GroupHead::Tuplet {
            num: n,
            ratio,
            look,
        }
    };
    Ok(Some(head))
}

fn assemble(lines: Vec<SrcLine>, header: &[Attr], diags: &mut Vec<Diag>) -> Vec<Bar> {
    let mut groups: Vec<(String, Vec<SrcLine>)> = Vec::new();
    for line in lines {
        match groups.last_mut() {
            Some((label, g)) if *label == line.label => g.push(line),
            _ => {
                if groups.iter().any(|(l, _)| *l == line.label) {
                    diags.push(Diag::error(
                        line.src,
                        line.label,
                        "the lines of a bar must be together; this bar number is used before",
                    ));
                    continue;
                }
                groups.push((line.label.clone(), vec![line]));
            }
        }
    }
    let mut meter = header
        .iter()
        .rev()
        .find_map(|a| match a {
            Attr::Time(m) => Some(m.clone()),
            _ => None,
        })
        .unwrap_or_else(Meter::four_four);
    let labels: Vec<String> = groups.iter().map(|(l, _)| l.clone()).collect();
    let mut bars = Vec::new();
    for (i, (label, lines)) in groups.into_iter().enumerate() {
        let next = labels.get(i + 1).map(String::as_str);
        let bar = assemble_bar(label, lines, &mut meter, next, diags);
        bars.push(bar);
    }
    bars
}

struct GridLine {
    src: usize,
    lane: Lane,
    raw: Vec<Raw>,
}

fn assemble_bar(
    label: String,
    lines: Vec<SrcLine>,
    meter: &mut Meter,
    next: Option<&str>,
    diags: &mut Vec<Diag>,
) -> Bar {
    let src = lines.first().map_or(0, |l| l.src);
    let (attrs, grid) = attrs_and_grid(&label, lines, diags);
    for a in &attrs {
        if let Attr::Time(m) = a {
            *meter = m.clone();
        }
    }
    let cells_attr = attrs.iter().find_map(|a| match a {
        Attr::Cells(n) => Some(*n as usize),
        _ => None,
    });
    let mut layout = BarLayout {
        meter: meter.clone(),
        ..BarLayout::default()
    };
    let base = meter.cells().unwrap_or_else(|_| vec![Frac::new(1, 4); 4]);
    let nominal: Vec<Frac> = match cells_attr {
        Some(n) => base.iter().copied().cycle().take(n).collect(),
        None => base,
    };
    let is_bar_rest = |g: &GridLine| matches!(g.raw.as_slice(), [Raw::Bar, Raw::Word(w), Raw::Bar] if w.starts_with('R'));
    let counts: Vec<Option<usize>> = grid
        .iter()
        .map(|g| (!is_bar_rest(g)).then(|| cell_count(&g.raw)))
        .collect();
    let starts_x = grid.iter().any(|g| first_word(&g.raw) == Some("x"));
    let ends_x = grid.iter().any(|g| last_word(&g.raw) == Some("x"));
    let partial_ok = label == "0"
        || label.ends_with('b')
        || next.is_some_and(|n| n.ends_with('b'))
        || starts_x
        || ends_x;
    let k = counts
        .iter()
        .flatten()
        .copied()
        .next()
        .unwrap_or(nominal.len());
    layout.full = nominal.iter().copied().sum();
    let partial = partial_ok && k < nominal.len() && cells_attr.is_none();
    let expected = if partial { k } else { nominal.len() };
    layout.cells = if !partial {
        nominal.clone()
    } else if label == "0" || label.ends_with('b') || starts_x {
        nominal[nominal.len() - k..].to_vec()
    } else {
        nominal[..k].to_vec()
    };
    let mut out_lines = Vec::new();
    let mut bar_rests = Vec::new();
    let mut x_slots: Option<(Frac, Frac, String)> = None;
    for (g, count) in grid.into_iter().zip(counts) {
        let at = format!("{label} {}", g.lane);
        let Some(count) = count else {
            bar_rests.push(g);
            continue;
        };
        if count != expected {
            let msg = if cells_attr.is_some() {
                format!("{count} cells; this bar has cells={expected}")
            } else {
                format!(
                    "{count} cells; time={meter} has {} cells. An incomplete bar starts or ends with x",
                    nominal.len()
                )
            };
            diags.push(Diag::error(g.src, at, msg));
            continue;
        }
        match layout_line(g.lane, &g.raw, &layout.cells) {
            Ok(els) => {
                let (lead, trail) = edge_gaps(&els);
                match &x_slots {
                    None => x_slots = Some((lead, trail, g.lane.to_string())),
                    Some((l, t, first)) if (*l, *t) != (lead, trail) => {
                        diags.push(Diag::error(
                            g.src,
                            at,
                            format!(
                                "the x slots differ from those of the {first} line of this bar"
                            ),
                        ));
                        continue;
                    }
                    Some(_) => {}
                }
                out_lines.push(Line {
                    lane: g.lane,
                    src: g.src,
                    els,
                });
            }
            Err((t, msg)) => {
                let cell = layout.cell_at(t) + 1;
                diags.push(Diag::error(g.src, format!("{at} cell {cell}"), msg));
            }
        }
    }
    if let Some((lead, trail, _)) = x_slots {
        layout.lead = lead;
        layout.trail = trail;
    }
    for g in bar_rests {
        let Raw::Word(w) = &g.raw[1] else { continue };
        match bar_rest_line(&label, &g, w, &layout) {
            Ok(line) => out_lines.push(line),
            Err(d) => diags.push(d),
        }
    }
    Bar {
        label,
        src,
        attrs,
        lines: out_lines,
        layout,
    }
}

fn attrs_and_grid(
    label: &str,
    lines: Vec<SrcLine>,
    diags: &mut Vec<Diag>,
) -> (Vec<Attr>, Vec<GridLine>) {
    let mut attrs = Vec::new();
    let mut grid: Vec<GridLine> = Vec::new();
    for line in lines {
        match line.body {
            Body::Attrs(a) => attrs.extend(a),
            Body::Grid(lane, raw) => {
                if grid.iter().any(|g| g.lane == lane) {
                    diags.push(Diag::error(
                        line.src,
                        format!("{label} {lane}"),
                        "this lane appears twice in the bar",
                    ));
                    continue;
                }
                grid.push(GridLine {
                    src: line.src,
                    lane,
                    raw,
                });
            }
        }
    }
    (attrs, grid)
}

fn edge_gaps(els: &[El]) -> (Frac, Frac) {
    let lead = match els.first() {
        Some(El {
            kind: ElKind::Tok(Tok::Gap),
            dur,
            ..
        }) => *dur,
        _ => Frac::ZERO,
    };
    let trail = match els.last() {
        Some(El {
            kind: ElKind::Tok(Tok::Gap),
            dur,
            ..
        }) if els.len() > 1 || lead.is_zero() => *dur,
        _ => Frac::ZERO,
    };
    (lead, trail)
}

fn bar_rest_line(label: &str, g: &GridLine, word: &str, layout: &BarLayout) -> Result<Line, Diag> {
    let at = format!("{label} {}", g.lane);
    if !g.lane.is_voice() {
        return Err(Diag::error(
            g.src,
            at,
            "R is a whole-bar rest; it is only in a voice lane",
        ));
    }
    match crate::text::token::parse_event(word) {
        Ok(
            ev @ Event {
                head: Head::BarRest,
                ..
            },
        ) if ev.grace.is_none() && ev.after.is_none() => {
            let el = El {
                start: layout.lead,
                dur: layout.length(),
                kind: ElKind::Tok(Tok::Event(Box::new(ev))),
            };
            Ok(Line {
                lane: g.lane,
                src: g.src,
                els: vec![el],
            })
        }
        Ok(_) => Err(Diag::error(g.src, at, "R takes only `:` marks")),
        Err(e) => Err(Diag::error(g.src, at, e)),
    }
}

fn first_word(raw: &[Raw]) -> Option<&str> {
    match raw.get(1)? {
        Raw::Word(w) => Some(w),
        _ => None,
    }
}

fn last_word(raw: &[Raw]) -> Option<&str> {
    match raw.get(raw.len().checked_sub(2)?)? {
        Raw::Word(w) => Some(w),
        _ => None,
    }
}

fn bars_in(raw: &[Raw]) -> usize {
    raw.iter().filter(|r| matches!(r, Raw::Bar)).count()
}

fn chunks(raw: &[Raw]) -> Vec<&[Raw]> {
    let inner = &raw[1..raw.len() - 1];
    inner.split(|r| matches!(r, Raw::Bar)).collect()
}

fn cell_count(raw: &[Raw]) -> usize {
    chunks(raw)
        .iter()
        .map(|c| match c {
            [Raw::Group(_, inner)] => bars_in(inner) + 1,
            _ => 1,
        })
        .sum()
}

type LayoutError = (Frac, String);

fn count_i64(n: usize) -> i64 {
    i64::try_from(n).expect("a slot count fits in i64")
}

fn valid_slot_count(cell: Frac, n: usize) -> bool {
    cell_counts(cell).contains(&count_i64(n))
}

fn layout_line(lane: Lane, raw: &[Raw], cells: &[Frac]) -> Result<Vec<El>, LayoutError> {
    let mut starts = Vec::with_capacity(cells.len() + 1);
    let mut t = Frac::ZERO;
    for c in cells {
        starts.push(t);
        t += *c;
    }
    starts.push(t);
    let mut slots: Vec<(Frac, Frac, &Raw)> = Vec::new();
    let mut ci = 0;
    for chunk in chunks(raw) {
        let at = starts[ci.min(cells.len() - 1)];
        if chunk.is_empty() {
            return Err((at, "an empty cell; write `_` for nothing".into()));
        }
        if let [g @ Raw::Group(_, inner)] = chunk {
            let n = bars_in(inner) + 1;
            if n > 1 && !lane.is_voice() {
                return Err((
                    at,
                    "a group in this lane is one slot; it has no `|` inside".into(),
                ));
            }
            let span: Frac = cells[ci..ci + n].iter().copied().sum();
            slots.push((at, span, g));
            ci += n;
            continue;
        }
        if chunk
            .iter()
            .any(|r| matches!(r, Raw::Group(_, inner) if bars_in(inner) > 0))
        {
            return Err((
                at,
                "a group with `|` inside fills its cells; it is the only token of them".into(),
            ));
        }
        let cell = cells[ci];
        if !valid_slot_count(cell, chunk.len()) {
            let counts = if cell.is_pow2() {
                "1, 2, 4, 8, 16, 32 or 64"
            } else {
                "1, 3, 6, 12, 24 or 48"
            };
            return Err((
                at,
                format!(
                    "{} slots. Use {counts} slots, or mark a tuplet: 3[...]",
                    chunk.len()
                ),
            ));
        }
        let d = cell / count_i64(chunk.len());
        for (k, r) in chunk.iter().enumerate() {
            slots.push((at + d * count_i64(k), d, r));
        }
        ci += 1;
    }
    let els = build(lane, slots, &starts, Nesting::Top)?;
    check_gaps(&els, cells)?;
    Ok(els)
}

fn check_gaps(els: &[El], cells: &[Frac]) -> Result<(), LayoutError> {
    let last_start: Frac = cells[..cells.len() - 1].iter().copied().sum();
    for (i, e) in els.iter().enumerate() {
        match &e.kind {
            ElKind::Tok(Tok::Gap) => {
                let lead_ok = i == 0 && e.end() <= cells[0];
                let trail_ok = i == els.len() - 1 && e.start >= last_start;
                if !lead_ok && !trail_ok {
                    return Err((
                        e.start,
                        "x is only at the start of the first cell or the end of the last cell"
                            .into(),
                    ));
                }
            }
            ElKind::Group(_, children) if has_gap(children) => {
                return Err((e.start, "x is not permitted in a group".into()));
            }
            _ => {}
        }
    }
    Ok(())
}

fn has_gap(els: &[El]) -> bool {
    els.iter().any(|e| match &e.kind {
        ElKind::Tok(Tok::Gap) => true,
        ElKind::Group(_, c) => has_gap(c),
        ElKind::Tok(_) => false,
    })
}

fn build(
    lane: Lane,
    slots: Vec<(Frac, Frac, &Raw)>,
    bounds: &[Frac],
    nesting: Nesting,
) -> Result<Vec<El>, LayoutError> {
    let mut els: Vec<El> = Vec::new();
    let mut inner: Vec<Option<&[Raw]>> = Vec::new();
    for (start, dur, raw) in slots {
        match raw {
            Raw::Bar => {}
            Raw::Group(head, items) => {
                els.push(El {
                    start,
                    dur,
                    kind: ElKind::Group(*head, Vec::new()),
                });
                inner.push(Some(items));
            }
            Raw::Word(w) => match parse_word(lane, w).map_err(|e| (start, e))? {
                Word::Dash => extend(lane, &mut els, &mut inner, start, dur, nesting)?,
                Word::Tok(tok) => {
                    if matches!(&tok, Tok::Event(ev) if ev.head == Head::BarRest) {
                        return Err((
                            start,
                            "R is a whole-bar rest; it is the only cell of its line".into(),
                        ));
                    }
                    els.push(El {
                        start,
                        dur,
                        kind: ElKind::Tok(tok),
                    });
                    inner.push(None);
                }
            },
        }
    }
    for (el, items) in els.iter_mut().zip(inner) {
        let (Some(items), ElKind::Group(head, children)) = (items, &mut el.kind) else {
            continue;
        };
        *children = layout_group(lane, *head, items, el.start, el.dur, bounds)?;
    }
    Ok(merge(els))
}

fn extend(
    lane: Lane,
    els: &mut Vec<El>,
    inner: &mut Vec<Option<&[Raw]>>,
    start: Frac,
    dur: Frac,
    nesting: Nesting,
) -> Result<(), LayoutError> {
    // Outside a voice lane a group is only a time grid, so a `-` after it continues the span.
    let after_group = matches!(els.last().map(|e| &e.kind), Some(ElKind::Group(..)));
    if after_group && !lane.is_voice() {
        els.push(El {
            start,
            dur,
            kind: ElKind::Tok(Tok::Cont),
        });
        inner.push(None);
        return Ok(());
    }
    let Some(last) = els.last_mut() else {
        if nesting == Nesting::InGroup && !lane.is_span() {
            return Err((start, "`-` at the start of a group".into()));
        }
        if lane.is_span() {
            els.push(El {
                start,
                dur,
                kind: ElKind::Tok(Tok::Cont),
            });
            inner.push(None);
            return Ok(());
        }
        let msg = match lane {
            Lane::Voice(..) => {
                "`-` in the first slot of a bar. A note that crosses a barline is written again with ~"
            }
            _ => "this lane has no `-`",
        };
        return Err((start, msg.into()));
    };
    let ok = match &mut last.kind {
        ElKind::Group(..) => Ok(()),
        ElKind::Tok(tok) => match tok {
            Tok::Event(_)
            | Tok::Cont
            | Tok::Pedal(_)
            | Tok::Hairpin(..)
            | Tok::Ottava(_)
            | Tok::Trill => Ok(()),
            Tok::Text(t) => {
                t.ext = true;
                Ok(())
            }
            Tok::Tempo(t) if t.text.is_some() => {
                t.ext = true;
                Ok(())
            }
            Tok::Tempo(_) => Err("`-` after a tempo mark needs a text: \"rit.\" - -"),
            Tok::Empty | Tok::Gap if lane.is_voice() => Err("`-` after `_` or `x`"),
            Tok::Empty | Tok::Gap => Err("`-` with no span to continue"),
            Tok::Dyn(_) | Tok::DynText(_) | Tok::Clef(_) | Tok::Phrase(_) => {
                Err("this lane has no `-`")
            }
        },
    };
    ok.map_err(|m| (start, m.to_string()))?;
    last.dur += dur;
    Ok(())
}

fn layout_group(
    lane: Lane,
    head: GroupHead,
    items: &[Raw],
    start: Frac,
    dur: Frac,
    bounds: &[Frac],
) -> Result<Vec<El>, LayoutError> {
    let num = match head {
        GroupHead::Tuplet { num, .. } => num as usize,
        GroupHead::Tremolo { .. } => 2,
    };
    let n = items.iter().filter(|r| !matches!(r, Raw::Bar)).count();
    let valid = n >= num && n % num == 0 && (n / num).is_power_of_two();
    if !valid {
        let msg = match head {
            GroupHead::Tremolo { .. } => format!(
                "{n} slots in a tremolo group. Use 2 notes, with `-` for the slots: t3[C5 - G5 -]"
            ),
            GroupHead::Tuplet { .. } => format!(
                "{n} slots in a {num}-group. Use {num}, {}, {} ... slots",
                num * 2,
                num * 4
            ),
        };
        return Err((start, msg));
    }
    let slot = dur / count_i64(n);
    let mut slots = Vec::new();
    let mut k = 0i64;
    for r in items {
        if matches!(r, Raw::Bar) {
            let t = start + slot * k;
            if !bounds.contains(&t) {
                return Err((
                    t,
                    "a `|` in the group is not on a cell boundary; use `-` after the group".into(),
                ));
            }
            continue;
        }
        slots.push((start + slot * k, slot, r));
        k += 1;
    }
    build(lane, slots, bounds, Nesting::InGroup)
}

fn merge(els: Vec<El>) -> Vec<El> {
    let mut out: Vec<El> = Vec::with_capacity(els.len());
    for e in els {
        if let Some(last) = out.last_mut() {
            let joins = match (&last.kind, &e.kind) {
                (ElKind::Tok(Tok::Empty), ElKind::Tok(Tok::Empty))
                | (ElKind::Tok(Tok::Gap), ElKind::Tok(Tok::Gap)) => true,
                (ElKind::Tok(t), ElKind::Tok(Tok::Empty)) => t.is_point(),
                _ => false,
            };
            if joins {
                last.dur += e.dur;
                continue;
            }
        }
        out.push(e);
    }
    out
}
