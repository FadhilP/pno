use std::fmt::Write as _;

use crate::frac::Frac;
use crate::model::{
    AccDisplay, ArpKind, Articulation, Attr, Bar, BarLayout, Clef, Dyn, El, ElKind, Event,
    GlissLine, Grace, GroupHead, Hairpin, Head, Lane, Metronome, Note, OrnamentKind, Ottava, Pedal,
    PitchMark, Place, Score, Slurs, Staff, Tempo, TempoMark, TextFrame, Tok, TokMark, TupletLook,
};
use crate::text::attr::{escape, fmt_num, fmt_signed};

pub fn write(score: &Score) -> Result<String, String> {
    let mut out = String::new();
    let mut header: Vec<&Attr> = score.header.iter().collect();
    header.sort_by_key(|a| a.order());
    for a in header {
        let _ = writeln!(out, "@ {a}");
    }
    let label_w = score.bars.iter().map(|b| b.label.len()).max().unwrap_or(1);
    let kept: Vec<Vec<&crate::model::Line>> = score.bars.iter().map(kept_lines).collect();
    let lane_w = kept
        .iter()
        .flatten()
        .map(|l| l.lane.to_string().len())
        .chain(std::iter::once(1))
        .max()
        .unwrap_or(1);
    for (bar, lines) in score.bars.iter().zip(&kept) {
        out.push('\n');
        if !bar.attrs.is_empty() {
            let mut attrs: Vec<&Attr> = bar.attrs.iter().collect();
            attrs.sort_by_key(|a| a.order());
            let list: Vec<String> = attrs.iter().map(ToString::to_string).collect();
            let _ = writeln!(
                out,
                "{:label_w$} {:lane_w$} {}",
                bar.label,
                "@",
                list.join(" ")
            );
        }
        for line in lines {
            let body = write_line(line.lane, &line.els, &bar.layout)
                .map_err(|e| format!("{} {}: {e}", bar.label, line.lane))?;
            let _ = writeln!(
                out,
                "{:label_w$} {:lane_w$} {body}",
                bar.label,
                line.lane.to_string()
            );
        }
    }
    Ok(out)
}

fn kept_lines(bar: &Bar) -> Vec<&crate::model::Line> {
    let mut lines: Vec<&crate::model::Line> = bar
        .lines
        .iter()
        .filter(|l| {
            let required = matches!(l.lane, Lane::Voice(_, 1) | Lane::Ped | Lane::PedRule);
            required || has_content(&l.els)
        })
        .collect();
    lines.sort_by_key(|l| l.lane.order());
    lines
}

fn has_content(els: &[El]) -> bool {
    els.iter().any(|e| match &e.kind {
        ElKind::Tok(Tok::Empty | Tok::Gap) => false,
        ElKind::Tok(_) => true,
        ElKind::Group(_, c) => has_content(c),
    })
}

const PLAIN_COUNTS: [i64; 7] = [1, 2, 4, 8, 16, 32, 64];
const DOTTED_COUNTS: [i64; 6] = [1, 3, 6, 12, 24, 48];

pub fn cell_counts(cell: Frac) -> &'static [i64] {
    if cell.is_pow2() {
        &PLAIN_COUNTS
    } else if (cell / 3).is_pow2() {
        &DOTTED_COUNTS
    } else {
        &PLAIN_COUNTS[..1]
    }
}

pub fn write_line(lane: Lane, els: &[El], layout: &BarLayout) -> Result<String, String> {
    if let [
        El {
            kind: ElKind::Tok(Tok::Event(ev)),
            ..
        },
    ] = els
    {
        if ev.head == Head::BarRest {
            return Ok(format!("| {} |", event(ev)));
        }
    }
    let flat;
    let els = if lane.is_voice() {
        els
    } else {
        flat = flat_tokens(els);
        &flat
    };
    let starts = {
        let mut s = layout.cell_starts();
        s.push(layout.total());
        s
    };
    let mut out = String::from("|");
    let mut ci = 0;
    while ci < layout.cells.len() {
        let (cs, ce) = (starts[ci], starts[ci + 1]);
        if let Some((text, next)) = bar_form_group(lane, els, &starts, ci)? {
            let _ = write!(out, " {text} |");
            ci = next;
            continue;
        }
        let cell = ce - cs;
        let n = slot_count(els, cs, ce, cell_counts(cell));
        if n.is_none() && !lane.is_voice() {
            if let Some(text) = time_grid(lane, els, cs, ce)? {
                let _ = write!(out, " {text} |");
                ci += 1;
                continue;
            }
        }
        let n =
            n.ok_or_else(|| format!("cell {} cannot be written with a valid slot count", ci + 1))?;
        let unit = cell / n;
        for k in 0..n {
            let t = cs + unit * k;
            let _ = write!(out, " {}", slot_token(lane, els, t)?);
        }
        out.push_str(" |");
        ci += 1;
    }
    Ok(out)
}

