use crate::model::{
    AccDisplay, ArpKind, Articulation, Clef, Dyn, DynMark, DynText, Event, Grace, GraceNote,
    Hairpin, Head, Lane, MarkKind, Metronome, Note, OrnamentKind, Ottava, Pedal, Pitch, PitchMark,
    Place, SlurStart, Slurs, Staff, Tempo, TempoMark, Text, Tok, TokMark,
};

#[derive(Debug)]
pub enum Word {
    Dash,
    Tok(Tok),
}

pub fn parse_word(lane: Lane, w: &str) -> Result<Word, String> {
    match w {
        "-" => return Ok(Word::Dash),
        "_" => return Ok(Word::Tok(Tok::Empty)),
        "x" => return Ok(Word::Tok(Tok::Gap)),
        _ => {}
    }
    let tok = match lane {
        Lane::Voice(..) => Tok::Event(Box::new(parse_event(w)?)),
        Lane::Ped | Lane::PedRule => match w {
            "P" => Tok::Pedal(Pedal::Press),
            "C" => Tok::Pedal(Pedal::Change),
            _ => return Err(format!("`{w}` is not a pedal mark. Use P, C, - or _")),
        },
        Lane::Tempo => Tok::Tempo(parse_tempo(w)?),
        Lane::Mark(_, kind, _) => match kind {
            MarkKind::Dyn => parse_dyn(w)?,
            MarkKind::Hairpin => {
                let (body, place) = split_place(w)?;
                let hp = match body {
                    "<" => Hairpin::Cresc,
                    ">" => Hairpin::Dim,
                    _ => return Err(format!("`{w}` is not a hairpin mark. Use <, >, - or _")),
                };
                Tok::Hairpin(hp, place)
            }
            MarkKind::Text => {
                let (body, place) = split_place(w)?;
                let (text, rest) = quoted(body)?;
                if !rest.is_empty() {
                    return Err(format!("unexpected `{rest}` after the text"));
                }
                Tok::Text(Text {
                    text,
                    ext: false,
                    place,
                })
            }
            MarkKind::Clef => Tok::Clef(match w {
                "treble" => Clef::Treble,
                "bass" => Clef::Bass,
                "alto" => Clef::Alto,
                "tenor" => Clef::Tenor,
                _ => {
                    return Err(format!(
                        "`{w}` is not a clef. Use treble, bass, alto or tenor"
                    ));
                }
            }),
            MarkKind::Ottava => Tok::Ottava(match w {
                "8va" => Ottava::Up8,
                "8vb" => Ottava::Down8,
                "15ma" => Ottava::Up15,
                "15mb" => Ottava::Down15,
                _ => {
                    return Err(format!(
                        "`{w}` is not an ottava mark. Use 8va, 8vb, 15ma or 15mb"
                    ));
                }
            }),
        },
    };
    Ok(Word::Tok(tok))
}

fn split_place(w: &str) -> Result<(&str, Option<Place>), String> {
    if let Some(b) = w.strip_suffix(":above") {
        return Ok((b, Some(Place::Above)));
    }
    if let Some(b) = w.strip_suffix(":below") {
        return Ok((b, Some(Place::Below)));
    }
    Ok((w, None))
}

pub fn quoted(s: &str) -> Result<(String, &str), String> {
    let Some(body) = s.strip_prefix('"') else {
        return Err(format!("`{s}`: a text must be in quotes"));
    };
    let mut out = String::new();
    let mut chars = body.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Ok((out, &body[i + 1..])),
            '\\' => match chars.next() {
                Some((_, '"')) => out.push('"'),
                Some((_, 'n')) => out.push('\n'),
                Some((_, '\\')) => out.push('\\'),
                _ => return Err("in a text, `\\` must be followed by `\"` or `n`".into()),
            },
            _ => out.push(c),
        }
    }
    Err(format!("`{s}`: the text has no closing quote"))
}

fn parse_dyn(w: &str) -> Result<Tok, String> {
    let (body, place) = split_place(w)?;
    if body.starts_with('"') {
        let (text, rest) = quoted(body)?;
        let level = match rest.strip_prefix('=') {
            Some(n) => Some(parse_number(n)?),
            None if rest.is_empty() => None,
            None => return parse_mark(body, place).map(Tok::Dyn),
        };
        return Ok(Tok::DynText(DynText { text, level, place }));
    }
    parse_mark(body, place).map(Tok::Dyn)
}

