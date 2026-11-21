use std::path::Path;

use crate::perform::BarOnsets;
use crate::render;
use crate::video::{NO_FFMPEG, Rgb, ffmpeg};

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Place {
    Below,
    Top,
}

impl Place {
    // The band at the top lies over the falling notes, so it lets them show through.
    fn opacity(self) -> f64 {
        match self {
            Place::Below => 1.0,
            Place::Top => 0.4,
        }
    }
}

pub struct Source {
    pub place: Place,
    // MusicXML with no title frame and no pedal marks.
    pub xml: String,
    pub bars: Vec<BarOnsets>,
}

// A narrow page, so that a system holds few bars and fills the band at a large size.
const STYLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<museScore version="4.40">
  <Style>
    <pageWidth>9.4488</pageWidth>
    <pageHeight>11.6929</pageHeight>
    <pagePrintableWidth>8.2677</pagePrintableWidth>
    <pageEvenLeftMargin>0.590551</pageEvenLeftMargin>
    <pageOddLeftMargin>0.590551</pageOddLeftMargin>
    <pageEvenTopMargin>0.590551</pageEvenTopMargin>
    <pageEvenBottomMargin>0.590551</pageEvenBottomMargin>
    <pageOddTopMargin>0.590551</pageOddTopMargin>
    <pageOddBottomMargin>0.590551</pageOddBottomMargin>
    <pageTwosided>0</pageTwosided>
    <enableVerticalSpread>0</enableVerticalSpread>
    <createMultiMeasureRests>0</createMultiMeasureRests>
    <showHeader>0</showHeader>
    <showFooter>0</showFooter>
  </Style>
</museScore>
"#;
const DPI: f64 = 240.0;
// MuseScore writes positions in twelfths of a pixel of the -r resolution.
const POSITION_UNIT: f64 = 12.0;
// The middle of a note head from the left of its segment: 0.6 of MuseScore's 1.764 mm staff space.
const HEAD_MIDDLE: f64 = 0.6 * 1.764 / 25.4 * DPI;
// The brace and the marks at the ends of a system are outside its measures.
const SIDE: f64 = 0.2 * DPI;
const INK_MIN: u8 = 16;
const MARGIN: f64 = 40.0;

// A written measure: its system, and x positions in the band.
pub struct Measure {
    pub system: usize,
    pub left: f64,
    pub right: f64,
    // The note and rest columns, from left to right.
    pub columns: Vec<f64>,
}

// A played bar in seconds, with the time of each written onset.
pub struct TimedBar {
    pub bar: usize,
    pub start: f64,
    pub end: f64,
    pub onsets: Vec<f64>,
}

// The cursor is at x at this time, and moves in a line to the next anchor of the same played bar.
#[derive(Debug, PartialEq)]
pub struct Anchor {
    pub time: f64,
    pub x: f64,
    pub played: usize,
}

// Each played bar has its own anchors, so that a bar whose columns MuseScore lays out
// differently from its onsets (a tremolo, notes across the staves) moves the cursor in a line
// across the bar and the other bars stay exact.
pub fn anchors(measures: &[Measure], played: &[TimedBar]) -> Vec<Anchor> {
    let mut out = Vec::new();
    for (k, bar) in played.iter().enumerate() {
        let m = &measures[bar.bar];
        let one_each = !bar.onsets.is_empty() && bar.onsets.len() == m.columns.len();
        let columns: Vec<(f64, f64)> = if one_each {
            bar.onsets
                .iter()
                .copied()
                .zip(m.columns.iter().copied())
                .collect()
        } else {
            vec![(bar.start, m.columns.first().copied().unwrap_or(m.left))]
        };
        let points = columns.into_iter().chain([(bar.end, m.right)]);
        out.extend(points.map(|(time, x)| Anchor { time, x, played: k }));
    }
    out
}

// The played bar and the x of the cursor at a time; before the first anchor, the first.
pub fn cursor(anchors: &[Anchor], now: f64) -> Option<(usize, f64)> {
    let next = anchors.partition_point(|a| a.time <= now);
    let Some(at) = next.checked_sub(1).map(|i| &anchors[i]) else {
        return anchors.first().map(|a| (a.played, a.x));
    };
    match anchors.get(next) {
        Some(to) if to.played == at.played && to.time > at.time => {
            let part = (now - at.time) / (to.time - at.time);
            Some((at.played, at.x + (to.x - at.x) * part))
        }
        _ => Some((at.played, at.x)),
    }
}

