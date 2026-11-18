use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

const A2P: &str = include_str!("../skill/audio-to-pno/scripts/a2p.py");
const SETUP_ENV: &str = include_str!("../skill/audio-to-pno/scripts/setup_env.py");
const REQUIREMENTS: &str = include_str!("../skill/audio-to-pno/requirements.txt");

/// Runs a2p.py in its venv and returns its exit code. The first run, and the first run after a change of
/// setup_env.py or requirements.txt, runs setup_env.py before it.
pub fn run(args: &[OsString]) -> Result<i32, String> {
    // the folder of Python's Path.home(), where setup_env.py puts the venv
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .ok_or("cannot find the home folder")?;
    let cache = PathBuf::from(home).join(".cache").join("audio-to-pno");
    let dir = cache.join("pno");
    let script = dir.join("scripts").join("a2p.py");
    let setup = dir.join("scripts").join("setup_env.py");
    write_if_changed(&script, A2P)?;
    write_if_changed(&setup, SETUP_ENV)?;
    write_if_changed(&dir.join("requirements.txt"), REQUIREMENTS)?;

    let python = cache.join("venv").join(if cfg!(windows) {
        "Scripts/python.exe"
    } else {
        "bin/python"
    });
    let stamp = dir.join("setup.txt");
    let wanted = format!("{SETUP_ENV}{REQUIREMENTS}");
    let set_up = std::fs::read_to_string(&stamp).is_ok_and(|done| done == wanted);
    if !python.exists() || !set_up {
        run_setup(&setup, &cache)?;
        std::fs::write(&stamp, wanted)
            .map_err(|e| format!("cannot write {}: {e}", stamp.display()))?;
    }

    let status = Command::new(&python)
        .arg(&script)
        .args(args)
        .status()
        .map_err(|e| format!("cannot start {}: {e}", python.display()))?;
    Ok(status.code().unwrap_or(1))
}

fn run_setup(setup: &Path, cache: &Path) -> Result<(), String> {
    let python = std::env::var_os("PNO_PYTHON").unwrap_or_else(|| "python".into());
    eprintln!(
        "pno a2p: setting up the Python environment and the models in {} (about 1.3 GB, 3.8 GB with CUDA)",
        cache.display()
    );
    // the messages of pip: keep them out of the output of a2p.py
    let status = Command::new(&python)
        .arg(setup)
        .stdout(std::io::stderr())
        .status()
        .map_err(|e| {
            format!(
                "cannot start {}: {e}. Install Python 3.10, or set PNO_PYTHON to its path",
                python.to_string_lossy()
            )
        })?;
    if status.success() {
        return Ok(());
    }
    Err(format!(
        "{} failed ({status}); `pno a2p` runs it again next time",
        setup.display()
    ))
}

fn write_if_changed(path: &Path, text: &str) -> Result<(), String> {
    if std::fs::read_to_string(path).is_ok_and(|old| old == text) {
        return Ok(());
    }
    let parent = path.parent().unwrap_or(path);
    std::fs::create_dir_all(parent)
        .and_then(|()| std::fs::write(path, text))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}