fn flat_tokens(els: &[El]) -> Vec<El> {
    els.iter()
        .flat_map(|e| match &e.kind {
            ElKind::Tok(_) => vec![e.clone()],
            ElKind::Group(_, children) => flat_tokens(children),
        })
        .collect()
}

// A cell of a mark lane at tuplet times: the smallest tuplet number, then the fewest slots.
fn time_grid(lane: Lane, els: &[El], cs: Frac, ce: Frac) -> Result<Option<String>, String> {
    let plain = cell_counts(ce - cs);
    for num in 2..=32i64 {
        let counts: Vec<i64> = (0..6).map(|k| num << k).collect();
        if counts.iter().any(|n| plain.contains(n)) {
            continue;
        }
        let Some(n) = slot_count(els, cs, ce, &counts) else {
            continue;
        };
        let unit = (ce - cs) / n;
        let slots: Vec<String> = (0..n)
            .map(|k| slot_token(lane, els, cs + unit * k))
            .collect::<Result<_, _>>()?;
        return Ok(Some(format!("{num}[{}]", slots.join(" "))));
    }
    Ok(None)
}

fn covering(els: &[El], t: Frac) -> Option<&El> {
    els.iter().find(|e| e.start <= t && t < e.end())
}

fn slot_token(lane: Lane, els: &[El], t: Frac) -> Result<String, String> {
    let e = covering(els, t).ok_or_else(|| format!("no content at time {t}"))?;
    if e.start == t {
        return el_text(lane, e);
    }
    let cont = match &e.kind {
        ElKind::Tok(Tok::Empty) => "_",
        ElKind::Tok(Tok::Gap) => "x",
        ElKind::Tok(tok) if tok.is_point() => "_",
        _ => "-",
    };
    Ok(cont.to_string())
}

fn slot_count(els: &[El], from: Frac, to: Frac, counts: &[i64]) -> Option<i64> {
    let span = to - from;
    let mut bounds = Vec::new();
    let mut ext = Vec::new();
    for e in els {
        if e.start >= from && e.start < to {
            bounds.push(e.start - from);
            if let ElKind::Tok(t) = &e.kind {
                if t.extended() && e.end() <= to {
                    ext.push(e.dur);
                }
            }
        }
        if e.end() > from && e.end() < to {
            bounds.push(e.end() - from);
        }
    }
    counts.iter().copied().find(|n| {
        let unit = span / *n;
        bounds.iter().all(|b| (*b / unit).is_int()) && ext.iter().all(|d| *d / unit >= Frac::int(2))
    })
}

fn bar_form_group(
    lane: Lane,
    els: &[El],
    starts: &[Frac],
    ci: usize,
) -> Result<Option<(String, usize)>, String> {
    let cs = starts[ci];
    let Some(e) = els.iter().find(|e| e.start == cs) else {
        return Ok(None);
    };
    if !matches!(e.kind, ElKind::Group(..)) || e.end() <= starts[ci + 1] {
        return Ok(None);
    }
    let Some(last) = starts.iter().position(|s| *s == e.end()) else {
        return Ok(None);
    };
    let inner: Vec<Frac> = starts[ci + 1..last].to_vec();
    match group_text(lane, e, Some(&inner))? {
        Some(text) => Ok(Some((text, last))),
        None => Ok(None),
    }
}

fn el_text(lane: Lane, e: &El) -> Result<String, String> {
    match &e.kind {
        ElKind::Tok(t) => Ok(tok_text(lane, t)),
        ElKind::Group(..) => {
            group_text(lane, e, None)?.ok_or_else(|| "a group cannot be written".to_string())
        }
    }
}

