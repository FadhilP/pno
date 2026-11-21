mod a2p;
mod accidental;
mod analysis;
mod analyze;
mod audio;
mod check;
mod diag;
mod edit;
mod fmt;
mod frac;
mod midi;
mod model;
mod musicxml;
mod perform;
mod render;
mod sheet;
mod text;
mod video;

use std::ffi::OsString;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::diag::{Diag, Severity};
use crate::model::Score;

#[derive(Parser)]
#[command(
    name = "pno",
    version,
    about = "A piano score format and CLI for AI agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Check syntax, bar fill, note values, ties and spans
    Check {
        file: PathBuf,
        /// Bars, for example 9-10 or 9-10,13-14; without it, all bars
        bars: Option<String>,
    },
    /// Write the file in its canonical form, then check it
    Fmt {
        file: PathBuf,
        /// Split invalid `-` runs with ties and renumber the bars
        #[arg(long)]
        fix: bool,
        /// Report if the file is not formatted; do not write it
        #[arg(long)]
        check: bool,
    },
    /// Write MusicXML or MIDI; PDF, PNG, SVG and audio through MuseScore; audio and video (.mp4) through sfizz with --sound
    Export(ExportArgs),
    /// Read MusicXML into a pno file; .mxl and .mscz go through MuseScore
    Import {
        file: PathBuf,
        /// Output file; without it the import writes the input name with .pno, if no such file exists
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Presentation detail to keep, for example --keep stems,beams
        #[arg(long, value_delimiter = ',')]
        keep: Vec<KeepItem>,
    },
    /// Write a trimmed PNG of a bar range, or of the score, through MuseScore
    Render {
        file: PathBuf,
        /// Bar range, for example 9-10; without it, the score with its line breaks
        bars: Option<String>,
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Image resolution in dots per inch
        #[arg(long, default_value_t = 150)]
        dpi: u32,
    },
    /// Print the pitches of each hand at each onset, with held notes and the pedal
    Sound {
        file: PathBuf,
        /// Bars, for example 9-10 or 9-10,13-14; without it, all bars
        bars: Option<String>,
        /// Also show the notes that the pedal holds
        #[arg(long)]
        pedal: bool,
        /// Also show the MIDI velocity of each new note, for example C5@80
        #[arg(long)]
        velocity: bool,
    },
    /// Print the chord of each beat, dissonances between the hands, and repeated bars
    Analyze {
        file: PathBuf,
        /// Bars, for example 9-10 or 9-10,13-14; without it, all bars
        bars: Option<String>,
        /// Also list M2, tritone, m7 and M9
        #[arg(long)]
        all: bool,
    },
    /// Insert, delete or copy bars; the file must be formatted and pass check
    Bars {
        #[command(subcommand)]
        op: BarsOp,
    },
    /// Replace lines by bar and lane, or add a lane to a bar, for example `pno set f.pno '9 lh | C3 | - | - | - |'`; the file must be formatted and pass check
    Set {
        file: PathBuf,
        #[arg(required = true)]
        lines: Vec<String>,
    },
    /// Transpose by semitones; with a bar range the key signatures do not change
    Transpose {
        file: PathBuf,
        #[arg(allow_hyphen_values = true)]
        semitones: i32,
        /// Bar range, for example 9-10
        bars: Option<String>,
    },
    /// Write the phrases that `export --phrase` finds as `/` marks in phrase lines, to correct them; the file must be formatted, pass check and have no phrase lines
    Phrase { file: PathBuf },
    /// Print the list of format sections, or one section, for example `pno spec 3.7`
    Spec { section: Option<String> },
    /// Run the audio-to-pno evidence tool; the first run sets up its Python environment and models. `pno a2p -h` lists its commands
    #[command(disable_help_flag = true)]
    A2p {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
}

