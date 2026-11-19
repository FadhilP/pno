use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::model::Staff;
use crate::perform::{Performance, Played, TICKS_PER_QUARTER};

const WIDTH: usize = 1920;
const HEIGHT: usize = 1080;
const FPS: f64 = 60.0;
const KEYBOARD_HEIGHT: usize = 200;
const KEYBOARD_TOP: usize = HEIGHT - KEYBOARD_HEIGHT;
// A note falls from the top of the frame to the keyboard in this time.
const SECONDS_SHOWN: f64 = 3.0;
// The video starts this long before the music, so that the first notes fall.
const LEAD_IN: f64 = 2.0;
// A note ends this long before the next note on its key, so that the eye sees two notes.
const REPEAT_GAP: f64 = 0.04;
const LOWEST_KEY: u8 = 21;
const HIGHEST_KEY: u8 = 108;
const WHITE_KEYS: f64 = 52.0;

type Rgb = [u8; 3];
const BACKGROUND: Rgb = [24, 24, 28];
const PEDAL_DOWN: Rgb = [43, 36, 63];
// The press of the pedal, and the strip above the keyboard while the pedal is down.
const PEDAL_MARK: Rgb = [100, 78, 144];
const PEDAL_EDGE: usize = 3;
const PEDAL_STRIP: usize = 3;
const OCTAVE_LINE: Rgb = [58, 58, 68];
const BAR_LINE: Rgb = [56, 56, 64];
const WHITE: Rgb = [236, 236, 232];
const BLACK: Rgb = [28, 28, 30];
const GAP: Rgb = [120, 120, 120];

// The colours of the notes of each staff.
pub struct Hands {
    pub upper: Rgb,
    pub lower: Rgb,
}

impl Default for Hands {
    fn default() -> Self {
        Hands {
            upper: [92, 196, 120],
            lower: [84, 148, 228],
        }
    }
}

impl Hands {
    // `RRGGBB,RRGGBB` for the upper and the lower staff; a `#` before each is permitted.
    pub fn parse(s: &str) -> Result<Hands, String> {
        let bad = || format!("--colors needs two hex colours such as 5CC478,5494E4, not `{s}`");
        let rgb = |c: &str| -> Option<Rgb> {
            let c = c.trim().trim_start_matches('#');
            if c.len() != 6 {
                return None;
            }
            let byte = |i: usize| u8::from_str_radix(c.get(i..i + 2)?, 16).ok();
            Some([byte(0)?, byte(2)?, byte(4)?])
        };
        let (upper, lower) = s.split_once(',').ok_or_else(bad)?;
        Ok(Hands {
            upper: rgb(upper).ok_or_else(bad)?,
            lower: rgb(lower).ok_or_else(bad)?,
        })
    }
}

struct Key {
    x: usize,
    width: usize,
    black: bool,
}

fn is_black(key: u8) -> bool {
    matches!(key % 12, 1 | 3 | 6 | 8 | 10)
}

fn keys() -> Vec<Key> {
    let white = WIDTH as f64 / WHITE_KEYS;
    let mut whites_before = 0.0;
    (LOWEST_KEY..=HIGHEST_KEY)
        .map(|k| {
            if is_black(k) {
                let width = white * 0.6;
                return Key {
                    x: (whites_before * white - width / 2.0).round() as usize,
                    width: width.round() as usize,
                    black: true,
                };
            }
            let x = (whites_before * white).round() as usize;
            whites_before += 1.0;
            Key {
                x,
                width: (whites_before * white).round() as usize - x,
                black: false,
            }
        })
        .collect()
}

// The time in seconds of each tempo change, to turn ticks into seconds.
fn seconds(tempo: &[(f64, f64)], tick: f64) -> f64 {
    let mut s = 0.0;
    let mut from = 0.0;
    let mut qpm = 120.0;
    for &(t, q) in tempo.iter().take_while(|p| p.0 <= tick) {
        s += (t - from) / TICKS_PER_QUARTER * 60.0 / qpm;
        from = t;
        qpm = q;
    }
    s + (tick - from) / TICKS_PER_QUARTER * 60.0 / qpm
}

struct Frame(Vec<u8>);

