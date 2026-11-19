mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::compare::{self, Detail};
use common::{path, pno, temp_dir};

enum Outcome {
    RoundTrip,
    Stop(String),
    Unclassified(String),
    Fault(String),
}

// Bars of the source, and those that come back equal. A stop or a fault before the
// comparison gives no equal bars.
#[derive(Default)]
struct Bars {
    total: usize,
    // `None` when the file does not reach the comparison.
    equal: Option<usize>,
}

fn scores(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for p in entries.flatten().map(|e| e.path()) {
        if p.is_dir() {
            scores(&p, out);
            continue;
        }
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext == "musicxml" || ext == "xml" {
            out.push(p);
        }
    }
}

fn classify(err: &str) -> Outcome {
    let feature = err
        .trim()
        .strip_suffix(" is not supported")
        .and_then(|head| head.split_once("the import stops"))
        .and_then(|(_, tail)| tail.split_once(": "))
        .map(|(_, what)| what.to_string());
    let Some(what) = feature else {
        return Outcome::Fault(first_line(err));
    };
    if let Some(attr) = what.strip_prefix("the attribute ") {
        let name = attr.split('=').next().unwrap_or(attr);
        let of = attr.rsplit_once(" of ").map_or("", |(_, el)| el);
        return Outcome::Unclassified(format!("attribute {name} of {of}"));
    }
    let unknown_element = what.starts_with('<') && what.contains("> in <");
    if unknown_element {
        return Outcome::Unclassified(what);
    }
    Outcome::Stop(what)
}

fn first_line(s: &str) -> String {
    let line = s.trim().lines().next().unwrap_or("");
    let generic: String = line
        .chars()
        .map(|c| if c.is_ascii_digit() { '#' } else { c })
        .collect();
    generic.chars().take(140).collect()
}

fn source_bars(file: &Path) -> usize {
    let Ok(xml) = std::fs::read_to_string(file) else {
        return 0;
    };
    std::panic::catch_unwind(|| compare::content(&xml, Detail::NoStemsBeams).measures.len())
        .unwrap_or(0)
}

fn outcome(file: &Path, dir: &Path, i: usize, bars: &mut Bars) -> Outcome {
    bars.total = source_bars(file);
    let imported = dir.join(format!("{i}.pno"));
    let run = pno(&["import", path(file), "-o", path(&imported)]);
    if run.code != 0 {
        return classify(&run.stderr);
    }
    let exported = dir.join(format!("{i}.musicxml"));
    let run = pno(&["export", path(&imported), path(&exported)]);
    if run.code != 0 {
        return Outcome::Fault(format!("export: {}", first_line(&run.stderr)));
    }
    let (Ok(want), Ok(got)) = (
        std::fs::read_to_string(file),
        std::fs::read_to_string(&exported),
    ) else {
        return Outcome::Fault("a file is not UTF-8".to_string());
    };
    // One file with a comparer panic must not end the report of the others.
    let diffs = std::panic::catch_unwind(|| {
        let want = compare::content(&want, Detail::NoStemsBeams);
        let got = compare::content(&got, Detail::NoStemsBeams);
        (
            compare::differences(&want, &got),
            compare::equal_measures(&want, &got),
        )
    });
    let diffs = diffs.map(|(d, equal)| {
        bars.equal = Some(equal);
        d
    });
    match diffs {
        Err(_) => Outcome::Fault("the comparer panics".to_string()),
        Ok(d) if d.is_empty() => Outcome::RoundTrip,
        Ok(d) => {
            if std::env::var_os("PNO_CORPUS_VERBOSE").is_some() {
                eprintln!(
                    "{}:
{}",
                    file.display(),
                    d[..d.len().min(4)].join(
                        "
"
                    )
                );
            }
            Outcome::Fault(format!("differs: {}", first_line(&d[0])))
        }
    }
}

fn print_groups(title: &str, groups: &BTreeMap<String, Vec<PathBuf>>) {
    let mut sorted: Vec<_> = groups.iter().collect();
    sorted.sort_by_key(|(_, files)| std::cmp::Reverse(files.len()));
    println!("{title}:");
    for (key, files) in sorted {
        println!(
            "  {:4}  {key}  (for example {})",
            files.len(),
            files[0].display()
        );
    }
}

#[test]
#[ignore = "needs a corpus folder in PNO_CORPUS"]
fn corpus_files_round_trip_or_stop_with_a_named_feature() {
    let dirs = std::env::var_os("PNO_CORPUS").expect("PNO_CORPUS names the corpus folders");
    let mut files = Vec::new();
    for d in std::env::split_paths(&dirs) {
        scores(&d, &mut files);
    }
    files.sort();
    assert!(!files.is_empty(), "no .musicxml files in PNO_CORPUS");
    let dir = temp_dir("corpus");
    let mut round_trips = 0;
    let mut stops: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    let mut unclassified: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    let mut faults: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    let (mut bars_total, mut bars_equal, mut bars_compared) = (0, 0, 0);
    for (i, f) in files.iter().enumerate() {
        let mut bars = Bars::default();
        let result = outcome(f, &dir, i, &mut bars);
        bars_total += bars.total;
        if let Some(equal) = bars.equal {
            bars_equal += equal;
            bars_compared += bars.total;
        }
        let group = match result {
            Outcome::RoundTrip => {
                round_trips += 1;
                continue;
            }
            Outcome::Stop(k) => stops.entry(k),
            Outcome::Unclassified(k) => unclassified.entry(k),
            Outcome::Fault(k) => faults.entry(k),
        };
        group.or_default().push(f.clone());
    }
    let count = |g: &BTreeMap<String, Vec<PathBuf>>| g.values().map(Vec::len).sum::<usize>();
    println!(
        "{} files: {round_trips} round trips ({:.1} %), {} named stops, {} unclassified, {} faults",
        files.len(),
        100.0 * f64::from(round_trips) / files.len() as f64,
        count(&stops),
        count(&unclassified),
        count(&faults)
    );
    let percent = |part: usize, all: usize| 100.0 * part as f64 / all.max(1) as f64;
    println!(
        "{bars_total} bars: {bars_equal} come back equal ({:.1} %); {:.1} % of the {bars_compared} bars of the files that the import and export pass",
        percent(bars_equal, bars_total),
        percent(bars_equal, bars_compared)
    );
    print_groups("named stops", &stops);
    print_groups("unclassified", &unclassified);
    print_groups("faults", &faults);
    assert_eq!(count(&unclassified) + count(&faults), 0);
}