// The ink of a system, white on clear: 0 is no ink.
pub struct System {
    ink: Vec<u8>,
    width: usize,
    height: usize,
    staff_top: f64,
    staff_bottom: f64,
}

impl System {
    fn scaled(&self, scale: f64) -> System {
        let (ink, width, height) = shrink(&self.ink, self.width, self.height, scale);
        System {
            ink,
            width,
            height,
            staff_top: self.staff_top * scale,
            staff_bottom: self.staff_bottom * scale,
        }
    }
}

// The cursor is on one system from this time.
struct Visit {
    start: f64,
    system: usize,
}

pub struct Sheet {
    left: usize,
    systems: Vec<System>,
    measures: Vec<Measure>,
    // The written bar of each played bar.
    bars: Vec<usize>,
    anchors: Vec<Anchor>,
    visits: Vec<Visit>,
    slot: usize,
    opacity: f64,
}

#[derive(Clone, Copy)]
struct Rect {
    page: usize,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

fn positions(path: &Path) -> Result<Vec<Rect>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let doc = roxmltree::Document::parse(&text)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut out: Vec<(f64, Rect)> = doc
        .descendants()
        .filter(|n| n.has_tag_name("element"))
        .map(|n| {
            let num = |k: &str| {
                n.attribute(k)
                    .and_then(|v| v.parse::<f64>().ok())
                    .ok_or_else(|| format!("{}: an element has no {k}", path.display()))
            };
            let rect = Rect {
                page: num("page")? as usize,
                x: num("x")? / POSITION_UNIT,
                y: num("y")? / POSITION_UNIT,
                w: num("sx")? / POSITION_UNIT,
                h: num("sy")? / POSITION_UNIT,
            };
            Ok((num("id")?, rect))
        })
        .collect::<Result<_, String>>()?;
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(out.into_iter().map(|(_, r)| r).collect())
}

fn same_row(a: &Rect, b: &Rect) -> bool {
    a.page == b.page && (a.y - b.y).abs() < 1.0
}

// The ink of a page: dark and opaque is 255; the paper and the clear background are 0.
fn page_ink(page: &Path) -> Result<(Vec<u8>, usize), String> {
    let bytes = std::fs::read(page).map_err(|e| format!("cannot read {}: {e}", page.display()))?;
    let size = |at: usize| {
        let b: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
        Some(u32::from_be_bytes(b) as usize)
    };
    let (width, height) = size(16)
        .zip(size(20))
        .ok_or_else(|| format!("{} is not a PNG", page.display()))?;
    let out = ffmpeg()
        .arg("-i")
        .arg(page)
        .args(["-f", "rawvideo", "-pix_fmt", "ya8", "-"])
        .output()
        .map_err(|e| format!("cannot start ffmpeg: {e}. {NO_FFMPEG}"))?;
    if !out.status.success() || out.stdout.len() != width * height * 2 {
        return Err(format!("ffmpeg cannot read {}", page.display()));
    }
    let ink = out
        .stdout
        .chunks_exact(2)
        .map(|p| (u16::from(p[1]) * u16::from(255 - p[0]) / 255) as u8)
        .collect();
    Ok((ink, width))
}

// Each output pixel is the mean of the input pixels under it.
fn shrink(ink: &[u8], width: usize, height: usize, scale: f64) -> (Vec<u8>, usize, usize) {
    let size = |n: usize| ((n as f64 * scale).round() as usize).max(1);
    let (w, h) = (size(width), size(height));
    let span = |d: usize, n: usize| {
        let from = ((d as f64 / scale) as usize).min(n - 1);
        let to = (((d + 1) as f64 / scale) as usize).clamp(from + 1, n);
        from..to
    };
    let mut out = vec![0; w * h];
    for y in 0..h {
        let rows = span(y, height);
        for x in 0..w {
            let cols = span(x, width);
            let count = rows.len() * cols.len();
            let sum: usize = rows
                .clone()
                .flat_map(|r| &ink[r * width + cols.start..r * width + cols.end])
                .map(|&v| usize::from(v))
                .sum();
            out[y * w + x] = (sum / count) as u8;
        }
    }
    (out, w, h)
}