fn parse_mark(body: &str, place: Option<Place>) -> Result<Dyn, String> {
    let (before, rest) = if body.starts_with('"') {
        let (t, r) = quoted(body)?;
        (Some(t), r)
    } else {
        (None, body)
    };
    let end = rest.find('"').unwrap_or(rest.len());
    let name = &rest[..end];
    let Some(mark) = DynMark::parse(name) else {
        return Err(format!(
            "`{name}` is not a dynamic mark. Use pppp to ffff, mp, mf, sf, sfz, sffz, sfp, fp, fz, rf or rfz, or a text in quotes"
        ));
    };
    let after = if end < rest.len() {
        let (t, r) = quoted(&rest[end..])?;
        if !r.is_empty() {
            return Err(format!("unexpected `{r}` after the dynamic mark"));
        }
        Some(t)
    } else {
        None
    };
    Ok(Dyn {
        before,
        mark,
        after,
        place,
    })
}

fn parse_tempo(w: &str) -> Result<Tempo, String> {
    let (body, place) = split_place(w)?;
    let (text, rest) = if body.starts_with('"') {
        let (t, r) = quoted(body)?;
        (Some(t), r)
    } else {
        (None, body)
    };
    let mark = if rest.is_empty() {
        None
    } else if let Some(n) = rest.strip_prefix('=') {
        Some(TempoMark::Sound(parse_number(n)?))
    } else if let Some(m) = rest.strip_prefix('_') {
        Some(TempoMark::hidden(parse_metronome(m)?))
    } else if let Some(inner) = rest.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        Some(TempoMark::Shown {
            metronome: parse_metronome(inner)?,
            parens: true,
        })
    } else {
        Some(TempoMark::Shown {
            metronome: parse_metronome(rest)?,
            parens: false,
        })
    };
    if text.is_none() && mark.is_none() {
        return Err(format!("`{w}` is not a tempo mark"));
    }
    Ok(Tempo {
        text,
        mark,
        ext: false,
        place,
    })
}

fn parse_metronome(s: &str) -> Result<Metronome, String> {
    let bad = || format!("`{s}` is not a metronome mark. Use for example q=66 or q.=44");
    let mut chars = s.chars();
    let unit = chars
        .next()
        .filter(|c| "whqes".contains(*c))
        .ok_or_else(bad)?;
    let rest = chars.as_str();
    let (dotted, rest) = match rest.strip_prefix('.') {
        Some(r) => (true, r),
        None => (false, rest),
    };
    let n = rest.strip_prefix('=').ok_or_else(bad)?;
    Ok(Metronome {
        unit,
        dotted,
        bpm: parse_number(n)?,
    })
}

fn parse_number(s: &str) -> Result<f64, String> {
    let ok = !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.');
    match s.parse::<f64>() {
        Ok(v) if ok => Ok(v),
        _ => Err(format!("`{s}` is not a number")),
    }
}

struct Cursor<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<char> {
        self.s[self.i..].chars().next()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.i += c.len_utf8();
            return true;
        }
        false
    }

    fn take_while(&mut self, f: impl Fn(char) -> bool) -> &'a str {
        let start = self.i;
        while let Some(c) = self.peek() {
            if !f(c) {
                break;
            }
            self.i += c.len_utf8();
        }
        &self.s[start..self.i]
    }

    fn rest(&self) -> &'a str {
        &self.s[self.i..]
    }
}

pub fn parse_event(w: &str) -> Result<Event, String> {
    let mut c = Cursor { s: w, i: 0 };
    let ev = event(&mut c).map_err(|e| format!("`{w}`: {e}"))?;
    if !c.rest().is_empty() {
        let hint = if c.rest().starts_with(',') {
            "; join the pitches of a chord with `+`: `C4+E4`"
        } else {
            ""
        };
        return Err(format!("`{w}`: unexpected `{}`{hint}", c.rest()));
    }
    Ok(ev)
}

fn event(c: &mut Cursor) -> Result<Event, String> {
    let before = if c.peek() == Some('{') {
        Some(grace(c)?)
    } else {
        None
    };
    let head = if c.eat('R') {
        Head::BarRest
    } else if c.eat('r') {
        let mut staff = None;
        while c.eat('^') {
            match pitch_mark(c)? {
                PitchMark::CrossStaff(s) => staff = Some(s),
                _ => return Err("a rest takes only the pitch marks ^lh and ^rh".into()),
            }
        }
        Head::Rest(staff)
    } else {
        Head::Chord(chord(c)?)
    };
    let marks = tok_marks(c)?;
    let slurs = slurs(c)?;
    let after = if c.peek() == Some('{') {
        Some(grace(c)?)
    } else {
        None
    };
    if before.is_some() && !matches!(head, Head::Chord(_)) {
        return Err("grace notes need a main note".into());
    }
    Ok(Event {
        grace: before,
        head,
        marks,
        slurs,
        after,
    })
}

