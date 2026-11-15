use std::fmt;

use crate::model::{
    Attr, BarStyle, BracketEnd, Break, DynMark, Jump, Key, Lane, MetaField, Meter, Mode,
    PedalStyle, Sign, TimeSymbol,
};
use crate::text::token::quoted;

const MAJOR_KEYS: [&str; 15] = [
    "Cb", "Gb", "Db", "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#",
];
const MINOR_KEYS: [&str; 15] = [
    "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#", "G#", "D#", "A#",
];

const JUMPS: [(&str, Jump); 8] = [
    ("dc", Jump::Dc),
    ("ds", Jump::Ds),
    ("dc-fine", Jump::DcFine),
    ("ds-fine", Jump::DsFine),
    ("dc-coda", Jump::DcCoda),
    ("ds-coda", Jump::DsCoda),
    ("tocoda", Jump::ToCoda),
    ("fine", Jump::Fine),
];

const BAR_STYLES: [(&str, BarStyle); 6] = [
    ("normal", BarStyle::Normal),
    ("double", BarStyle::Double),
    ("final", BarStyle::Final),
    ("dashed", BarStyle::Dashed),
    ("dotted", BarStyle::Dotted),
    ("none", BarStyle::None),
];

pub fn split_attrs(s: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    let mut rest = s.trim_start();
    while !rest.is_empty() {
        let eq = rest
            .find('=')
            .ok_or_else(|| format!("`{rest}`: an attribute is name=value"))?;
        let name = &rest[..eq];
        if name.contains(char::is_whitespace) || name.is_empty() {
            return Err(format!("`{name}`: an attribute is name=value"));
        }
        let after = &rest[eq + 1..];
        let (value, next) = if after.starts_with('"') {
            let (text, r) = quoted(after)?;
            (format!("\"{}\"", escape(&text)), r)
        } else {
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            (after[..end].to_string(), &after[end..])
        };
        if !next.is_empty() && !next.starts_with(char::is_whitespace) {
            return Err(format!("`{next}`: separate attributes with a space"));
        }
        out.push((name.to_string(), value));
        rest = next.trim_start();
    }
    Ok(out)
}

pub fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn number(v: &str) -> Result<f64, String> {
    v.parse::<f64>()
        .map_err(|_| format!("`{v}` is not a number"))
}

fn signed(v: &str) -> Result<f64, String> {
    if !v.starts_with(['+', '-']) && v != "0" {
        return Err(format!("`{v}` needs a sign, for example +10 or -15"));
    }
    number(v)
}

pub fn parse_attr(name: &str, value: &str) -> Result<Attr, String> {
    let unknown_value = || format!("`{value}` is not a value of {name}=");
    if let Some(field) = MetaField::ALL.iter().find(|f| f.name() == name) {
        let (text, rest) = quoted(value)?;
        if !rest.is_empty() {
            return Err(unknown_value());
        }
        return Ok(Attr::Meta(*field, text));
    }
    if let Some(sub) = name.strip_prefix("vel.") {
        if let Some(mark) = DynMark::parse(sub) {
            return Ok(Attr::VelMark(mark, number(value)?));
        }
        return match Lane::parse(sub) {
            Some(lane @ Lane::Voice(..)) => Ok(Attr::VelLane(lane, signed(value)?)),
            _ => Err(format!(
                "`{name}`: vel. takes a dynamic mark or a voice lane, for example vel.p= or vel.rh2="
            )),
        };
    }
    let attr = match name {
        "format" => Attr::Format(value.parse().map_err(|_| unknown_value())?),
        "key" => Attr::Key(parse_key(value).ok_or_else(unknown_value)?),
        "time" => Attr::Time(parse_time(value)?),
        "barline" => {
            let (style, fermata) = match value.strip_suffix(":ferm") {
                Some(s) => (s, true),
                None => (value, false),
            };
            let style = BAR_STYLES
                .iter()
                .find(|(n, _)| *n == style)
                .map(|(_, s)| *s)
                .ok_or_else(unknown_value)?;
            if style == BarStyle::Normal && !fermata {
                return Err("barline=normal is the default; it is used only with :ferm".into());
            }
            Attr::Barline { style, fermata }
        }
        "cells" => Attr::Cells(
            value
                .parse()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(unknown_value)?,
        ),
        "repeat" => parse_repeat(value).ok_or_else(unknown_value)?,
        "volta" => parse_volta(value).ok_or_else(unknown_value)?,
        "sign" => Attr::Sign(match value {
            "segno" => Sign::Segno,
            "coda" => Sign::Coda,
            _ => return Err(unknown_value()),
        }),
        "jump" => Attr::Jump(
            JUMPS
                .iter()
                .find(|(n, _)| *n == value)
                .map(|(_, j)| *j)
                .ok_or_else(unknown_value)?,
        ),
        "break" => Attr::Break(match value {
            "system" => Break::System,
            "page" => Break::Page,
            _ => return Err(unknown_value()),
        }),
        "pedal-style" => Attr::PedalStyle(match value {
            "line" => PedalStyle::Line,
            "sign" => PedalStyle::Sign,
            _ => return Err(unknown_value()),
        }),
        "program" => Attr::Program(
            value
                .parse()
                .ok()
                .filter(|p| (1..=128).contains(p))
                .ok_or_else(unknown_value)?,
        ),
        "volume" => Attr::Volume(number(value)?),
        "pan" => Attr::Pan(number(value)?),
        _ => return Err(format!("unknown attribute `{name}`")),
    };
    Ok(attr)
}