#[derive(clap::Args)]
struct ExportArgs {
    file: PathBuf,
    /// Output files; the extension of each selects its format. MIDI, audio and video share one performance
    #[arg(required = true)]
    out: Vec<PathBuf>,
    /// MIDI, audio and video: play with small changes of timing and velocity, as a pianist does
    #[arg(long)]
    humanize: bool,
    /// The seed of --humanize; the same seed gives the same performance
    #[arg(long, requires = "humanize")]
    seed: Option<u64>,
    /// MIDI, audio and video: shape each phrase with a curve of tempo and velocity; AMOUNT scales the curve (1 if not given, at most 4)
    #[arg(long, num_args = 0..=1, default_missing_value = "1", value_name = "AMOUNT")]
    phrase: Option<f64>,
    /// Audio and video: play the MIDI with this SFZ instrument through sfizz, not with MuseScore
    #[arg(long)]
    sound: Option<PathBuf>,
    /// MIDI, audio and video: play only a bar range, for example 9-14
    #[arg(long)]
    bars: Option<String>,
    /// Video: the note colours of the upper and the lower staff, for example 5CC478,5494E4
    #[arg(long, value_name = "RH,LH")]
    colors: Option<String>,
    /// Video: show the score in a band below the keyboard or at the top, with a cursor that follows the music
    #[arg(long, value_enum, num_args = 0..=1, default_missing_value = "below")]
    sheet: Option<sheet::Place>,
}

#[derive(Subcommand)]
enum BarsOp {
    /// Insert bars of rests before a bar, or at the end with `end`
    Insert {
        file: PathBuf,
        before: String,
        #[arg(long, default_value_t = 1)]
        count: usize,
    },
    /// Delete a bar range, for example 9-10
    Delete { file: PathBuf, bars: String },
    /// Copy a bar range to the place before a bar, or to the end with `end`
    Copy {
        file: PathBuf,
        bars: String,
        before: String,
    },
}

const SPEC: &str = include_str!("../SPEC.md");

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum KeepItem {
    Stems,
    Beams,
}

struct Failure {
    code: u8,
    msg: String,
}

fn usage(msg: impl Into<String>) -> Failure {
    Failure {
        code: 2,
        msg: msg.into(),
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(f) => {
            if !f.msg.is_empty() {
                eprintln!("{}", f.msg);
            }
            ExitCode::from(f.code)
        }
    }
}

fn load(file: &Path) -> Result<(Score, Vec<Diag>), Failure> {
    let src = std::fs::read_to_string(file)
        .map_err(|e| usage(format!("cannot read {}: {e}", file.display())))?;
    let parsed = text::parse::parse(&src);
    Ok((parsed.score, parsed.diags))
}

fn bar_range(score: &Score, spec: &str) -> Result<RangeInclusive<usize>, Failure> {
    let (from, to) = spec.split_once('-').unwrap_or((spec, spec));
    let find = |label: &str| {
        score
            .bars
            .iter()
            .position(|b| b.label == label)
            .ok_or_else(|| usage(format!("there is no bar {label}")))
    };
    let (a, b) = (find(from)?, find(to)?);
    if a > b {
        return Err(usage(format!("the bar range {spec} is reversed")));
    }
    Ok(a..=b)
}

// Playback from bar 1 starts with the pickup before it.
fn with_pickup(score: &Score, range: RangeInclusive<usize>) -> RangeInclusive<usize> {
    if *range.start() == 1 && score.bars[0].label == "0" {
        0..=*range.end()
    } else {
        range
    }
}

fn bar_ranges(score: &Score, spec: &str) -> Result<Vec<RangeInclusive<usize>>, Failure> {
    spec.split(',')
        .map(|s| bar_range(score, s.trim()))
        .collect()
}

fn bar_list(score: &Score, spec: Option<&str>) -> Result<Vec<usize>, Failure> {
    let Some(spec) = spec else {
        return Ok((0..score.bars.len()).collect());
    };
    let mut bars: Vec<usize> = bar_ranges(score, spec)?.into_iter().flatten().collect();
    bars.sort_unstable();
    bars.dedup();
    Ok(bars)
}

fn bar_place(score: &Score, label: &str) -> Result<usize, Failure> {
    if label == "end" {
        return Ok(score.bars.len());
    }
    bar_range(score, label).map(|r| *r.start())
}

fn editable(file: &Path) -> Result<Score, Failure> {
    let score = checked(file)?;
    let src = std::fs::read_to_string(file)
        .map_err(|e| usage(format!("cannot read {}: {e}", file.display())))?;
    let canonical =
        fmt::format(score.clone(), fmt::Fix::No).map_err(|e| Failure { code: 1, msg: e })?;
    if canonical != src {
        return Err(Failure {
            code: 1,
            msg: format!("{} is not formatted; run `pno fmt` first", file.display()),
        });
    }
    Ok(score)
}