pub fn default_ratio(num: u32, span: Frac) -> Option<u32> {
    if span.is_pow2() {
        return match num {
            3 => Some(2),
            5..=7 => Some(4),
            _ => None,
        };
    }
    if (span / 3).is_pow2() {
        return match num {
            2 | 4 => Some(3),
            _ => None,
        };
    }
    None
}

fn group_head_text(head: GroupHead, span: Frac) -> String {
    match head {
        GroupHead::Tremolo { strokes } => format!("t{strokes}["),
        GroupHead::Tuplet { num, ratio, look } => {
            let mut s = num.to_string();
            if let Some(b) = ratio.filter(|b| Some(*b) != default_ratio(num, span)) {
                let _ = write!(s, ":{b}");
            }
            s.push_str(match look {
                TupletLook::Default => "",
                TupletLook::Up => "u",
                TupletLook::Down => "d",
                TupletLook::Hidden => "h",
            });
            s.push('[');
            s
        }
    }
}

fn group_text(lane: Lane, e: &El, bars: Option<&[Frac]>) -> Result<Option<String>, String> {
    let ElKind::Group(head, children) = &e.kind else {
        return Ok(None);
    };
    let num = match head {
        GroupHead::Tuplet { num, .. } => i64::from(*num),
        GroupHead::Tremolo { .. } => 2,
    };
    let counts: Vec<i64> = (0..7).map(|k| num << k).collect();
    let mut probe = children.clone();
    if let Some(b) = bars {
        for t in b {
            probe.push(El {
                start: *t,
                dur: Frac::ZERO,
                kind: ElKind::Tok(Tok::Empty),
            });
        }
    }
    let Some(n) = slot_count(&probe, e.start, e.end(), &counts) else {
        return match bars {
            Some(_) => Ok(None),
            None => Err("a group cannot be written with a valid slot count".into()),
        };
    };
    let unit = e.dur / n;
    let mut parts = Vec::new();
    for k in 0..n {
        let t = e.start + unit * k;
        if bars.is_some_and(|b| b.contains(&t)) {
            parts.push("|".to_string());
        }
        parts.push(slot_token(lane, children, t)?);
    }
    Ok(Some(format!(
        "{}{}]",
        group_head_text(*head, e.dur),
        parts.join(" ")
    )))
}

fn place_text(lane: Lane, place: Option<Place>) -> &'static str {
    let default = match lane {
        Lane::Mark(Staff::Upper, crate::model::MarkKind::Text, _) | Lane::Tempo => Place::Above,
        _ => Place::Below,
    };
    match place {
        Some(p) if p != default => match p {
            Place::Above => ":above",
            Place::Below => ":below",
        },
        _ => "",
    }
}

fn quote(s: &str) -> String {
    format!("\"{}\"", escape(s))
}

pub fn tok_text(lane: Lane, t: &Tok) -> String {
    match t {
        Tok::Empty => "_".into(),
        Tok::Gap => "x".into(),
        Tok::Cont => "-".into(),
        Tok::Phrase(m) => m.text().into(),
        Tok::Event(ev) => event(ev),
        Tok::Pedal(Pedal::Press) => "P".into(),
        Tok::Pedal(Pedal::Change) => "C".into(),
        Tok::Dyn(d) => dyn_text(lane, d),
        Tok::DynText(d) => {
            let level = d
                .level
                .map(|v| format!("={}", fmt_num(v)))
                .unwrap_or_default();
            format!("{}{level}{}", quote(&d.text), place_text(lane, d.place))
        }
        Tok::Hairpin(h, p) => {
            let s = match h {
                Hairpin::Cresc => "<",
                Hairpin::Dim => ">",
            };
            format!("{s}{}", place_text(lane, *p))
        }
        Tok::Text(t) => {
            let frame = match t.frame {
                TextFrame::None => "",
                TextFrame::Box => ":box",
                TextFrame::Rehearsal => ":mark",
            };
            format!("{}{frame}{}", quote(&t.text), place_text(lane, t.place))
        }
        Tok::Tempo(t) => tempo_text(lane, t),
        Tok::Trill => "tr".into(),
        Tok::Clef(c) => match c {
            Clef::Treble => "treble",
            Clef::Bass => "bass",
            Clef::Alto => "alto",
            Clef::Tenor => "tenor",
            Clef::Treble8va => "treble8va",
            Clef::Treble8vb => "treble8vb",
            Clef::Bass8va => "bass8va",
            Clef::Bass8vb => "bass8vb",
        }
        .into(),
        Tok::Ottava(o) => match o {
            Ottava::Up8 => "8va",
            Ottava::Down8 => "8vb",
            Ottava::Up15 => "15ma",
            Ottava::Down15 => "15mb",
        }
        .into(),
    }
}

