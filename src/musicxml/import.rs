mod build;
mod fix;
mod read;
mod report;

use roxmltree::{Document, ParsingOptions};

pub use build::{Keep, with_x};

pub struct Imported {
    pub text: String,
    pub report: Vec<String>,
}

pub fn import(xml: &str, keep: Keep) -> Result<Imported, String> {
    let opts = ParsingOptions {
        allow_dtd: true,
        ..ParsingOptions::default()
    };
    let doc = Document::parse_with_options(xml, opts)
        .map_err(|e| format!("the file is not valid MusicXML: {e}"))?;
    let mut rep = report::Report::default();
    let src = read::read(doc.root_element(), &mut rep)?;
    let mut built = build::build(&src, keep, &mut rep)?;
    fix::ties(&mut built, &mut rep)?;
    fix::slurs(&mut built, &mut rep);
    fix::mark_velocities(&mut built, &src.vel_lines, &mut rep);
    fix::note_velocities(&mut built, &src.vel_lines);
    nothing_off_the_grid(&built)?;
    let text = crate::fmt::format(built.score, crate::fmt::Fix::No)
        .map_err(|e| format!("internal: the import cannot write the file: {e}"))?;
    verify(&text)?;
    Ok(Imported {
        text,
        report: rep.lines(),
    })
}

// A lane shows a time only on a slot of a plain cell or of one tuplet group (SPEC 3.3).
fn nothing_off_the_grid(built: &build::Built) -> Result<(), String> {
    for (bi, bar) in built.score.bars.iter().enumerate() {
        let rounded = built.rounded.get(bi).copied().unwrap_or(false);
        for line in &bar.lines {
            if crate::text::write::write_line(line.lane, &line.els, &bar.layout).is_ok() {
                continue;
            }
            let what = match (line.lane.is_voice(), rounded) {
                (true, true) => "a tuplet whose rounded lengths do not add up",
                (true, false) => "a note or rest at a time that no slot or group shows",
                (false, _) => "a mark at a time that no slot shows",
            };
            let number = built.numbers.get(bi).map_or("", String::as_str);
            return Err(read::stop(&format!("{what} ({})", line.lane), number));
        }
    }
    Ok(())
}

fn verify(text: &str) -> Result<(), String> {
    let parsed = crate::text::parse::parse(text);
    let errors: Vec<String> = crate::check::check(&parsed.score, &parsed.diags)
        .into_iter()
        .filter(|d| d.severity == crate::diag::Severity::Error)
        .map(|d| d.to_string())
        .take(5)
        .collect();
    if errors.is_empty() {
        return Ok(());
    }
    Err(format!(
        "internal: the imported file has errors:\n{}",
        errors.join("\n")
    ))
}