fn save(file: &Path, score: Score) -> Result<(), Failure> {
    let text = fmt::format(score, fmt::Fix::Yes).map_err(|e| Failure { code: 1, msg: e })?;
    let parsed = text::parse::parse(&text);
    let errors: Vec<Diag> = check::check(&parsed.score, &parsed.diags)
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    if !errors.is_empty() {
        print_diags(&errors);
        return Err(Failure {
            code: 1,
            msg: format!(
                "the edit gives {} errors; {} is not changed",
                errors.len(),
                file.display()
            ),
        });
    }
    std::fs::write(file, text)
        .map_err(|e| usage(format!("cannot write {}: {e}", file.display())))?;
    println!("wrote {}", file.display());
    Ok(())
}

fn spec(section: Option<&str>) -> Result<(), Failure> {
    let sections: Vec<&str> = SPEC
        .split(
            "
## ",
        )
        .skip(1)
        .collect();
    let Some(want) = section else {
        for s in &sections {
            println!("{}", s.lines().next().unwrap_or(""));
        }
        println!("print one section with `pno spec <number>`, for example `pno spec 3.7`");
        return Ok(());
    };
    let found = sections
        .iter()
        .find(|s| s.split_whitespace().next() == Some(want))
        .ok_or_else(|| {
            usage(format!(
                "there is no section {want}; run `pno spec` for the list"
            ))
        })?;
    println!("## {}", found.trim_end());
    Ok(())
}

// Lines that only get new padding (a wider lane name in the file) are not changes.
fn print_changed_lines(old: &str, new: &str) {
    let words = |l: &str| l.split_whitespace().collect::<Vec<_>>().join(" ");
    let old_words: Vec<String> = old.lines().map(words).collect();
    let new_words: Vec<String> = new.lines().map(words).collect();
    for l in old
        .lines()
        .filter(|l| !l.is_empty() && !new_words.contains(&words(l)))
    {
        println!("-{l}");
    }
    for l in new
        .lines()
        .filter(|l| !l.is_empty() && !old_words.contains(&words(l)))
    {
        println!("+{l}");
    }
}

fn print_diags(diags: &[Diag]) -> usize {
    for d in diags {
        println!("{d}");
    }
    diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count()
}

fn checked(file: &Path) -> Result<Score, Failure> {
    let (score, parse_diags) = load(file)?;
    let diags = check::check(&score, &parse_diags);
    let errors: Vec<Diag> = diags
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    if !errors.is_empty() {
        print_diags(&errors);
        return Err(Failure {
            code: 1,
            msg: format!("{} errors; run `pno check` for details", errors.len()),
        });
    }
    Ok(score)
}

// A range shows only its own diagnostics, but `ok` needs a clean file.
fn report_check(
    score: &Score,
    diags: Vec<Diag>,
    ranges: Option<&[RangeInclusive<usize>]>,
) -> Result<(), Failure> {
    let (shown, others): (Vec<Diag>, Vec<Diag>) = diags
        .into_iter()
        .partition(|d| ranges.is_none_or(|rs| rs.iter().any(|r| in_range(score, r, d.line))));
    let errors = print_diags(&shown);
    let warnings = shown.len() - errors;
    let other_errors = others
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    let elsewhere = if other_errors > 0 {
        format!("; {other_errors} errors in other bars")
    } else {
        String::new()
    };
    if errors + other_errors > 0 {
        return Err(Failure {
            code: 1,
            msg: format!("{errors} errors, {warnings} warnings{elsewhere}"),
        });
    }
    println!("ok: {} bars, {warnings} warnings", score.bars.len());
    Ok(())
}

fn in_range(score: &Score, range: &RangeInclusive<usize>, line: usize) -> bool {
    let first = score.bars[*range.start()].src;
    let next = score
        .bars
        .get(range.end() + 1)
        .map_or(usize::MAX, |b| b.src);
    (first..next).contains(&line)
}

fn musescore_from_xml(xml: &str, out: &Path, args: &[&str]) -> Result<Vec<PathBuf>, String> {
    let tmp = render::temp_path("musicxml");
    std::fs::write(&tmp, xml).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    let result = render::convert(&tmp, out, args);
    let _ = std::fs::remove_file(&tmp);
    result
}

