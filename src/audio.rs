use std::path::{Path, PathBuf};
use std::process::Command;

use crate::render::temp_path;

// Salamander Grand Piano plays a release sample and a resonance sample for each key,
// so the 64 voices of sfizz are too few with the pedal down.
const POLYPHONY: &str = "256";
// The loudest sample of an export: -1 dBFS.
const PEAK: f64 = 29_204.0;
// After the gain, the end is cut where no sample is above this level (-72 dBFS), plus a margin.
const SILENT: i16 = 8;
const MARGIN_SECONDS: f64 = 0.05;

fn program(env: &str, name: &str) -> PathBuf {
    std::env::var_os(env).map_or_else(|| PathBuf::from(name), PathBuf::from)
}

fn run(cmd: &mut Command, name: &str, hint: &str) -> Result<(), String> {
    let out = cmd
        .output()
        .map_err(|e| format!("cannot start {name}: {e}. {hint}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let msg = String::from_utf8_lossy(&out.stdout);
    Err(format!(
        "{name} failed ({}): {}",
        out.status,
        if err.trim().is_empty() {
            msg.trim()
        } else {
            err.trim()
        }
    ))
}

// Plays the MIDI file with an SFZ instrument through sfizz, then converts the WAV with ffmpeg
// if the output is not a WAV file.
pub fn render(midi: &[u8], sound: &Path, out: &Path) -> Result<(), String> {
    let is_sfz = sound
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("sfz"));
    if !is_sfz {
        return Err(format!(
            "{} is not an SFZ instrument (.sfz); sfizz plays only SFZ",
            sound.display()
        ));
    }
    if !sound.is_file() {
        return Err(format!("there is no file {}", sound.display()));
    }
    let is_wav = out
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
    let mid = temp_path("mid");
    let wav = if is_wav {
        out.to_path_buf()
    } else {
        temp_path("wav")
    };
    let result = std::fs::write(&mid, midi)
        .map_err(|e| format!("cannot write {}: {e}", mid.display()))
        .and_then(|()| {
            run(
                Command::new(program("PNO_SFIZZ", "sfizz_render"))
                    .arg("--sfz")
                    .arg(sound)
                    .arg("--midi")
                    .arg(&mid)
                    .arg("--wav")
                    .arg(&wav)
                    .args(["--polyphony", POLYPHONY, "--use-eot"]),
                "sfizz_render",
                "Install sfizz (https://sfz.tools/sfizz) and put sfizz_render on the PATH, or set PNO_SFIZZ to its path",
            )
        })
        .and_then(|()| normalize(&wav))
        .and_then(|()| {
            if is_wav {
                return Ok(());
            }
            run(
                Command::new(program("PNO_FFMPEG", "ffmpeg"))
                    .args(["-y", "-loglevel", "error", "-i"])
                    .arg(&wav)
                    .arg(out),
                "ffmpeg",
                "Install ffmpeg and put it on the PATH, or set PNO_FFMPEG to its path; or export a .wav file",
            )
        });
    let _ = std::fs::remove_file(&mid);
    if !is_wav {
        let _ = std::fs::remove_file(&wav);
    }
    result
}

// sfizz writes the level of the samples; Salamander is recorded with much headroom.
// The gain brings the loudest sample to PEAK and keeps the dynamics. The silence at the end
// is cut.
fn normalize(wav: &Path) -> Result<(), String> {
    let cannot = |e: std::io::Error| format!("cannot change {}: {e}", wav.display());
    let mut bytes = std::fs::read(wav).map_err(cannot)?;
    let Some(pcm) = pcm16_data(&bytes) else {
        return Err(format!("{} is not 16-bit PCM WAV", wav.display()));
    };
    let end = (pcm.start + pcm.len).min(bytes.len());
    let data = &mut bytes[pcm.start..end];
    let sample = |c: &[u8]| f64::from(i16::from_le_bytes([c[0], c[1]]));
    let peak = data
        .chunks_exact(2)
        .map(|c| sample(c).abs())
        .fold(0.0, f64::max);
    if peak == 0.0 {
        return Ok(());
    }
    let gain = PEAK / peak;
    for c in data.chunks_exact_mut(2) {
        let v = (sample(c) * gain).round().clamp(-32_768.0, 32_767.0) as i16;
        c.copy_from_slice(&v.to_le_bytes());
    }
    let data_is_last = end == bytes.len();
    if data_is_last && pcm.frame > 0 {
        let sound = bytes[pcm.start..end]
            .chunks_exact(2)
            .rposition(|c| i16::from_le_bytes([c[0], c[1]]).unsigned_abs() > SILENT.unsigned_abs());
        let margin = (f64::from(pcm.rate) * MARGIN_SECONDS) as usize * pcm.frame;
        let keep = sound.map_or(0, |i| (i * 2 / pcm.frame + 1) * pcm.frame) + margin;
        let keep = keep.min(end - pcm.start);
        bytes.truncate(pcm.start + keep);
        let riff = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&riff.to_le_bytes());
        bytes[pcm.start - 4..pcm.start].copy_from_slice(&(keep as u32).to_le_bytes());
    }
    std::fs::write(wav, bytes).map_err(cannot)
}

struct Pcm {
    start: usize,
    len: usize,
    // Bytes of one sample of all channels.
    frame: usize,
    rate: u32,
}

// The data chunk, if the format chunk is 16-bit PCM.
fn pcm16_data(b: &[u8]) -> Option<Pcm> {
    if b.get(..4)? != b"RIFF" || b.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut i = 12;
    let mut format: Option<(u32, usize)> = None;
    let u16_at = |at: usize| -> Option<u16> {
        Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
    };
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32::from_le_bytes(b[i + 4..i + 8].try_into().ok()?) as usize;
        let body = i + 8;
        if id == b"fmt " {
            let pcm16 = u16_at(body)? == 1 && u16_at(body + 14)? == 16;
            let rate = u32::from_le_bytes(b.get(body + 4..body + 8)?.try_into().ok()?);
            format = pcm16.then_some((rate, usize::from(u16_at(body + 12)?)));
        }
        if id == b"data" {
            let (rate, frame) = format?;
            return Some(Pcm {
                start: body,
                len,
                frame,
                rate,
            });
        }
        i = body.checked_add(len)?.checked_add(len % 2)?;
    }
    None
}