fn dyn_text(lane: Lane, d: &Dyn) -> String {
    let before = d.before.as_deref().map(quote).unwrap_or_default();
    let after = d.after.as_deref().map(quote).unwrap_or_default();
    format!(
        "{before}{}{after}{}",
        d.mark.name(),
        place_text(lane, d.place)
    )
}

fn metronome_text(m: &Metronome) -> String {
    let dot = if m.dotted { "." } else { "" };
    format!("{}{dot}={}", m.unit, fmt_num(m.bpm))
}

fn tempo_text(lane: Lane, t: &Tempo) -> String {
    let mut s = t.text.as_deref().map(quote).unwrap_or_default();
    match &t.mark {
        None => {}
        Some(TempoMark::Sound(bpm)) => {
            let _ = write!(s, "={}", fmt_num(*bpm));
        }
        Some(TempoMark::Hidden(m)) => {
            let _ = write!(s, "_{}", metronome_text(m));
        }
        Some(TempoMark::Shown {
            metronome: m,
            parens,
        }) => {
            let body = metronome_text(m);
            if *parens {
                let _ = write!(s, "({body})");
            } else {
                s.push_str(&body);
            }
        }
    }
    s.push_str(place_text(lane, t.place));
    s
}

fn acc_text(a: i8) -> &'static str {
    match a {
        0 => "n",
        1 => "#",
        2 => "##",
        -1 => "b",
        _ => "bb",
    }
}

pub fn tok_mark_text(m: &TokMark) -> String {
    let s = match m {
        TokMark::Art(a) => match a {
            Articulation::Staccato => "st",
            Articulation::Staccatissimo => "stm",
            Articulation::Tenuto => "ten",
            Articulation::Accent => "ac",
            Articulation::Marcato => "marc",
            Articulation::MarcatoDown => "marcd",
        },
        TokMark::Fermata { inverted: false } => "ferm",
        TokMark::Fermata { inverted: true } => "fermi",
        TokMark::Ornament { kind, above, below } => {
            let name = match kind {
                OrnamentKind::Trill => "tr",
                OrnamentKind::TrillLine => "trl",
                OrnamentKind::Mordent => "mord",
                OrnamentKind::Pralltriller => "pral",
                OrnamentKind::LongMordent => "lmord",
                OrnamentKind::LongPralltriller => "lpral",
                OrnamentKind::Turn => "turn",
                OrnamentKind::InvertedTurn => "iturn",
                OrnamentKind::DelayedTurn => "dturn",
                OrnamentKind::DelayedInvertedTurn => "diturn",
            };
            let mut s = format!(":{name}");
            if let Some(a) = above {
                s.push_str(acc_text(*a));
            }
            if let Some(b) = below {
                let _ = write!(s, "/{}", acc_text(*b));
            }
            return s;
        }
        TokMark::Arp(k) => match k {
            ArpKind::Plain => "arp",
            ArpKind::Up => "arpu",
            ArpKind::Down => "arpd",
            ArpKind::CrossStaff => "arp2",
        },
        TokMark::Small => "small",
        TokMark::Tremolo(n) => return format!(":trem{n}"),
        TokMark::Vel(v) => return format!(":v{}", fmt_signed(*v)),
        TokMark::Mute => "v0",
        TokMark::BeamBegin => "bb",
        TokMark::BeamEnd => "be",
        TokMark::BeamBreak => "b1",
        TokMark::StemUp => "up",
        TokMark::StemDown => "down",
        TokMark::SlurUp => "su",
        TokMark::SlurDown => "sd",
        TokMark::GraceValue(v) => return format!(":{v}"),
    };
    format!(":{s}")
}