fn through_musescore(xml: &str, out: &Path, args: &[&str]) -> Result<(), Failure> {
    for p in musescore_from_xml(xml, out, args).map_err(|e| Failure { code: 1, msg: e })? {
        println!("wrote {}", p.display());
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Output {
    Midi,
    Audio,
    Video,
    MusicXml,
    // MuseScore's score files: they keep the pedal marks, as MusicXML does.
    ScoreFile,
    // PDF, PNG, SVG and the other formats of MuseScore.
    Notation,
}

fn output_kind(path: &Path) -> Output {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "mid" | "midi" => Output::Midi,
        "wav" | "mp3" | "flac" | "ogg" => Output::Audio,
        "mp4" => Output::Video,
        "musicxml" | "xml" => Output::MusicXml,
        "mxl" | "mscz" | "mscx" => Output::ScoreFile,
        _ => Output::Notation,
    }
}

// What the outputs of one export share.
struct Sources<'a> {
    // The performance with no release tail, for MIDI.
    plain: Option<perform::Performance>,
    // The performance with the release tail, for audio and video with --sound.
    released: Option<perform::Performance>,
    sound: Option<&'a Path>,
    hands: video::Hands,
    xml: Option<String>,
    // The notation without pedal marks, for PDF, PNG, SVG and the other picture formats.
    notation_xml: Option<String>,
    sheet: Option<sheet::Source>,
}

fn export_one(out: &Path, kind: Output, src: &Sources) -> Result<Vec<PathBuf>, String> {
    let missing = || format!("{}: nothing to export it from", out.display());
    let write = |bytes: &[u8]| {
        std::fs::write(out, bytes).map_err(|e| format!("cannot write {}: {e}", out.display()))
    };
    match (kind, src.sound) {
        (Output::Midi, _) => write(&midi::write(src.plain.as_ref().ok_or_else(missing)?)?)?,
        (Output::Video, Some(sfz)) => {
            let p = src.released.as_ref().ok_or_else(missing)?;
            let bytes = midi::write(p)?;
            let wav = render::temp_path("wav");
            let silent = render::temp_path("mp4");
            let (sound, picture) = std::thread::scope(|s| {
                let sound = s.spawn(|| audio::render(&bytes, sfz, &wav));
                let picture = video::render(p, &src.hands, src.sheet.as_ref(), &silent);
                (sound.join(), picture)
            });
            let sound = sound.unwrap_or_else(|_| Err("the audio render failed".into()));
            let result = sound
                .and(picture)
                .and_then(|()| video::mux(&silent, &wav, out));
            let _ = std::fs::remove_file(&wav);
            let _ = std::fs::remove_file(&silent);
            result?;
        }
        (Output::Audio, Some(sfz)) => {
            let p = src.released.as_ref().ok_or_else(missing)?;
            audio::render(&midi::write(p)?, sfz, out)?;
        }
        (Output::MusicXml, _) => write(src.xml.as_ref().ok_or_else(missing)?.as_bytes())?,
        (Output::Video, None) => return Err(missing()),
        (Output::Audio, None) | (Output::ScoreFile, _) => {
            return musescore_from_xml(src.xml.as_ref().ok_or_else(missing)?, out, &[]);
        }
        (Output::Notation, _) => {
            return musescore_from_xml(src.notation_xml.as_ref().ok_or_else(missing)?, out, &[]);
        }
    }
    Ok(vec![out.to_path_buf()])
}

fn random_seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    u64::from(nanos) ^ u64::from(std::process::id()) << 32
}

fn read_musicxml(file: &Path) -> Result<String, Failure> {
    let ext = file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let read = |p: &Path| {
        std::fs::read_to_string(p).map_err(|e| usage(format!("cannot read {}: {e}", p.display())))
    };
    match ext.as_str() {
        "musicxml" | "xml" => read(file),
        "mxl" | "mscz" | "mscx" => {
            let tmp = render::temp_path("musicxml");
            render::convert(file, &tmp, &[]).map_err(|e| Failure { code: 1, msg: e })?;
            let text = read(&tmp);
            let _ = std::fs::remove_file(&tmp);
            text
        }
        _ => Err(usage(format!(
            "{} is not a MusicXML file (.musicxml, .xml, .mxl or .mscz)",
            file.display()
        ))),
    }
}