impl Frame {
    fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, c: Rgb) {
        let (x0, x1) = (x.min(WIDTH), (x + w).min(WIDTH));
        let line = c.repeat(x1 - x0);
        for row in y..(y + h).min(HEIGHT) {
            self.0[(row * WIDTH + x0) * 3..(row * WIDTH + x1) * 3].copy_from_slice(&line);
        }
    }
}

fn shade(c: Rgb, f: f64) -> Rgb {
    c.map(|v| (f64::from(v) * f).round().min(255.0) as u8)
}

struct Timed {
    on: f64,
    off: f64,
    key: usize,
    color: Rgb,
}

struct Scene {
    keys: Vec<Key>,
    notes: Vec<Timed>,
    // The times when the pedal is down.
    pedal: Vec<(f64, f64)>,
    bars: Vec<f64>,
}

fn draw(frame: &mut Frame, scene: &Scene, now: f64) {
    let keys = &scene.keys;
    let top = KEYBOARD_TOP as f64;
    // The keyboard line is `now`.
    let row = |t: f64| {
        (top - (t - now) * top / SECONDS_SHOWN)
            .round()
            .clamp(0.0, top) as usize
    };
    let shown = |from: f64, to: f64| from < now + SECONDS_SHOWN && to > now;
    frame.fill(0, 0, WIDTH, HEIGHT, BACKGROUND);
    for &(from, to) in scene.pedal.iter().filter(|p| shown(p.0, p.1)) {
        frame.fill(0, row(to), WIDTH, row(from) - row(to), PEDAL_DOWN);
        if from > now {
            let edge = row(from).saturating_sub(PEDAL_EDGE);
            frame.fill(0, edge, WIDTH, PEDAL_EDGE, PEDAL_MARK);
        }
    }
    let each_c = keys.iter().zip(LOWEST_KEY..).filter(|(_, n)| n % 12 == 0);
    for (k, _) in each_c {
        frame.fill(k.x, 0, 1, KEYBOARD_TOP, OCTAVE_LINE);
    }
    for &t in scene.bars.iter().filter(|&&t| shown(t, t)) {
        frame.fill(0, row(t), WIDTH, 1, BAR_LINE);
    }
    let mut down: Vec<Option<Rgb>> = vec![None; keys.len()];
    for black in [false, true] {
        let visible = scene.notes.iter().filter(|n| shown(n.on, n.off));
        for n in visible.filter(|n| keys[n.key].black == black) {
            let k = &keys[n.key];
            let (y, height) = (row(n.off), row(n.on) - row(n.off));
            frame.fill(k.x, y, k.width, height, shade(n.color, 0.55));
            frame.fill(
                k.x + 1,
                y + 1,
                k.width.saturating_sub(2),
                height.saturating_sub(2),
                n.color,
            );
            if n.on <= now {
                down[n.key] = Some(n.color);
            }
        }
    }
    let pedal_down = scene
        .pedal
        .iter()
        .any(|&(from, to)| from <= now && now < to);
    if pedal_down {
        frame.fill(
            0,
            KEYBOARD_TOP - PEDAL_STRIP,
            WIDTH,
            PEDAL_STRIP,
            PEDAL_MARK,
        );
    }
    frame.fill(0, KEYBOARD_TOP, WIDTH, 3, [200, 60, 60]);
    for (k, d) in keys.iter().zip(&down).filter(|(k, _)| !k.black) {
        frame.fill(
            k.x,
            KEYBOARD_TOP + 3,
            k.width,
            KEYBOARD_HEIGHT - 3,
            d.unwrap_or(WHITE),
        );
        frame.fill(k.x, KEYBOARD_TOP + 3, 1, KEYBOARD_HEIGHT - 3, GAP);
    }
    let black_height = (KEYBOARD_HEIGHT as f64 * 0.62).floor() as usize;
    for (k, d) in keys.iter().zip(&down).filter(|(k, _)| k.black) {
        let c = d.map_or(BLACK, |c| shade(c, 0.75));
        frame.fill(k.x, KEYBOARD_TOP + 3, k.width, black_height, c);
    }
}