fn grace(c: &mut Cursor) -> Result<Grace, String> {
    c.eat('{');
    let slash = c.eat('/');
    let mut notes = Vec::new();
    loop {
        let chord = chord(c)?;
        let marks = tok_marks(c)?;
        let slurs = slurs(c)?;
        notes.push(GraceNote {
            notes: chord,
            marks,
            slurs,
        });
        if c.eat('}') {
            break;
        }
        if !c.eat(',') {
            return Err("a grace group is `{D5,E5}` with notes separated by `,`".into());
        }
    }
    Ok(Grace { slash, notes })
}

fn chord(c: &mut Cursor) -> Result<Vec<Note>, String> {
    let mut notes = vec![note(c)?];
    while c.peek() == Some('+') {
        c.eat('+');
        notes.push(note(c)?);
    }
    Ok(notes)
}

fn note(c: &mut Cursor) -> Result<Note, String> {
    let tied = c.eat('~');
    let hidden = c.eat('_');
    let pitch = pitch(c)?;
    let acc = if c.eat('!') {
        AccDisplay::Force
    } else if c.eat('?') {
        AccDisplay::Paren
    } else {
        AccDisplay::Default
    };
    let mut marks = Vec::new();
    while c.eat('^') {
        marks.push(pitch_mark(c)?);
    }
    Ok(Note {
        tied,
        hidden,
        pitch,
        acc,
        marks,
    })
}

fn pitch(c: &mut Cursor) -> Result<Pitch, String> {
    let letter = c.peek().ok_or("a pitch is missing")?;
    let step = crate::model::STEPS
        .iter()
        .position(|s| *s == letter)
        .ok_or_else(|| {
            format!(
                "`{}` is not a pitch, rest or mark; a pitch is a letter A to G, an accidental and an octave: `Bb3`",
                c.rest()
            )
        })?;
    c.i += 1;
    let acc = c.take_while(|ch| ch == '#' || ch == 'b');
    let alter = match acc {
        "" => 0,
        "#" => 1,
        "##" => 2,
        "b" => -1,
        "bb" => -2,
        _ => return Err(format!("`{acc}` is not an accidental")),
    };
    let oct = c.take_while(|ch| ch.is_ascii_digit());
    let octave: i8 = oct.parse().map_err(|_| {
        format!("the pitch {letter}{acc} has no octave, for example {letter}{acc}4")
    })?;
    if octave > 9 {
        return Err(format!("octave {octave} is out of range"));
    }
    Ok(Pitch {
        step: step as u8,
        alter,
        octave,
    })
}

fn signed(c: &mut Cursor, what: &str) -> Result<f64, String> {
    let sign = match c.peek() {
        Some('+') => 1.0,
        Some('-') => -1.0,
        _ => return Err(format!("{what} needs a sign, for example +8 or -12")),
    };
    c.i += 1;
    let n = c.take_while(|ch| ch.is_ascii_digit() || ch == '.');
    Ok(sign * parse_number(n)?)
}

fn pitch_mark(c: &mut Cursor) -> Result<PitchMark, String> {
    let name = c.take_while(|ch| ch.is_ascii_lowercase());
    let mark = match name {
        "f" => {
            let d = c.take_while(|ch| ch.is_ascii_digit());
            PitchMark::Finger(
                d.parse()
                    .map_err(|_| "a finger mark is ^f and a number, for example ^f3")?,
            )
        }
        "v" if c.eat('0') => PitchMark::Mute,
        "v" => PitchMark::Vel(signed(c, "a velocity offset")?),
        "a" => PitchMark::Attack(signed(c, "an attack offset")?),
        "r" => PitchMark::Release(signed(c, "a release offset")?),
        "lh" => PitchMark::CrossStaff(Staff::Lower),
        "rh" => PitchMark::CrossStaff(Staff::Upper),
        "t" => PitchMark::TieLater,
        "tu" => PitchMark::TieUp,
        "td" => PitchMark::TieDown,
        "hide" => PitchMark::Hide,
        "lv" => PitchMark::LetRing,
        _ => return Err(format!("`^{name}` is not a pitch mark")),
    };
    Ok(mark)
}

fn tok_marks(c: &mut Cursor) -> Result<Vec<TokMark>, String> {
    let mut marks = Vec::new();
    while c.eat(':') {
        let name = c.take_while(|ch| ch.is_ascii_alphanumeric() || "#/+-.".contains(ch));
        marks.push(tok_mark(name)?);
    }
    Ok(marks)
}

