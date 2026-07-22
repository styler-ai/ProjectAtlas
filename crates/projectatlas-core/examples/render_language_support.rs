//! Render the Markdown or Pages language-and-ecosystem projection from one authority.

use std::io::Write as _;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let first = arguments.next();
    let (rendered, path) = if first.as_deref() == Some(std::ffi::OsStr::new("--html")) {
        (
            projectatlas_core::support_catalog::render_language_support_html()?,
            arguments.next(),
        )
    } else {
        (
            projectatlas_core::language::render_language_support_markdown()?,
            first,
        )
    };
    if let Some(path) = path {
        std::fs::write(path, rendered)?;
    } else {
        std::io::stdout().write_all(rendered.as_bytes())?;
    }
    Ok(())
}
