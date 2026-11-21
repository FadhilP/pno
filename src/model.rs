use crate::frac::Frac;

#[derive(Clone, Debug, Default)]
pub struct Score {
    pub header: Vec<Attr>,
    pub bars: Vec<Bar>,
}

impl Score {
    pub fn one_staff(&self) -> bool {
        self.header.contains(&Attr::OneStaff)
    }
}

#[derive(Clone, Debug)]
pub struct Bar {
    pub label: String,
    pub src: usize,
    pub attrs: Vec<Attr>,
    pub lines: Vec<Line>,
    pub layout: BarLayout,
}

// Bar time starts at the first cell of the bar, including leading `x` time.
#[derive(Clone, Debug, Default)]
pub struct BarLayout {
    pub meter: Meter,
    pub cells: Vec<Frac>,
    pub lead: Frac,
    pub trail: Frac,
    pub full: Frac,
}

impl BarLayout {
    pub fn total(&self) -> Frac {
        self.cells.iter().copied().sum()
    }

    pub fn length(&self) -> Frac {
        self.total() - self.lead - self.trail
    }

    pub fn cell_starts(&self) -> Vec<Frac> {
        let mut t = Frac::ZERO;
        self.cells
            .iter()
            .map(|d| {
                let s = t;
                t += *d;
                s
            })
            .collect()
    }

    pub fn cell_at(&self, t: Frac) -> usize {
        let mut end = Frac::ZERO;
        for (i, d) in self.cells.iter().enumerate() {
            end += *d;
            if t < end {
                return i;
            }
        }
        self.cells.len().saturating_sub(1)
    }

    pub fn is_partial(&self) -> bool {
        self.length() < self.full
    }
}