// The rows of a page from the middle of the gap above a system to the middle of the gap below,
// cut to the ink, so that ledger lines, dynamics and slurs stay with their system.
fn cut_systems(
    page: &[u8],
    page_width: usize,
    columns: std::ops::Range<usize>,
    rows: &[&Rect],
    scale: f64,
) -> Vec<System> {
    let page_height = page.len() / page_width;
    let line = |r: usize| &page[r * page_width + columns.start..r * page_width + columns.end];
    let mut out = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let above = i
            .checked_sub(1)
            .map_or(0.0, |p| (rows[p].y + rows[p].h + row.y) / 2.0);
        let below = rows
            .get(i + 1)
            .map_or(page_height as f64, |next| (row.y + row.h + next.y) / 2.0);
        let mut inked = (above as usize..(below as usize).min(page_height))
            .filter(|&r| line(r).iter().any(|&v| v > INK_MIN));
        let first = inked.next().unwrap_or(page_height).min(row.y as usize);
        let last = inked
            .next_back()
            .unwrap_or(0)
            .max((row.y + row.h) as usize)
            .min(page_height - 1);
        let full = System {
            ink: (first..=last).flat_map(line).copied().collect(),
            width: columns.len(),
            height: last + 1 - first,
            staff_top: row.y - first as f64,
            staff_bottom: row.y + row.h - first as f64,
        };
        out.push(full.scaled(scale));
    }
    out
}