fn run(cmd: Cmd) -> Result<(), Failure> {
    match cmd {
        Cmd::Check { file, bars } => {
            let (score, parse_diags) = load(&file)?;
            let diags = check::check(&score, &parse_diags);
            let ranges = bars.map(|b| bar_ranges(&score, &b)).transpose()?;
            report_check(&score, diags, ranges.as_deref())
        }
        Cmd::Fmt { file, fix, check } => {
            let src = std::fs::read_to_string(&file)
                .map_err(|e| usage(format!("cannot read {}: {e}", file.display())))?;
            let parsed = text::parse::parse(&src);
            if !parsed.diags.is_empty() {
                print_diags(&parsed.diags);
                return Err(Failure {
                    code: 1,
                    msg: "fmt needs a file that parses; fix the errors first".into(),
                });
            }
            let mode = if fix { fmt::Fix::Yes } else { fmt::Fix::No };
            let out = fmt::format(parsed.score, mode).map_err(|e| Failure { code: 1, msg: e })?;
            if out == src {
                println!("{} is formatted", file.display());
            } else if check {
                return Err(Failure {
                    code: 1,
                    msg: format!("{} is not formatted", file.display()),
                });
            } else {
                std::fs::write(&file, &out)
                    .map_err(|e| usage(format!("cannot write {}: {e}", file.display())))?;
                println!("formatted {}", file.display());
            }
            let parsed = text::parse::parse(&out);
            let diags = check::check(&parsed.score, &parsed.diags);
            report_check(&parsed.score, diags, None)
        }
        Cmd::Export(args) => export(args),
        Cmd::Import { file, out, keep } => import(&file, out, &keep),
        Cmd::Render {
            file,
            bars,
            out,
            dpi,
        } => render_png(&file, bars.as_deref(), out, dpi),
        Cmd::Sound {
            file,
            bars,
            pedal,
            velocity,
        } => {
            let score = checked(&file)?;
            let bars = bar_list(&score, bars.as_deref())?;
            let notes = if pedal {
                analyze::PedalNotes::Show
            } else {
                analyze::PedalNotes::Hide
            };
            let a = analysis::Analysis::new(&score);
            let velocities = if velocity {
                analyze::Velocities::Show
            } else {
                analyze::Velocities::Hide
            };
            print!("{}", analyze::sound(&a, &bars, notes, velocities));
            Ok(())
        }
        Cmd::Analyze { file, bars, all } => {
            let score = checked(&file)?;
            let bars = bar_list(&score, bars.as_deref())?;
            let which = if all {
                analyze::Dissonances::All
            } else {
                analyze::Dissonances::Strong
            };
            let a = analysis::Analysis::new(&score);
            print!("{}", analyze::analyze(&a, &bars, which));
            Ok(())
        }
        Cmd::Bars { op } => edit_bars(op),
        Cmd::Transpose {
            file,
            semitones,
            bars,
        } => {
            let mut score = editable(&file)?;
            let range = bars.map(|b| bar_range(&score, &b)).transpose()?;
            let n = edit::transpose(&mut score, semitones, range.as_ref())
                .map_err(|msg| Failure { code: 1, msg })?;
            println!("transposed {n} notes");
            save(&file, score)
        }
        Cmd::Set { file, lines } => set(&file, &lines),
        Cmd::Phrase { file } => mark_phrases(&file),
        Cmd::Spec { section } => spec(section.as_deref()),
        Cmd::A2p { args } => match a2p::run(&args).map_err(|msg| Failure { code: 1, msg })? {
            0 => Ok(()),
            code => Err(Failure {
                code: u8::try_from(code).unwrap_or(1),
                msg: String::new(),
            }),
        },
    }
}