#[derive(Clone, Debug)]
pub struct Line {
    pub lane: Lane,
    pub src: usize,
    pub els: Vec<El>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct El {
    pub start: Frac,
    pub dur: Frac,
    pub kind: ElKind,
}

impl El {
    pub fn end(&self) -> Frac {
        self.start + self.dur
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ElKind {
    Tok(Tok),
    Group(GroupHead, Vec<El>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupHead {
    Tuplet {
        num: u32,
        ratio: Option<u32>,
        look: TupletLook,
    },
    Tremolo {
        strokes: u8,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TupletLook {
    Default,
    Up,
    Down,
    Hidden,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Empty,
    Gap,
    Cont,
    Event(Box<Event>),
    Pedal(Pedal),
    Dyn(Dyn),
    DynText(DynText),
    Hairpin(Hairpin, Option<Place>),
    Text(Text),
    Tempo(Tempo),
    Clef(Clef),
    Ottava(Ottava),
    // The wavy line of a trill over several notes; the trill sign is `:tr` on its first note.
    Trill,
    Phrase(PhraseMark),
}

// Each mark starts a phrase. `Section` also starts a section, a phrase of phrases;
// `Breath` and `Caesura` are drawn after the note that ends at their time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhraseMark {
    Phrase,
    Section,
    Breath,
    Caesura,
}

impl PhraseMark {
    pub const ALL: [PhraseMark; 4] = [
        PhraseMark::Phrase,
        PhraseMark::Section,
        PhraseMark::Breath,
        PhraseMark::Caesura,
    ];

    pub fn text(self) -> &'static str {
        match self {
            PhraseMark::Phrase => "/",
            PhraseMark::Section => "//",
            PhraseMark::Breath => "breath",
            PhraseMark::Caesura => "caesura",
        }
    }

    pub fn parse(s: &str) -> Option<PhraseMark> {
        PhraseMark::ALL.into_iter().find(|m| m.text() == s)
    }

    pub fn is_drawn(self) -> bool {
        matches!(self, PhraseMark::Breath | PhraseMark::Caesura)
    }
}

impl Tok {
    // A point token marks a moment; its slot length has no meaning.
    pub fn is_point(&self) -> bool {
        match self {
            Tok::Clef(_) | Tok::Phrase(_) | Tok::DynText(_) => true,
            Tok::Dyn(d) => !d.mark.is_momentary(),
            Tok::Text(t) => !t.ext,
            Tok::Tempo(t) => !t.ext,
            _ => false,
        }
    }

    pub fn extended(&self) -> bool {
        match self {
            Tok::Text(t) => t.ext,
            Tok::Tempo(t) => t.ext,
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Staff {
    Upper,
    Lower,
}

impl Staff {
    pub fn name(self) -> &'static str {
        match self {
            Staff::Upper => "rh",
            Staff::Lower => "lh",
        }
    }

    pub fn number(self) -> u8 {
        match self {
            Staff::Upper => 1,
            Staff::Lower => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MarkKind {
    Ottava,
    Clef,
    Text,
    Dyn,
    Hairpin,
    Trill,
}

impl MarkKind {
    fn name(self) -> &'static str {
        match self {
            MarkKind::Ottava => "8va",
            MarkKind::Clef => "clef",
            MarkKind::Text => "txt",
            MarkKind::Dyn => "dyn",
            MarkKind::Hairpin => "hp",
            MarkKind::Trill => "trl",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lane {
    Voice(Staff, u8),
    Ped,
    PedRule,
    Tempo,
    Phrase,
    Mark(Staff, MarkKind, u8),
}

impl Lane {
    pub fn parse(s: &str) -> Option<Lane> {
        match s {
            "ped" => return Some(Lane::Ped),
            "ped*" => return Some(Lane::PedRule),
            "tempo" => return Some(Lane::Tempo),
            "phrase" => return Some(Lane::Phrase),
            _ => {}
        }
        let staff = match s.get(..2)? {
            "rh" => Staff::Upper,
            "lh" => Staff::Lower,
            _ => return None,
        };
        let rest = &s[2..];
        let Some(kind) = rest.strip_prefix('.') else {
            return match rest {
                "" => Some(Lane::Voice(staff, 1)),
                "2" | "3" | "4" => Some(Lane::Voice(staff, rest.parse().ok()?)),
                _ => None,
            };
        };
        let (name, n) = match kind.strip_suffix(['2', '3']) {
            Some(base) => (base, if kind.ends_with('2') { 2 } else { 3 }),
            None => (kind, 1),
        };
        let kind = match name {
            "8va" => MarkKind::Ottava,
            "clef" => MarkKind::Clef,
            "txt" => MarkKind::Text,
            "dyn" => MarkKind::Dyn,
            "hp" => MarkKind::Hairpin,
            "trl" => MarkKind::Trill,
            _ => return None,
        };
        let has_second = matches!(kind, MarkKind::Text | MarkKind::Dyn | MarkKind::Hairpin);
        if n > 1 && !has_second {
            return None;
        }
        Some(Lane::Mark(staff, kind, n))
    }

    pub fn staff(self) -> Option<Staff> {
        match self {
            Lane::Voice(s, _) | Lane::Mark(s, _, _) => Some(s),
            Lane::Ped => Some(Staff::Lower),
            Lane::Tempo => Some(Staff::Upper),
            Lane::PedRule | Lane::Phrase => None,
        }
    }

    pub fn is_voice(self) -> bool {
        matches!(self, Lane::Voice(..))
    }

    pub fn is_span(self) -> bool {
        match self {
            Lane::Ped | Lane::PedRule | Lane::Tempo => true,
            Lane::Mark(_, k, _) => {
                matches!(
                    k,
                    MarkKind::Ottava | MarkKind::Text | MarkKind::Hairpin | MarkKind::Trill
                )
            }
            Lane::Voice(..) | Lane::Phrase => false,
        }
    }

    pub fn order(self) -> (u8, u8, u8) {
        match self {
            Lane::PedRule => (0, 0, 0),
            Lane::Tempo => (1, 0, 0),
            Lane::Phrase => (1, 1, 0),
            Lane::Mark(Staff::Upper, k, n) => (2, k as u8, n),
            Lane::Voice(Staff::Upper, n) => (3, n, 0),
            Lane::Voice(Staff::Lower, n) => (4, n, 0),
            Lane::Mark(Staff::Lower, k, n) => (5, k as u8, n),
            Lane::Ped => (6, 0, 0),
        }
    }
}

impl std::fmt::Display for Lane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Lane::Voice(s, 1) => write!(f, "{}", s.name()),
            Lane::Voice(s, n) => write!(f, "{}{n}", s.name()),
            Lane::Ped => f.write_str("ped"),
            Lane::PedRule => f.write_str("ped*"),
            Lane::Tempo => f.write_str("tempo"),
            Lane::Phrase => f.write_str("phrase"),
            Lane::Mark(s, k, 1) => write!(f, "{}.{}", s.name(), k.name()),
            Lane::Mark(s, k, n) => write!(f, "{}.{}{n}", s.name(), k.name()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Pitch {
    pub step: u8,
    pub alter: i8,
    pub octave: i8,
}

pub const STEPS: [char; 7] = ['C', 'D', 'E', 'F', 'G', 'A', 'B'];
const STEP_SEMITONES: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];

impl Pitch {
    pub const OCTAVES: std::ops::RangeInclusive<i8> = 0..=9;

    pub fn midi(self) -> i32 {
        (i32::from(self.octave) + 1) * 12
            + STEP_SEMITONES[usize::from(self.step)]
            + i32::from(self.alter)
    }

    pub fn letter(self) -> char {
        STEPS[usize::from(self.step)]
    }
}

impl std::fmt::Display for Pitch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let acc = match self.alter {
            -2 => "bb",
            -1 => "b",
            1 => "#",
            2 => "##",
            _ => "",
        };
        write!(f, "{}{acc}{}", self.letter(), self.octave)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccDisplay {
    Default,
    Force,
    Paren,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PitchMark {
    Finger(u8),
    CrossStaff(Staff),
    TieLater,
    Hide,
    LetRing,
    Vel(f64),
    // `^v0`: the note is drawn and does not sound.
    Mute,
    // `^m`: the main note of a chord, which the voicing level of the lane does not lower.
    Main,
    Attack(f64),
    Release(f64),
    TieUp,
    TieDown,
    // A glissando from this pitch to the next chord of the lane.
    Gliss(GlissLine),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlissLine {
    Wavy,
    WavyText,
    Straight,
    StraightText,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub tied: bool,
    // `_` before the pitch: the note sounds and is not drawn.
    pub hidden: bool,
    pub pitch: Pitch,
    pub acc: AccDisplay,
    pub marks: Vec<PitchMark>,
}

impl Note {
    pub fn vel(&self) -> Option<f64> {
        self.marks.iter().find_map(|m| match m {
            PitchMark::Vel(v) => Some(*v),
            _ => None,
        })
    }

    pub fn has(&self, mark: PitchMark) -> bool {
        self.marks.contains(&mark)
    }

    pub fn cross_staff(&self) -> Option<Staff> {
        self.marks.iter().find_map(|m| match m {
            PitchMark::CrossStaff(s) => Some(*s),
            _ => None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Head {
    Chord(Vec<Note>),
    Rest(Option<Staff>),
    BarRest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Articulation {
    Staccato,
    Staccatissimo,
    Tenuto,
    Accent,
    Marcato,
    MarcatoDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrnamentKind {
    Trill,
    TrillLine,
    Mordent,
    Pralltriller,
    LongMordent,
    LongPralltriller,
    Turn,
    InvertedTurn,
    DelayedTurn,
    DelayedInvertedTurn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArpKind {
    Plain,
    Up,
    Down,
    CrossStaff,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TokMark {
    Art(Articulation),
    Fermata {
        inverted: bool,
    },
    Ornament {
        kind: OrnamentKind,
        above: Option<i8>,
        below: Option<i8>,
    },
    Arp(ArpKind),
    Small,
    Tremolo(u8),
    Vel(f64),
    // `:v0`: the notes are drawn and do not sound.
    Mute,
    BeamBegin,
    BeamEnd,
    BeamBreak,
    StemUp,
    StemDown,
    SlurUp,
    SlurDown,
    GraceValue(u32),
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Slurs {
    pub ends: Vec<Option<u32>>,
    pub starts: Vec<SlurStart>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlurStart {
    pub number: Option<u32>,
    // `(u` and `(d`; the first slur of a token takes `:su` and `:sd` instead.
    pub place: Option<Place>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraceNote {
    pub notes: Vec<Note>,
    pub marks: Vec<TokMark>,
    pub slurs: Slurs,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Grace {
    pub slash: bool,
    pub notes: Vec<GraceNote>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub grace: Option<Grace>,
    pub head: Head,
    pub marks: Vec<TokMark>,
    pub slurs: Slurs,
    pub after: Option<Grace>,
}

impl Event {
    pub fn notes(&self) -> &[Note] {
        match &self.head {
            Head::Chord(n) => n,
            _ => &[],
        }
    }

    pub fn has(&self, mark: TokMark) -> bool {
        self.marks.contains(&mark)
    }

    pub fn vel(&self) -> Option<f64> {
        self.marks.iter().find_map(|m| match m {
            TokMark::Vel(v) => Some(*v),
            _ => None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pedal {
    Press,
    Change,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Above,
    Below,
}

pub const DYN_MARKS: [&str; 18] = [
    "pppp", "ppp", "pp", "p", "mp", "mf", "f", "ff", "fff", "ffff", "sf", "sfz", "sffz", "sfp",
    "fp", "fz", "rf", "rfz",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DynMark(pub u8);

impl DynMark {
    pub fn parse(s: &str) -> Option<DynMark> {
        DYN_MARKS
            .iter()
            .position(|m| *m == s)
            .map(|i| DynMark(u8::try_from(i).expect("DYN_MARKS has fewer than 256 marks")))
    }

    pub fn name(self) -> &'static str {
        DYN_MARKS[usize::from(self.0)]
    }

    pub fn is_momentary(self) -> bool {
        self.0 >= 10
    }

    pub fn default_velocity(self) -> f64 {
        const TABLE: [f64; 18] = [
            16.0, 26.0, 38.0, 48.0, 58.0, 68.0, 80.0, 92.0, 106.0, 120.0, 92.0, 92.0, 106.0, 92.0,
            80.0, 92.0, 92.0, 92.0,
        ];
        TABLE[usize::from(self.0)]
    }

    // `fp` and `sfp` fall back to `p` after their slot.
    pub fn after(self) -> Option<DynMark> {
        match self.name() {
            "fp" | "sfp" => DynMark::parse("p"),
            _ => None,
        }
    }
}

// A dynamic text with no standard mark: `"cresc."`, `"sempre pp"=33`.
#[derive(Clone, Debug, PartialEq)]
pub struct DynText {
    pub text: String,
    // The velocity from this slot on, for the two staves; `None` changes no level.
    pub level: Option<f64>,
    pub place: Option<Place>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dyn {
    pub before: Option<String>,
    pub mark: DynMark,
    pub after: Option<String>,
    pub place: Option<Place>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hairpin {
    Cresc,
    Dim,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub text: String,
    pub ext: bool,
    pub place: Option<Place>,
    pub frame: TextFrame,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextFrame {
    #[default]
    None,
    Box,
    Rehearsal,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Metronome {
    pub unit: char,
    pub dotted: bool,
    pub bpm: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TempoMark {
    // `=72`: plays at 72 quarters per minute and prints nothing.
    Sound(f64),
    // `_e=110`: plays at the metronome value and prints nothing.
    Hidden(Metronome),
    Shown { metronome: Metronome, parens: bool },
}

impl Metronome {
    pub fn quarters_per_minute(&self) -> f64 {
        let unit = match self.unit {
            'w' => 4.0,
            'h' => 2.0,
            'e' => 0.5,
            's' => 0.25,
            _ => 1.0,
        };
        let dotted = if self.dotted { 1.5 } else { 1.0 };
        self.bpm * unit * dotted
    }
}

impl TempoMark {
    pub fn quarters_per_minute(&self) -> f64 {
        match self {
            TempoMark::Sound(q) => *q,
            TempoMark::Hidden(m) | TempoMark::Shown { metronome: m, .. } => m.quarters_per_minute(),
        }
    }

    // A hidden quarter metronome is `=72`, the canonical form.
    pub fn hidden(m: Metronome) -> TempoMark {
        if m.unit == 'q' && !m.dotted {
            TempoMark::Sound(m.bpm)
        } else {
            TempoMark::Hidden(m)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tempo {
    pub text: Option<String>,
    pub mark: Option<TempoMark>,
    pub ext: bool,
    pub place: Option<Place>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clef {
    Treble,
    Bass,
    Alto,
    Tenor,
    Treble8va,
    Treble8vb,
    Bass8va,
    Bass8vb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ottava {
    Up8,
    Down8,
    Up15,
    Down15,
}

impl Ottava {
    // Octaves between the sounding and the written pitch.
    pub fn shift(self) -> i8 {
        match self {
            Ottava::Up8 => 1,
            Ottava::Down8 => -1,
            Ottava::Up15 => 2,
            Ottava::Down15 => -2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Meter {
    pub beats: Vec<u32>,
    pub beat_type: u32,
    pub pattern: Option<Vec<u32>>,
    pub symbol: Option<TimeSymbol>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeSymbol {
    Common,
    Cut,
}

impl Meter {
    pub fn four_four() -> Meter {
        Meter {
            beats: vec![4],
            beat_type: 4,
            pattern: None,
            symbol: None,
        }
    }

    pub fn numerator(&self) -> u32 {
        self.beats.iter().sum()
    }

    pub fn cells(&self) -> Result<Vec<Frac>, String> {
        let unit = Frac::new(1, i64::from(self.beat_type));
        let num = self.numerator();
        if matches!(self.beat_type, 1 | 2 | 4) {
            return Ok(vec![unit; num as usize]);
        }
        if !matches!(self.beat_type, 8 | 16 | 32) {
            return Err(format!(
                "time signature with lower number {} is not supported",
                self.beat_type
            ));
        }
        let pattern = self
            .pattern
            .as_deref()
            .or_else(|| (self.beats.len() > 1).then_some(self.beats.as_slice()));
        if let Some(p) = pattern {
            if p.iter().any(|g| !matches!(g, 2 | 3)) || p.iter().sum::<u32>() != num {
                return Err(
                    "a beat pattern has groups of 2 and 3 with the sum of the upper number".into(),
                );
            }
            return Ok(p.iter().map(|g| unit * i64::from(*g)).collect());
        }
        match num {
            3 | 6 | 9 | 12 | 18 | 24 => Ok(vec![unit * 3; (num / 3) as usize]),
            2 | 4 => Ok(vec![unit * 2; (num / 2) as usize]),
            _ => {
                let den = self.beat_type;
                Err(format!(
                    "time={num}/{den} needs a beat pattern, for example time={num}/{den}(2+2+3)"
                ))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub fifths: i8,
    pub mode: Option<Mode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Major,
    Minor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MetaField {
    Title,
    Subtitle,
    Composer,
    Arranger,
    Lyricist,
    Copyright,
}

impl MetaField {
    pub const ALL: [MetaField; 6] = [
        MetaField::Title,
        MetaField::Subtitle,
        MetaField::Composer,
        MetaField::Arranger,
        MetaField::Lyricist,
        MetaField::Copyright,
    ];

    pub fn name(self) -> &'static str {
        match self {
            MetaField::Title => "title",
            MetaField::Subtitle => "subtitle",
            MetaField::Composer => "composer",
            MetaField::Arranger => "arranger",
            MetaField::Lyricist => "lyricist",
            MetaField::Copyright => "copyright",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarStyle {
    Normal,
    Double,
    Final,
    Dashed,
    Dotted,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BracketEnd {
    Open,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Jump {
    Dc,
    Ds,
    DcFine,
    DsFine,
    DcCoda,
    DsCoda,
    ToCoda,
    Fine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sign {
    Segno,
    Coda,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Break {
    System,
    Page,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PedalStyle {
    Line,
    Sign,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Attr {
    Format(u32),
    Meta(MetaField, String),
    Key(Key),
    Time(Meter),
    Barline {
        style: BarStyle,
        fermata: bool,
    },
    Cells(u32),
    Repeat {
        start: bool,
        end: Option<u32>,
    },
    Volta {
        numbers: Vec<u32>,
        end: Option<BracketEnd>,
    },
    Sign(Sign),
    Jump(Jump),
    Break(Break),
    PedalStyle(PedalStyle),
    VelMark(DynMark, f64),
    VelLane(Lane, f64),
    Voicing(Lane, f64),
    Program(u32),
    Volume(f64),
    Pan(f64),
    // `staves=1`: a score with only the upper staff.
    OneStaff,
}

impl Attr {
    pub fn order(&self) -> (u8, u32) {
        match self {
            Attr::Format(_) => (0, 0),
            Attr::Meta(f, _) => (1, *f as u32),
            Attr::Key(_) => (2, 0),
            Attr::Time(_) => (3, 0),
            Attr::Barline { .. } => (4, 0),
            Attr::Cells(_) => (5, 0),
            Attr::Repeat { .. } => (6, 0),
            Attr::Volta { .. } => (7, 0),
            Attr::Sign(_) => (8, 0),
            Attr::Jump(_) => (9, 0),
            Attr::Break(_) => (10, 0),
            Attr::PedalStyle(_) => (11, 0),
            Attr::VelMark(m, _) => (12, u32::from(m.0)),
            Attr::VelLane(l, _) => {
                let (a, b, _) = l.order();
                (13, u32::from(a) * 16 + u32::from(b))
            }
            Attr::Voicing(l, _) => {
                let (a, b, _) = l.order();
                (14, u32::from(a) * 16 + u32::from(b))
            }
            Attr::Program(_) => (15, 0),
            Attr::Volume(_) => (16, 0),
            Attr::Pan(_) => (17, 0),
            Attr::OneStaff => (18, 0),
        }
    }

    pub fn header_only(&self) -> bool {
        matches!(
            self,
            Attr::Format(_)
                | Attr::OneStaff
                | Attr::Meta(..)
                | Attr::PedalStyle(_)
                | Attr::Program(_)
                | Attr::Volume(_)
                | Attr::Pan(_)
        )
    }

    pub fn bar_only(&self) -> bool {
        matches!(
            self,
            Attr::Barline { .. }
                | Attr::Cells(_)
                | Attr::Repeat { .. }
                | Attr::Volta { .. }
                | Attr::Sign(_)
                | Attr::Jump(_)
                | Attr::Break(_)
        )
    }
}
