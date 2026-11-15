mod build;
mod fix;
mod read;
mod report;

use roxmltree::{Document, ParsingOptions};

pub use build::Keep;

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
    fix::note_velocities(&mut built, src.vel_lines.is_empty());
    no_mark_off_the_grid(&built.score, &built.rounded)?;
    let text = crate::fmt::format(built.score, crate::fmt::Fix::No)
        .map_err(|e| format!("internal: the import cannot write the file: {e}"))?;
    verify(&text)?;
    Ok(Imported {
        text,
        report: rep.lines(),
    })
}

// A mark lane shows a time only on a slot of a plain cell or of one tuplet group (SPEC 3.3).
// Voice lines can be off the grid in a bar with rounded tuplet lengths that do not add up.
fn no_mark_off_the_grid(score: &crate::model::Score, rounded: &[bool]) -> Result<(), String> {
    for (bi, bar) in score.bars.iter().enumerate() {
        let rounded = rounded.get(bi).copied().unwrap_or(false);
        for line in &bar.lines {
            let voice = line.lane.is_voice();
            if voice && !rounded {
                continue;
            }
            if crate::text::write::write_line(line.lane, &line.els, &bar.layout).is_ok() {
                continue;
            }
            let what = if voice {
                "a tuplet whose rounded lengths do not add up"
            } else {
                "a mark at a time that no slot shows"
            };
            return Err(read::stop(
                &format!("{what} (bar {}, {})", bar.label, line.lane),
                "",
            ));
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