fn pitch_mark_text(m: &PitchMark) -> String {
    match m {
        PitchMark::Finger(n) => format!("^f{n}"),
        PitchMark::CrossStaff(s) => format!("^{}", s.name()),
        PitchMark::TieLater => "^t".into(),
        PitchMark::Hide => "^hide".into(),
        PitchMark::LetRing => "^lv".into(),
        PitchMark::Gliss(GlissLine::Wavy) => "^gl".into(),
        PitchMark::Gliss(GlissLine::WavyText) => "^glt".into(),
        PitchMark::Gliss(GlissLine::Straight) => "^sl".into(),
        PitchMark::Gliss(GlissLine::StraightText) => "^slt".into(),
        PitchMark::Vel(v) => format!("^v{}", fmt_signed(*v)),
        PitchMark::Mute => "^v0".into(),
        PitchMark::Attack(v) => format!("^a{}", fmt_signed(*v)),
        PitchMark::Release(v) => format!("^r{}", fmt_signed(*v)),
        PitchMark::TieUp => "^tu".into(),
        PitchMark::TieDown => "^td".into(),
    }
}

fn is_default_mark(m: &TokMark) -> bool {
    matches!(m, TokMark::Vel(v) if *v == 0.0)
}

fn marks_text(marks: &[TokMark], skip: impl Fn(&TokMark) -> bool) -> String {
    let mut list: Vec<String> = marks
        .iter()
        .filter(|m| !is_default_mark(m) && !skip(m))
        .map(tok_mark_text)
        .collect();
    list.sort();
    list.dedup();
    list.concat()
}

fn note_text(n: &Note) -> String {
    let mut s = String::new();
    if n.tied {
        s.push('~');
    }
    if n.hidden {
        s.push('_');
    }
    let _ = write!(s, "{}", n.pitch);
    s.push_str(match n.acc {
        AccDisplay::Default => "",
        AccDisplay::Force => "!",
        AccDisplay::Paren => "?",
    });
    let mut marks: Vec<String> = n
        .marks
        .iter()
        .filter(|m| !matches!(m, PitchMark::Vel(v) if *v == 0.0))
        .map(pitch_mark_text)
        .collect();
    marks.sort();
    marks.dedup();
    s.push_str(&marks.concat());
    s
}

fn chord_text(notes: &[Note]) -> String {
    let mut sorted: Vec<&Note> = notes.iter().collect();
    sorted.sort_by_key(|n| (n.pitch.midi(), n.pitch.step));
    let parts: Vec<String> = sorted.into_iter().map(note_text).collect();
    parts.join("+")
}

fn slurs_text(s: &Slurs) -> String {
    let one = |c: char, n: Option<u32>| match n {
        Some(n) => format!("{c}{n}"),
        None => c.to_string(),
    };
    let ends: String = s.ends.iter().map(|n| one(')', *n)).collect();
    let starts: String = s
        .starts
        .iter()
        .map(|st| {
            let dir = match st.place {
                Some(Place::Above) => "u",
                Some(Place::Below) => "d",
                None => "",
            };
            one('(', st.number) + dir
        })
        .collect();
    ends + &starts
}

fn grace_text(g: &Grace) -> String {
    let default = if g.notes.len() == 1 { 8 } else { 16 };
    let notes: Vec<String> = g
        .notes
        .iter()
        .map(|n| {
            let marks = marks_text(&n.marks, |m| *m == TokMark::GraceValue(default));
            format!("{}{marks}{}", chord_text(&n.notes), slurs_text(&n.slurs))
        })
        .collect();
    format!("{{{}{}}}", if g.slash { "/" } else { "" }, notes.join(","))
}

pub fn event(ev: &Event) -> String {
    let mut s = String::new();
    if let Some(g) = &ev.grace {
        s.push_str(&grace_text(g));
    }
    match &ev.head {
        Head::Chord(notes) => s.push_str(&chord_text(notes)),
        Head::Rest(staff) => {
            s.push('r');
            if let Some(st) = staff {
                let _ = write!(s, "^{}", st.name());
            }
        }
        Head::BarRest => s.push('R'),
    }
    s.push_str(&marks_text(&ev.marks, |_| false));
    s.push_str(&slurs_text(&ev.slurs));
    if let Some(g) = &ev.after {
        s.push_str(&grace_text(g));
    }
    s
}
