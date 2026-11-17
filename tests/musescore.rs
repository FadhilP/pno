mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use common::{fixtures, path, pno, root, temp_dir};

fn musescore() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("PNO_MUSESCORE") {
        return Some(PathBuf::from(p));
    }
    let dir = std::env::var_os("ProgramFiles")?;
    let p = Path::new(&dir)
        .join("MuseScore 4")
        .join("bin")
        .join("MuseScore4.exe");
    p.is_file().then_some(p)
}

fn log_folder() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let p = Path::new(&local)
        .join("MuseScore")
        .join("MuseScore4")
        .join("logs");
    p.is_dir().then_some(p)
}

// MuseScore reports MusicXML import problems only as debug lines in its log.
fn import_problems(logs: &Path, since: SystemTime) -> Vec<String> {
    let mut out = Vec::new();
    let mut read = 0;
    for entry in std::fs::read_dir(logs)
        .expect("log folder")
        .map(|e| e.expect("log entry"))
    {
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .expect("log time");
        if modified < since {
            continue;
        }
        read += 1;
        let text = String::from_utf8_lossy(&std::fs::read(entry.path()).expect("log")).into_owned();
        out.extend(
            text.lines()
                .filter(|l| l.contains("to_xml_log"))
                .map(str::to_string),
        );
    }
    assert!(read > 0, "MuseScore wrote no log during the test");
    out
}

fn png_size(file: &Path) -> (u32, u32) {
    let bytes = std::fs::read(file).expect("PNG");
    assert!(
        bytes.starts_with(b"\x89PNG"),
        "{} is not a PNG",
        file.display()
    );
    let word = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
    (word(16), word(20))
}

#[test]
fn musescore_reads_the_exports() {
    let (Some(ms), Some(logs)) = (musescore(), log_folder()) else {
        eprintln!("MuseScore 4 or its log folder not found; skipped");
        return;
    };
    let dir = temp_dir("musescore");
    let since = SystemTime::now() - std::time::Duration::from_secs(1);
    for f in fixtures() {
        let name = f.file_stem().and_then(|s| s.to_str()).expect("file name");
        // MuseScore 4 pairs ties only inside one voice, so it reports a tie into another voice
        // (also in the source of the test score, which MuseScore 3 wrote).
        if name == "tie_voice_change" || name == "tristesse" {
            continue;
        }
        let xml = dir.join(format!("{name}.musicxml"));
        let run = pno(&["export", path(&f), path(&xml)]);
        assert_eq!(run.code, 0, "{name}: {}", run.stderr);
        let status = Command::new(&ms)
            .arg("-d")
            .arg("-o")
            .arg(dir.join(format!("{name}.mscx")))
            .arg(&xml)
            .output()
            .expect("MuseScore runs");
        assert!(status.status.success(), "{name}: MuseScore failed");
    }
    let problems = import_problems(&logs, since);
    assert!(
        problems.is_empty(),
        "import problems:\n{}",
        problems.join("\n")
    );

    let score = root().join("tests/fixtures/tristesse.pno");
    let png = dir.join("bars.png");
    let run = pno(&["render", path(&score), "9-10", "-o", path(&png)]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let (width, height) = png_size(&png);
    assert!(
        height * 3 < width,
        "a trimmed system is wide and short: {width}x{height}"
    );
}

#[test]
fn velocity_lines_survive_a_saved_musescore_file() {
    let Some(ms) = musescore() else {
        eprintln!("MuseScore 4 not found; skipped");
        return;
    };
    let dir = temp_dir("musescore-vel");
    let text = "@ format=1\n@ vel.p=52\n\n1 @      vel.rh=+10\n1 rh.dyn | p | _ | _ | _ |\n1 rh     | C5 | D5:v-10 | E5 | F5 |\n1 lh     | C3 | - | - | - |\n";
    let f = common::write(&dir, "v.pno", text);
    let xml = dir.join("v.musicxml");
    let saved = dir.join("v.mscz");
    let back = dir.join("back.pno");
    let run = pno(&["export", path(&f), path(&xml)]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let status = Command::new(&ms)
        .arg("-o")
        .arg(&saved)
        .arg(&xml)
        .output()
        .expect("MuseScore runs");
    assert!(status.status.success(), "MuseScore failed");
    let run = pno(&["import", path(&saved), "-o", path(&back)]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(std::fs::read_to_string(&back).expect("import"), text);
}