// The systems of the score at one scale that fits each in a frame this wide and in slot rows,
// and the cursor anchors of the played bars.
pub fn prepare(
    src: &Source,
    played: &[TimedBar],
    width: usize,
    slot: usize,
) -> Result<Sheet, String> {
    let dir = render::temp_path("sheet");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let result = prepare_in(&dir, src, played, width, slot);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn prepare_in(
    dir: &Path,
    src: &Source,
    played: &[TimedBar],
    width: usize,
    slot: usize,
) -> Result<Sheet, String> {
    let write = |name: &str, text: &str| {
        let p = dir.join(name);
        std::fs::write(&p, text).map_err(|e| format!("cannot write {}: {e}", p.display()))?;
        Ok::<_, String>(p)
    };
    let xml = write("score.musicxml", &src.xml)?;
    let style = write("style.mss", STYLE)?;
    let (mpos, spos, png) = (
        dir.join("score.mpos"),
        dir.join("score.spos"),
        dir.join("score.png"),
    );
    let style_arg = style.to_string_lossy();
    let dpi = DPI.to_string();
    render::convert_all(
        &xml,
        &[mpos.clone(), spos.clone(), png.clone()],
        &["-S", &style_arg, "-r", &dpi],
    )?;
    let boxes = positions(&mpos)?;
    if boxes.len() != src.bars.len() {
        return Err(format!(
            "MuseScore laid out {} measures for {} bars",
            boxes.len(),
            src.bars.len()
        ));
    }
    let mut system_of = Vec::with_capacity(boxes.len());
    let mut rows: Vec<Rect> = Vec::new();
    for b in &boxes {
        if rows.last().is_none_or(|r| !same_row(r, b)) {
            rows.push(*b);
        }
        system_of.push(rows.len() - 1);
    }
    let crop_left = (boxes.iter().map(|b| b.x).fold(f64::INFINITY, f64::min) - SIDE).max(0.0);
    let crop_right = boxes.iter().map(|b| b.x + b.w).fold(0.0, f64::max) + SIDE;
    let fit_width = (width as f64 - 2.0 * MARGIN) / (crop_right - crop_left);
    let pages: Vec<_> = if png.is_file() {
        vec![png.clone()]
    } else {
        render::pages(&png).collect()
    };
    let mut systems: Vec<System> = Vec::new();
    for (n, page) in pages.iter().enumerate() {
        let (ink, page_width) = page_ink(page)?;
        let columns = crop_left as usize..(crop_right as usize).min(page_width);
        let on_page: Vec<&Rect> = rows.iter().filter(|r| r.page == n).collect();
        systems.extend(cut_systems(&ink, page_width, columns, &on_page, fit_width));
    }
    if systems.len() != rows.len() {
        return Err(format!(
            "MuseScore wrote {} pages for {} systems",
            pages.len(),
            rows.len()
        ));
    }
    let tallest = systems.iter().map(|s| s.height).max().unwrap_or(1);
    let fit_height = (slot as f64 / tallest as f64).min(1.0);
    if fit_height < 1.0 {
        systems = systems.iter().map(|s| s.scaled(fit_height)).collect();
    }
    let scale = fit_width * fit_height;
    let used = systems.iter().map(|s| s.width).max().unwrap_or(0);
    let left = width.saturating_sub(used) / 2;
    let band_x = |x: f64| left as f64 + (x - crop_left) * scale;
    let segments = positions(&spos)?;
    let measures: Vec<Measure> = boxes
        .iter()
        .zip(&system_of)
        .map(|(b, &system)| {
            let inside = |s: &&Rect| same_row(s, b) && b.x <= s.x && s.x < b.x + b.w;
            let mut columns: Vec<f64> = segments
                .iter()
                .filter(inside)
                .map(|s| band_x(s.x + HEAD_MIDDLE))
                .collect();
            columns.sort_by(f64::total_cmp);
            Measure {
                system,
                left: band_x(b.x),
                right: band_x(b.x + b.w),
                columns,
            }
        })
        .collect();
    let anchors = anchors(&measures, played);
    let mut visits: Vec<Visit> = Vec::new();
    for a in &anchors {
        let system = measures[played[a.played].bar].system;
        if visits.last().is_none_or(|v| v.system != system) {
            visits.push(Visit {
                start: a.time,
                system,
            });
        }
    }
    Ok(Sheet {
        left,
        slot: systems.iter().map(|s| s.height).max().unwrap_or(0),
        systems,
        measures,
        bars: played.iter().map(|b| b.bar).collect(),
        anchors,
        visits,
        opacity: src.place.opacity(),
    })
}

const PAD: usize = 12;
const SCROLL: f64 = 0.3;
const PAPER: Rgb = [255, 255, 255];
const INK: Rgb = [16, 16, 16];
const CURSOR: Rgb = [230, 90, 80];
const CURSOR_WIDTH: f64 = 3.0;
const BAR_BOX: Rgb = [70, 130, 230];
const BAR_BOX_ALPHA: f64 = 40.0;
// The cursor and the bar box reach this far above and below the staves.
const REACH: f64 = 10.0;

fn blend(px: &mut [u8], c: Rgb, alpha: u8) {
    for (v, c) in px.iter_mut().zip(c) {
        let (v16, c16, a) = (u16::from(*v), u16::from(c), u16::from(alpha));
        *v = ((v16 * (255 - a) + c16 * a) / 255) as u8;
    }
}

impl Sheet {
    pub fn height(&self) -> usize {
        self.slot + 2 * PAD
    }

    // The rows top..top + height() of a frame of RGB pixels that is this wide.
    pub fn draw(&self, pixels: &mut [u8], width: usize, top: usize, now: f64) {
        let Some((played, x)) = cursor(&self.anchors, now) else {
            return;
        };
        let bottom = top + self.height();
        let v = self
            .visits
            .partition_point(|v| v.start <= now)
            .saturating_sub(1);
        let since = now - self.visits[v].start;
        let progress = if v == 0 || since >= SCROLL {
            1.0
        } else {
            let p = since / SCROLL;
            p * p * (3.0 - 2.0 * p)
        };
        let current = self.visits[v].system;
        let previous = (progress < 1.0).then(|| self.visits[v - 1].system);
        let shown = [(previous, -1.0), (Some(current), 0.0)];
        let faded = |alpha: f64| (alpha * self.opacity).round() as u8;
        let paint = |pixels: &mut [u8], [x0, x1, y0, y1]: [f64; 4], c: Rgb, alpha: u8| {
            let rows = (y0.max(top as f64) as usize)..(y1.min(bottom as f64).max(0.0) as usize);
            let cols = (x0.max(0.0) as usize)..(x1.min(width as f64).max(0.0) as usize);
            for r in rows {
                for col in cols.clone() {
                    let at = (r * width + col) * 3;
                    blend(&mut pixels[at..at + 3], c, alpha);
                }
            }
        };
        paint(
            pixels,
            [0.0, width as f64, top as f64, bottom as f64],
            PAPER,
            faded(255.0),
        );
        for (system, place) in shown.into_iter().filter_map(|(s, p)| Some((s?, p))) {
            let s = &self.systems[system];
            let slot_top = (top + PAD) as f64 + (place + 1.0 - progress) * (self.slot + PAD) as f64;
            let y = slot_top + (self.slot - s.height) as f64 / 2.0;
            let (y0, y1) = (y + s.staff_top - REACH, y + s.staff_bottom + REACH);
            let is_current = place == 0.0;
            if is_current {
                let m = &self.measures[self.bars[played]];
                paint(
                    pixels,
                    [m.left, m.right, y0, y1],
                    BAR_BOX,
                    faded(BAR_BOX_ALPHA),
                );
            }
            let first_row = y.round() as isize;
            for (row, line) in s.ink.chunks_exact(s.width).enumerate() {
                let Ok(r) = usize::try_from(first_row + row as isize) else {
                    continue;
                };
                if r < top || r >= bottom {
                    continue;
                }
                for (col, &a) in line.iter().enumerate().filter(|p| *p.1 > 0) {
                    let at = (r * width + self.left + col) * 3;
                    blend(&mut pixels[at..at + 3], INK, faded(f64::from(a)));
                }
            }
            if is_current {
                let half = CURSOR_WIDTH / 2.0;
                paint(pixels, [x - half, x + half, y0, y1], CURSOR, 255);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measure(left: f64, columns: &[f64]) -> Measure {
        Measure {
            system: 0,
            left,
            right: left + 100.0,
            columns: columns.to_vec(),
        }
    }

    fn bar(bar: usize, start: f64, onsets: &[f64]) -> TimedBar {
        TimedBar {
            bar,
            start,
            end: start + 2.0,
            onsets: onsets.to_vec(),
        }
    }

    fn x_at(anchors: &[Anchor], t: f64) -> f64 {
        cursor(anchors, t).expect("a cursor").1
    }

    #[test]
    fn the_cursor_is_on_each_column_at_its_onset_and_moves_between_them() {
        let measures = [measure(0.0, &[10.0, 40.0, 70.0])];
        let a = anchors(&measures, &[bar(0, 1.0, &[1.0, 1.5, 2.4])]);
        assert_eq!(x_at(&a, 1.0), 10.0);
        assert_eq!(x_at(&a, 1.25), 25.0);
        assert_eq!(x_at(&a, 1.5), 40.0);
        assert_eq!(x_at(&a, 2.4), 70.0);
        assert_eq!(x_at(&a, 3.0), 100.0);
    }

    #[test]
    fn a_bar_whose_columns_do_not_match_moves_in_a_line_and_the_next_bar_stays_exact() {
        let measures = [measure(0.0, &[10.0, 50.0]), measure(100.0, &[110.0, 160.0])];
        let played = [bar(0, 0.0, &[0.0, 0.5, 1.0]), bar(1, 2.0, &[2.0, 3.5])];
        let a = anchors(&measures, &played);
        assert_eq!(x_at(&a, 0.0), 10.0);
        assert_eq!(x_at(&a, 1.0), 55.0);
        assert_eq!(x_at(&a, 2.0), 110.0);
        assert_eq!(x_at(&a, 3.5), 160.0);
    }

    #[test]
    fn a_repeat_jumps_back_and_the_lead_in_waits_at_the_first_column() {
        let measures = [measure(0.0, &[10.0]), measure(100.0, &[110.0])];
        let played = [
            bar(0, 0.0, &[0.0]),
            bar(1, 2.0, &[2.0]),
            bar(0, 4.0, &[4.0]),
            bar(1, 6.0, &[6.0]),
        ];
        let a = anchors(&measures, &played);
        assert_eq!(cursor(&a, -2.0), Some((0, 10.0)));
        assert_eq!(x_at(&a, 3.0), 155.0);
        assert_eq!(cursor(&a, 4.0), Some((2, 10.0)));
        assert_eq!(cursor(&a, 6.0), Some((3, 110.0)));
    }
}