fn parse_key(v: &str) -> Option<Key> {
    if v == "0" {
        return Some(Key {
            fifths: 0,
            mode: None,
        });
    }
    if let Some(n) = v.strip_suffix('#') {
        return n
            .parse::<i8>()
            .ok()
            .filter(|n| (1..=7).contains(n))
            .map(|n| Key {
                fifths: n,
                mode: None,
            });
    }
    if let Some(n) = v
        .strip_suffix('b')
        .filter(|n| n.chars().all(|c| c.is_ascii_digit()))
    {
        return n
            .parse::<i8>()
            .ok()
            .filter(|n| (1..=7).contains(n))
            .map(|n| Key {
                fifths: -n,
                mode: None,
            });
    }
    let (table, tonic, mode) = match v.strip_suffix('m') {
        Some(t) => (&MINOR_KEYS, t, Mode::Minor),
        None => (&MAJOR_KEYS, v, Mode::Major),
    };
    let i = table.iter().position(|k| *k == tonic)?;
    Some(Key {
        fifths: i as i8 - 7,
        mode: Some(mode),
    })
}

fn parse_time(v: &str) -> Result<Meter, String> {
    let bad =
        || format!("`{v}` is not a time signature. Use for example 3/4, 6/8, 7/8(2+2+3), c or cut");
    match v {
        "c" => {
            return Ok(Meter {
                symbol: Some(TimeSymbol::Common),
                ..Meter::four_four()
            });
        }
        "cut" => {
            return Ok(Meter {
                beats: vec![2],
                beat_type: 2,
                pattern: None,
                symbol: Some(TimeSymbol::Cut),
            });
        }
        _ => {}
    }
    let (frac, pattern) = match v.split_once('(') {
        Some((f, p)) => {
            let p = p.strip_suffix(')').ok_or_else(bad)?;
            let groups: Option<Vec<u32>> = p.split('+').map(|g| g.parse().ok()).collect();
            (f, Some(groups.ok_or_else(bad)?))
        }
        None => (v, None),
    };
    let (num, den) = frac.split_once('/').ok_or_else(bad)?;
    let beats: Option<Vec<u32>> = num
        .split('+')
        .map(|g| g.parse().ok().filter(|n| *n > 0))
        .collect();
    let beats = beats.ok_or_else(bad)?;
    let beat_type: u32 = den.parse().map_err(|_| bad())?;
    if !matches!(beat_type, 1 | 2 | 4 | 8 | 16 | 32) {
        return Err(bad());
    }
    let meter = Meter {
        beats,
        beat_type,
        pattern,
        symbol: None,
    };
    meter.cells()?;
    Ok(meter)
}

fn parse_repeat(v: &str) -> Option<Attr> {
    let (body, times) = match v.split_once(':') {
        Some((b, t)) => (b, Some(t.parse::<u32>().ok().filter(|t| *t >= 2)?)),
        None => (v, None),
    };
    let (start, end) = match body {
        "start" => (true, false),
        "end" => (false, true),
        "start,end" => (true, true),
        _ => return None,
    };
    if times.is_some() && !end {
        return None;
    }
    Some(Attr::Repeat {
        start,
        end: end.then_some(times.unwrap_or(2)),
    })
}