const ORNAMENTS: [(&str, OrnamentKind); 10] = [
    ("diturn", OrnamentKind::DelayedInvertedTurn),
    ("dturn", OrnamentKind::DelayedTurn),
    ("iturn", OrnamentKind::InvertedTurn),
    ("turn", OrnamentKind::Turn),
    ("lmord", OrnamentKind::LongMordent),
    ("lpral", OrnamentKind::LongPralltriller),
    ("mord", OrnamentKind::Mordent),
    ("pral", OrnamentKind::Pralltriller),
    ("trl", OrnamentKind::TrillLine),
    ("tr", OrnamentKind::Trill),
];

fn acc_sign(s: &str) -> Option<i8> {
    match s {
        "n" => Some(0),
        "#" => Some(1),
        "##" => Some(2),
        "b" => Some(-1),
        "bb" => Some(-2),
        _ => None,
    }
}

fn tok_mark(name: &str) -> Result<TokMark, String> {
    let mark = match name {
        "st" => TokMark::Art(Articulation::Staccato),
        "stm" => TokMark::Art(Articulation::Staccatissimo),
        "ten" => TokMark::Art(Articulation::Tenuto),
        "ac" => TokMark::Art(Articulation::Accent),
        "marc" => TokMark::Art(Articulation::Marcato),
        "ferm" => TokMark::Fermata { inverted: false },
        "fermi" => TokMark::Fermata { inverted: true },
        "arp" => TokMark::Arp(ArpKind::Plain),
        "arpu" => TokMark::Arp(ArpKind::Up),
        "arpd" => TokMark::Arp(ArpKind::Down),
        "arp2" => TokMark::Arp(ArpKind::CrossStaff),
        "small" => TokMark::Small,
        "v0" => TokMark::Mute,
        "trem1" => TokMark::Tremolo(1),
        "trem2" => TokMark::Tremolo(2),
        "trem3" => TokMark::Tremolo(3),
        "bb" => TokMark::BeamBegin,
        "be" => TokMark::BeamEnd,
        "b1" => TokMark::BeamBreak,
        "up" => TokMark::StemUp,
        "down" => TokMark::StemDown,
        "su" => TokMark::SlurUp,
        "sd" => TokMark::SlurDown,
        "2" | "4" | "8" | "16" | "32" | "64" => {
            TokMark::GraceValue(name.parse().map_err(|_| "bad grace value")?)
        }
        _ => {
            if let Some(v) = name.strip_prefix('v') {
                let mut c = Cursor { s: v, i: 0 };
                let n = signed(&mut c, "a velocity offset")?;
                if !c.rest().is_empty() {
                    return Err(format!("`:{name}` is not a velocity offset"));
                }
                return Ok(TokMark::Vel(n));
            }
            return ornament(name);
        }
    };
    Ok(mark)
}

fn ornament(name: &str) -> Result<TokMark, String> {
    let unknown = || format!("`:{name}` is not a mark");
    let (kind, rest) = ORNAMENTS
        .iter()
        .find_map(|(n, k)| name.strip_prefix(n).map(|r| (*k, r)))
        .ok_or_else(unknown)?;
    let (above, below) = match rest.split_once('/') {
        Some((a, b)) => (a, Some(b)),
        None => (rest, None),
    };
    let above = match above {
        "" => None,
        a => Some(acc_sign(a).ok_or_else(unknown)?),
    };
    let below = match below {
        None => None,
        Some(b) => Some(acc_sign(b).ok_or_else(unknown)?),
    };
    Ok(TokMark::Ornament { kind, above, below })
}

fn slurs(c: &mut Cursor) -> Result<Slurs, String> {
    let mut s = Slurs {
        ends: Vec::new(),
        starts: Vec::new(),
    };
    loop {
        let is_end = match c.peek() {
            Some(')') => true,
            Some('(') => false,
            _ => break,
        };
        c.i += 1;
        let d = c.take_while(|ch| ch.is_ascii_digit());
        let num = if d.is_empty() {
            None
        } else {
            Some(d.parse().map_err(|_| "bad slur number")?)
        };
        if is_end {
            if !s.starts.is_empty() {
                return Err("write `)` before `(` on one token".into());
            }
            s.ends.push(num);
            continue;
        }
        let place = match c.peek() {
            Some('u') => Some(Place::Above),
            Some('d') => Some(Place::Below),
            _ => None,
        };
        if place.is_some() {
            c.i += 1;
            if s.starts.is_empty() {
                return Err("the direction of the first slur on a token is `:su` or `:sd`".into());
            }
        }
        s.starts.push(SlurStart { number: num, place });
    }
    Ok(s)
}