fn export(args: ExportArgs) -> Result<(), Failure> {
    let ExportArgs {
        file,
        out,
        humanize,
        seed,
        phrase,
        sound,
        bars,
        colors,
        sheet,
    } = args;
    let score = checked(&file)?;
    let a = analysis::Analysis::new(&score);
    let kinds: Vec<Output> = out.iter().map(|p| output_kind(p)).collect();
    let has = |k: Output| kinds.contains(&k);
    let sampled = has(Output::Audio) || has(Output::Video);
    if sound.is_some() && !sampled {
        return Err(usage("--sound applies only to audio and video output"));
    }
    if has(Output::Video) && sound.is_none() {
        return Err(usage("a video needs --sound for its audio"));
    }
    if colors.is_some() && !has(Output::Video) {
        return Err(usage("--colors applies only to video output"));
    }
    if sheet.is_some() && !has(Output::Video) {
        return Err(usage("--sheet applies only to video output"));
    }
    let sampled_with_sound = sampled && sound.is_some();
    let performed = has(Output::Midi) || sampled_with_sound;
    let shaped = humanize || phrase.is_some() || bars.is_some();
    if shaped && !performed {
        return Err(usage(
            "--humanize, --phrase, --sound and --bars apply only to MIDI, audio and video output",
        ));
    }
    if shaped && has(Output::Audio) && sound.is_none() {
        return Err(usage(
            "--humanize, --phrase and --bars need --sound for audio: MuseScore plays only the notation",
        ));
    }
    let hands = match &colors {
        Some(c) => video::Hands::parse(c).map_err(usage)?,
        None => video::Hands::default(),
    };
    let shaping = match phrase {
        None => perform::Shaping::Off,
        Some(f) if f > 0.0 && f <= 4.0 => perform::Shaping::Phrases(f),
        Some(_) => return Err(usage("--phrase AMOUNT is above 0 and at most 4")),
    };
    let bars = bars
        .map(|b| bar_range(&score, &b))
        .transpose()?
        .map(|r| with_pickup(&score, r));
    let mode = match (humanize, seed) {
        (false, _) => perform::Humanize::Off,
        (true, Some(s)) => perform::Humanize::Seed(s),
        (true, None) => {
            let s = random_seed();
            println!("humanize seed {s}");
            perform::Humanize::Seed(s)
        }
    };
    let perform_with = |tail: perform::Tail| {
        let opts = perform::Options {
            humanize: mode,
            shaping,
            bars: bars.clone(),
            tail,
        };
        perform::perform(&a, &opts).map_err(|msg| Failure { code: 1, msg })
    };
    let musescore_audio = has(Output::Audio) && sound.is_none();
    let needs_xml = has(Output::MusicXml) || has(Output::ScoreFile) || musescore_audio;
    let xml_with = |pedal| {
        musicxml::export(&a, musicxml::Scope::Full, pedal).map_err(|e| Failure { code: 1, msg: e })
    };
    let src = Sources {
        plain: has(Output::Midi)
            .then(|| perform_with(perform::Tail::None))
            .transpose()?,
        released: sampled_with_sound
            .then(|| perform_with(perform::Tail::Release))
            .transpose()?,
        sound: sound.as_deref(),
        hands,
        xml: needs_xml
            .then(|| xml_with(musicxml::PedalMarks::Show))
            .transpose()?,
        notation_xml: has(Output::Notation)
            .then(|| xml_with(musicxml::PedalMarks::Hide))
            .transpose()?,
        sheet: sheet
            .map(|place| sheet_source(&a, place))
            .transpose()
            .map_err(|e| Failure { code: 1, msg: e })?,
    };
    let mut failed: Vec<String> = Vec::new();
    for (path, kind) in out.iter().zip(&kinds) {
        match export_one(path, *kind, &src) {
            Ok(written) => written
                .iter()
                .for_each(|w| println!("wrote {}", w.display())),
            Err(e) => failed.push(e),
        }
    }
    if failed.is_empty() {
        return Ok(());
    }
    Err(Failure {
        code: 1,
        msg: failed.join("\n"),
    })
}

fn sheet_source(a: &analysis::Analysis, place: sheet::Place) -> Result<sheet::Source, String> {
    let all = 0..=a.score.bars.len().saturating_sub(1);
    Ok(sheet::Source {
        place,
        xml: musicxml::export(a, musicxml::Scope::Bars(all), musicxml::PedalMarks::Hide)?,
        bars: perform::bar_onsets(a),
    })
}