fn parse_volta(v: &str) -> Option<Attr> {
    let (nums, end) = match v.split_once(':') {
        Some((n, "open")) => (n, Some(BracketEnd::Open)),
        Some((n, "closed")) => (n, Some(BracketEnd::Closed)),
        Some(_) => return None,
        None => (v, None),
    };
    let numbers: Option<Vec<u32>> = nums
        .split(',')
        .map(|n| n.parse().ok().filter(|n| *n > 0))
        .collect();
    Some(Attr::Volta {
        numbers: numbers?,
        end,
    })
}

pub fn fmt_signed(v: f64) -> String {
    format!("{v:+}")
}

pub fn fmt_num(v: f64) -> String {
    format!("{v}")
}

impl fmt::Display for Meter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.symbol {
            Some(TimeSymbol::Common) => return f.write_str("c"),
            Some(TimeSymbol::Cut) => return f.write_str("cut"),
            None => {}
        }
        let beats: Vec<String> = self.beats.iter().map(u32::to_string).collect();
        write!(f, "{}/{}", beats.join("+"), self.beat_type)?;
        if let Some(p) = &self.pattern {
            let groups: Vec<String> = p.iter().map(u32::to_string).collect();
            write!(f, "({})", groups.join("+"))?;
        }
        Ok(())
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let i = (self.fifths + 7) as usize;
        match self.mode {
            Some(Mode::Major) => f.write_str(MAJOR_KEYS[i]),
            Some(Mode::Minor) => write!(f, "{}m", MINOR_KEYS[i]),
            None if self.fifths == 0 => f.write_str("0"),
            None if self.fifths > 0 => write!(f, "{}#", self.fifths),
            None => write!(f, "{}b", -self.fifths),
        }
    }
}

impl fmt::Display for Attr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Attr::Format(n) => write!(f, "format={n}"),
            Attr::Meta(field, text) => write!(f, "{}=\"{}\"", field.name(), escape(text)),
            Attr::Key(k) => write!(f, "key={k}"),
            Attr::Time(m) => write!(f, "time={m}"),
            Attr::Barline { style, fermata } => {
                let name = BAR_STYLES
                    .iter()
                    .find(|(_, s)| s == style)
                    .map_or("normal", |(n, _)| *n);
                write!(f, "barline={name}{}", if *fermata { ":ferm" } else { "" })
            }
            Attr::Cells(n) => write!(f, "cells={n}"),
            Attr::Repeat { start, end } => {
                let body = match (start, end) {
                    (true, Some(_)) => "start,end",
                    (true, None) => "start",
                    (false, _) => "end",
                };
                write!(f, "repeat={body}")?;
                match end {
                    Some(t) if *t != 2 => write!(f, ":{t}"),
                    _ => Ok(()),
                }
            }
            Attr::Volta { numbers, end } => {
                let nums: Vec<String> = numbers.iter().map(u32::to_string).collect();
                write!(f, "volta={}", nums.join(","))?;
                match end {
                    Some(BracketEnd::Open) => f.write_str(":open"),
                    Some(BracketEnd::Closed) => f.write_str(":closed"),
                    None => Ok(()),
                }
            }
            Attr::Sign(Sign::Segno) => f.write_str("sign=segno"),
            Attr::Sign(Sign::Coda) => f.write_str("sign=coda"),
            Attr::Jump(j) => {
                let name = JUMPS.iter().find(|(_, x)| x == j).map_or("dc", |(n, _)| *n);
                write!(f, "jump={name}")
            }
            Attr::Break(Break::System) => f.write_str("break=system"),
            Attr::Break(Break::Page) => f.write_str("break=page"),
            Attr::PedalStyle(PedalStyle::Line) => f.write_str("pedal-style=line"),
            Attr::PedalStyle(PedalStyle::Sign) => f.write_str("pedal-style=sign"),
            Attr::VelMark(m, v) => write!(f, "vel.{}={}", m.name(), fmt_num(*v)),
            Attr::VelLane(l, v) if *v == 0.0 => write!(f, "vel.{l}=0"),
            Attr::VelLane(l, v) => write!(f, "vel.{l}={}", fmt_signed(*v)),
            Attr::Program(p) => write!(f, "program={p}"),
            Attr::Volume(v) => write!(f, "volume={}", fmt_num(*v)),
            Attr::Pan(v) => write!(f, "pan={}", fmt_num(*v)),
        }
    }
}