fn timed(p: &Performance, hands: &Hands, played: &Played) -> Option<Timed> {
    let in_range = (LOWEST_KEY..=HIGHEST_KEY).contains(&played.key);
    if !in_range {
        return None;
    }
    let loud = 0.6 + 0.4 * f64::from(played.velocity) / 127.0;
    Some(Timed {
        on: seconds(&p.tempo, played.on),
        off: seconds(&p.tempo, played.off),
        key: usize::from(played.key - LOWEST_KEY),
        color: shade(
            match played.staff {
                Staff::Upper => hands.upper,
                Staff::Lower => hands.lower,
            },
            loud,
        ),
    })
}

fn pedal_spans(p: &Performance, end: f64) -> Vec<(f64, f64)> {
    let mut spans = Vec::new();
    let mut pressed: Option<f64> = None;
    for &(tick, value) in &p.pedal {
        let t = seconds(&p.tempo, tick);
        match (pressed, value >= 64) {
            (None, true) => pressed = Some(t),
            (Some(from), false) => {
                spans.push((from, t));
                pressed = None;
            }
            (None, false) | (Some(_), true) => {}
        }
    }
    spans.extend(pressed.map(|from| (from, end)));
    spans
}

fn ffmpeg() -> Command {
    let program = std::env::var_os("PNO_FFMPEG").unwrap_or_else(|| "ffmpeg".into());
    let mut cmd = Command::new(program);
    cmd.args(["-y", "-loglevel", "error"]);
    cmd
}

const NO_FFMPEG: &str = "Install ffmpeg and put it on the PATH, or set PNO_FFMPEG to its path";

// Falling notes over a keyboard, encoded by ffmpeg with no audio.
pub fn render(p: &Performance, hands: &Hands, out: &Path) -> Result<(), String> {
    let end = seconds(&p.tempo, p.end);
    let mut notes: Vec<Timed> = p.notes.iter().filter_map(|n| timed(p, hands, n)).collect();
    let mut next_on = [f64::INFINITY; 88];
    for n in notes.iter_mut().rev() {
        let gap = REPEAT_GAP.min((next_on[n.key] - n.on) / 2.0);
        n.off = n.off.min(next_on[n.key] - gap);
        next_on[n.key] = n.on;
    }
    let scene = Scene {
        keys: keys(),
        notes,
        pedal: pedal_spans(p, end),
        bars: p.bars.iter().map(|&t| seconds(&p.tempo, t)).collect(),
    };
    let size = format!("{WIDTH}x{HEIGHT}");
    let mut encoder = ffmpeg()
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24"])
        .args(["-s", &size, "-r", &FPS.to_string(), "-i", "-"])
        .args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "20"])
        .args(["-pix_fmt", "yuv420p", "-an"])
        .arg(out)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start ffmpeg: {e}. {NO_FFMPEG}"))?;
    let mut frame = Frame(vec![0; WIDTH * HEIGHT * 3]);
    let written = {
        let stdin = encoder.stdin.as_mut().ok_or("cannot write to ffmpeg")?;
        let frames = ((LEAD_IN + end) * FPS).ceil() as u32;
        (0..frames).try_for_each(|i| {
            draw(&mut frame, &scene, f64::from(i) / FPS - LEAD_IN);
            stdin.write_all(&frame.0)
        })
    };
    drop(encoder.stdin.take());
    let status = encoder.wait().map_err(|e| format!("ffmpeg failed: {e}"))?;
    match (status.success(), written) {
        (false, _) => Err(format!("ffmpeg failed ({status})")),
        (true, Err(e)) => Err(format!("cannot write the frames to ffmpeg: {e}")),
        (true, Ok(())) => Ok(()),
    }
}

// The video of `render` with the audio of `wav`, which starts after the lead-in.
pub fn mux(video: &Path, wav: &Path, out: &Path) -> Result<(), String> {
    let delay = format!("adelay={}:all=1", LEAD_IN * 1000.0);
    let status = ffmpeg()
        .arg("-i")
        .arg(video)
        .arg("-i")
        .arg(wav)
        .args(["-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-af", &delay])
        .args(["-shortest", "-movflags", "+faststart"])
        .arg(out)
        .status()
        .map_err(|e| format!("cannot start ffmpeg: {e}. {NO_FFMPEG}"))?;
    if status.success() {
        return Ok(());
    }
    Err(format!("ffmpeg failed ({status})"))
}