fn import(file: &Path, out: Option<PathBuf>, keep: &[KeepItem]) -> Result<(), Failure> {
    let target = if let Some(p) = out {
        p
    } else {
        let p = file.with_extension("pno");
        if p.exists() {
            return Err(usage(format!(
                "{} exists; give the output file with -o",
                p.display()
            )));
        }
        p
    };
    let xml = read_musicxml(file)?;
    let keep = musicxml::import::Keep {
        stems: keep.contains(&KeepItem::Stems),
        beams: keep.contains(&KeepItem::Beams),
    };
    let imported = musicxml::import::import(&xml, keep).map_err(|e| Failure { code: 1, msg: e })?;
    std::fs::write(&target, imported.text)
        .map_err(|e| usage(format!("cannot write {}: {e}", target.display())))?;
    println!("wrote {}", target.display());
    for line in imported.report {
        println!("{line}");
    }
    Ok(())
}

fn render_png(
    file: &Path,
    bars: Option<&str>,
    out: Option<PathBuf>,
    dpi: u32,
) -> Result<(), Failure> {
    let score = checked(file)?;
    let scope = match bars {
        Some(b) => musicxml::Scope::Bars(bar_range(&score, b)?),
        None => musicxml::Scope::Full,
    };
    let a = analysis::Analysis::new(&score);
    let xml = musicxml::export(&a, scope, musicxml::PedalMarks::Show)
        .map_err(|e| Failure { code: 1, msg: e })?;
    let out = out.unwrap_or_else(|| {
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("score");
        let part = bars.map(|b| format!("-{b}")).unwrap_or_default();
        file.with_file_name(format!("{stem}{part}.png"))
    });
    let style = render::temp_path("mss");
    std::fs::write(&style, render::IMAGE_STYLE)
        .map_err(|e| usage(format!("cannot write {}: {e}", style.display())))?;
    let style_arg = style.to_string_lossy();
    let args = ["-S", &style_arg, "-T", "10", "-r", &dpi.to_string()];
    let result = through_musescore(&xml, &out, &args);
    let _ = std::fs::remove_file(&style);
    result
}

fn edit_bars(op: BarsOp) -> Result<(), Failure> {
    let fail = |msg: String| Failure { code: 1, msg };
    let (file, score) = match op {
        BarsOp::Insert {
            file,
            before,
            count,
        } => {
            let mut score = editable(&file)?;
            let at = bar_place(&score, &before)?;
            edit::insert(&mut score, at, count).map_err(fail)?;
            (file, score)
        }
        BarsOp::Delete { file, bars } => {
            let mut score = editable(&file)?;
            let range = bar_range(&score, &bars)?;
            for note in edit::delete(&mut score, range) {
                println!("removed {note}");
            }
            (file, score)
        }
        BarsOp::Copy { file, bars, before } => {
            let mut score = editable(&file)?;
            let range = bar_range(&score, &bars)?;
            let at = bar_place(&score, &before)?;
            edit::copy(&mut score, range, at).map_err(fail)?;
            (file, score)
        }
    };
    save(&file, score)
}

fn set(file: &Path, lines: &[String]) -> Result<(), Failure> {
    let score = editable(file)?;
    let old = std::fs::read_to_string(file)
        .map_err(|e| usage(format!("cannot read {}: {e}", file.display())))?;
    let labels: Vec<&str> = score.bars.iter().map(|b| b.label.as_str()).collect();
    let text = edit::set_lines(&old, &labels, lines).map_err(usage)?;
    let parsed = text::parse::parse(&text);
    if !parsed.diags.is_empty() {
        print_diags(&parsed.diags);
        return Err(Failure {
            code: 1,
            msg: format!(
                "the new lines do not parse; {} is not changed",
                file.display()
            ),
        });
    }
    save(file, parsed.score)?;
    let new = std::fs::read_to_string(file)
        .map_err(|e| usage(format!("cannot read {}: {e}", file.display())))?;
    print_changed_lines(&old, &new);
    Ok(())
}

fn mark_phrases(file: &Path) -> Result<(), Failure> {
    let mut score = editable(file)?;
    let found = edit::add_phrases(&mut score).map_err(|msg| Failure { code: 1, msg })?;
    for p in &found {
        let bar = &score.bars[p.bar];
        let cell = bar.layout.cell_at(p.time) + 1;
        println!(
            "{} cell {cell}: {} ({})",
            bar.label,
            p.mark.text(),
            p.reason.text()
        );
    }
    println!("marked {} phrases", found.len());
    save(file, score)
}
