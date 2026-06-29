//! Purpose: Validate compatibility behavior for legacy source purpose metadata.

use assert_cmd::Command;
use predicates::prelude::*;
use std::error::Error;
use std::fs;

#[test]
fn lint_does_not_require_legacy_source_purpose_headers() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    fs::create_dir(&repo)?;
    fs::create_dir_all(repo.join(".projectatlas"))?;
    fs::write(
        repo.join(".projectatlas").join("config.toml"),
        r##"[project]
root = "."
map_path = ".projectatlas/projectatlas.toon"
nonsource_files_path = ".projectatlas/projectatlas-nonsource-files.toon"
purpose_filename = ".purpose"

[scan]
source_extensions = [".foo"]
exclude_dir_names = [".git", ".projectatlas", "target"]
exclude_path_prefixes = []
non_source_path_prefixes = []

[purpose]
default_style = "javadoc"
line_comment_prefixes = ["//", "#", "--", ";"]

[purpose.styles_by_extension]
"##,
    )?;
    fs::write(
        repo.join(".projectatlas")
            .join("projectatlas-nonsource-files.toon"),
        "nonsource_files[]:\n",
    )?;
    fs::write(repo.join("sample.foo"), "fn sample() {}\n")?;

    Command::cargo_bin("projectatlas")?
        .current_dir(&repo)
        .arg("lint")
        .assert()
        .success()
        .stderr(predicate::str::contains("Missing Purpose headers").not())
        .stderr(predicate::str::contains("Purpose style suggestions").not());

    Ok(())
}
