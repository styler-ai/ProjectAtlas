//! Render or update the checked-in language capability matrix from registry authority.

use std::io::Write as _;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rendered = projectatlas_core::language::render_language_support_markdown()?;
    if let Some(path) = std::env::args_os().nth(1) {
        std::fs::write(path, rendered)?;
    } else {
        std::io::stdout().write_all(rendered.as_bytes())?;
    }
    Ok(())
}
