use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

const NAMES: [&str; 4] = ["MuseScore4", "mscore4", "mscore", "musescore"];

fn standard_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(dir) = std::env::var_os(var) {
            out.push(
                Path::new(&dir)
                    .join("MuseScore 4")
                    .join("bin")
                    .join("MuseScore4.exe"),
            );
        }
    }
    out.push(PathBuf::from(
        "/Applications/MuseScore 4.app/Contents/MacOS/mscore",
    ));
    out.push(PathBuf::from("/usr/bin/mscore"));
    out.push(PathBuf::from("/usr/bin/musescore"));
    out
}

pub fn find_musescore() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("PNO_MUSESCORE") {
        return Some(PathBuf::from(p));
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        for name in NAMES {
            for candidate in [dir.join(name), dir.join(format!("{name}.exe"))] {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    standard_paths().into_iter().find(|p| p.is_file())
}

fn musescore() -> Result<PathBuf, String> {
    find_musescore().ok_or_else(|| {
        "MuseScore 4 was not found. Install it from https://musescore.org, or set PNO_MUSESCORE to the path of its program".into()
    })
}

// A compact layout for images that an agent reads: a number on each bar, no vertical spread.
pub const IMAGE_STYLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<museScore version="4.40">
  <Style>
    <enableVerticalSpread>0</enableVerticalSpread>
    <minSystemDistance>4</minSystemDistance>
    <maxSystemDistance>8</maxSystemDistance>
    <showMeasureNumber>1</showMeasureNumber>
    <showMeasureNumberOne>1</showMeasureNumberOne>
    <measureNumberInterval>1</measureNumberInterval>
    <measureNumberSystem>0</measureNumberSystem>
  </Style>
</museScore>
"#;

pub fn temp_path(ext: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pno-{}.{ext}", std::process::id()))
}

// MuseScore writes `name-1.png`, `name-2.png` for a PNG or SVG of more than one page.
pub fn pages(output: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    let stem = output.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
    let ext = output.extension().and_then(|s| s.to_str()).unwrap_or("");
    let dir = output.parent().unwrap_or(Path::new("."));
    (1..)
        .map(move |n| dir.join(format!("{stem}-{n}.{ext}")))
        .take_while(|p| p.is_file())
}

// MuseScore reports success when it cannot replace a file that another program has open, so
// the old files go first: a file that is still there after the run is new.
fn remove_old(output: &Path) -> Result<(), String> {
    for old in std::iter::once(output.to_path_buf()).chain(pages(output)) {
        match std::fs::remove_file(&old) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(format!(
                    "cannot replace {}: {e}. Close the program that has it open",
                    old.display()
                ));
            }
        }
    }
    Ok(())
}

fn run(args: &[&str], more: &[&OsStr]) -> Result<(), String> {
    let ms = musescore()?;
    let result = Command::new(&ms)
        .args(args)
        .args(more)
        .output()
        .map_err(|e| format!("cannot start {}: {e}", ms.display()))?;
    if !result.status.success() {
        let err = String::from_utf8_lossy(&result.stderr);
        return Err(format!(
            "MuseScore failed ({}): {}",
            result.status,
            err.trim()
        ));
    }
    Ok(())
}

// One launch for several outputs of one input, as MuseScore starts slowly.
pub fn convert_all(input: &Path, outputs: &[PathBuf], args: &[&str]) -> Result<(), String> {
    let quoted = |p: &Path| {
        let s = p.display().to_string();
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    };
    let outs: Vec<String> = outputs.iter().map(|p| quoted(p)).collect();
    let job = format!(
        "[{{\"in\": {}, \"out\": [{}]}}]",
        quoted(input),
        outs.join(", ")
    );
    let job_path = input.with_extension("json");
    std::fs::write(&job_path, job)
        .map_err(|e| format!("cannot write {}: {e}", job_path.display()))?;
    run(args, &[OsStr::new("-j"), job_path.as_os_str()])?;
    match outputs
        .iter()
        .find(|p| !p.is_file() && pages(p).next().is_none())
    {
        Some(missing) => Err(format!("MuseScore wrote no file {}", missing.display())),
        None => Ok(()),
    }
}

pub fn convert(input: &Path, output: &Path, args: &[&str]) -> Result<Vec<PathBuf>, String> {
    remove_old(output)?;
    run(
        args,
        &[OsStr::new("-o"), output.as_os_str(), input.as_os_str()],
    )?;
    if output.is_file() {
        return Ok(vec![output.to_path_buf()]);
    }
    let pages: Vec<PathBuf> = pages(output).collect();
    match pages.as_slice() {
        [] => Err(format!("MuseScore wrote no file {}", output.display())),
        [one] => {
            std::fs::rename(one, output)
                .map_err(|e| format!("cannot rename {}: {e}", one.display()))?;
            Ok(vec![output.to_path_buf()])
        }
        _ => Ok(pages),
    }
}
