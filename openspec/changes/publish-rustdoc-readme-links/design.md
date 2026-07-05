## Context

`04-Docs` already runs:

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked
```

and uploads `target/doc` to GitHub Pages. Local rustdoc output includes crate directories such as `projectatlas/`, `projectatlas_core/`, `projectatlas_db/`, `projectatlas_service/`, and `projectatlas_symbols/`, but no root `target/doc/index.html`. README should link the published documentation, and Pages root should guide readers to the CLI/runtime crate and supporting crates.

## Contract

The docs workflow SHALL:

- build rustdoc with warnings denied,
- create `target/doc/index.html` before uploading the Pages artifact,
- link to the ProjectAtlas CLI/runtime crate and supporting crates,
- avoid reading untracked local state.

README SHALL:

- link to the published GitHub Pages rustdoc site,
- link to the CLI/runtime crate entry point,
- keep existing Markdown docs links visible.

PR closeout SHALL:

- require README/docs/Pages-facing content to be updated when behavior, installation, release process, public API, or user workflow changes,
- keep release notes and docs aligned before issues are closed.

## Implementation Notes

- Generate the landing page with a workflow shell step so it always reflects the latest rustdoc artifact.
- Use stable crate paths:
  - `projectatlas/`
  - `projectatlas_core/`
  - `projectatlas_db/`
  - `projectatlas_service/`
  - `projectatlas_symbols/`
- Keep the landing page simple HTML without external assets.

## Edge Cases

- GitHub Pages root URL: should render the generated landing page.
- Local rustdoc validation: `cargo doc` remains the compile-time gate; the landing page is created only in the Pages workflow.
- PRs without user-facing changes: checklist can be satisfied by confirming README/docs remain current.

## Pre-Mortem

Risk: generated page links drift when crate names change.
Mitigation: crate rename changes should update this workflow step and README in the same PR.

Risk: README promises docs are current but release process forgets to update them.
Mitigation: add PR checklist language and include the rule in docs/index.md.

Risk: workflow heredoc breaks YAML indentation.
Mitigation: keep the generated HTML in one simple shell step and run YAML/CI validation through GitHub Actions.
