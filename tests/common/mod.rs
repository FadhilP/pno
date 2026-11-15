#![allow(dead_code)]

pub mod compare;

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn pno(args: &[&str]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_pno"))
        .args(args)
        .output()
        .expect("pno runs");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

pub fn temp_dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pno-test-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir is created");
    dir
}

pub fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, text).expect("temp file is written");
    p
}

pub fn path(p: &Path) -> &str {
    p.to_str().expect("path is UTF-8")
}

pub fn fixtures() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures");
    let mut out = vec![root.join("tristesse.pno")];
    let mut spec: Vec<PathBuf> = std::fs::read_dir(root.join("spec"))
        .expect("spec fixtures exist")
        .map(|e| e.expect("dir entry").path())
        .collect();
    spec.sort();
    out.extend(spec);
    out
}

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
