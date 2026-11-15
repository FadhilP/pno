use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Row {
    Position,
    PageSetup,
    Metadata,
    CreditCopy,
    SecondHeaderText,
    Font,
    Color,
    SameNameVelocity,
    Duplicate,
    ChordSymbols,
    Labels,
    JumpText,
    RestPosition,
    TupletBracket,
    TextBracket,
    TempoStaves,
    TableVelocity,
    CueElement,
    HiddenSize,
    PedalSignChange,
    PedalStaff,
    RoundedTuplets,
    Stems,
    Beams,
    BrokenTies,
    BrokenSlurs,
}

impl Row {
    fn text(self) -> &'static str {
        match self {
            Row::Position => "coordinates and sizes (by design)",
            Row::PageSetup => "page setup and engraving style (by design)",
            Row::Metadata => "file metadata (by design)",
            Row::SecondHeaderText => {
                "a second, different text for one header field (by design; the first stays)"
            }
            Row::CreditCopy => "page texts with no type that repeat a header field (by design)",
            Row::Font => "font style of text (by design)",
            Row::Color => "colors (by design)",
            Row::SameNameVelocity => {
                "the velocity of a second mark with the same name in one bar (by design); the note velocities stay"
            }
            Row::Duplicate => "duplicate marks on one chord (by design)",
            Row::ChordSymbols => "chord symbols (by design)",
            Row::Labels => "bar number labels of the source (by design)",
            Row::JumpText => "words of jump, sign and volta texts (by design)",
            Row::RestPosition => "the vertical position of rests (by design)",
            Row::TupletBracket => "the bracket of a tuplet (by design)",
            Row::TextBracket => "text lines with a bracket (by design)",
            Row::TempoStaves => {
                "the staves that show a tempo mark (by design; pno shows it on the top staff only)"
            }
            Row::TableVelocity => {
                "dynamic marks with no velocity; they get the table velocity (by design)"
            }
            Row::CueElement => "the cue element of small notes; pno plays small notes (by design)",
            Row::HiddenSize => "the size of notes that are not drawn (by design)",
            Row::RoundedTuplets => {
                "measures with tuplet lengths that the divisions round; pno writes the exact lengths (by design)"
            }
            Row::PedalStaff => {
                "pedal marks on the upper staff; pno writes them below the lower staff (by design)"
            }
            Row::PedalSignChange => {
                "a `Ped.` while the pedal is down (pno writes a pedal change) or a `*` with no pedal down (by design)"
            }
            Row::Stems => "stems (keep them with --keep stems)",
            Row::Beams => "beams (keep them with --keep beams)",
            Row::BrokenTies => "ties with no start or no end note (an error in the source)",
            Row::BrokenSlurs => "slurs with no start or no end note (an error in the source)",
        }
    }
}

#[derive(Debug, Default)]
pub struct Report {
    counts: BTreeMap<Row, usize>,
    notes: Vec<String>,
    pub accidentals: Option<bool>,
}

impl Report {
    pub fn drop(&mut self, row: Row) {
        self.drop_n(row, 1);
    }

    pub fn drop_n(&mut self, row: Row, n: usize) {
        if n > 0 {
            *self.counts.entry(row).or_default() += n;
        }
    }

    pub fn note(&mut self, text: String) {
        self.notes.push(text);
    }

    pub fn lines(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .counts
            .iter()
            .map(|(row, n)| format!("dropped {n}: {}", row.text()))
            .collect();
        out.extend(self.notes.iter().map(|n| format!("note: {n}")));
        out
    }
}
