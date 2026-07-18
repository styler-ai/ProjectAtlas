## 1. Spec and Issue Setup

- [x] 1.1 Create the OpenSpec proposal, design, spec delta, and task list with pre-mortem risks.
- [x] 1.2 Create GitHub issue #300, assign it to v0.3.25, and map `publish-rustdoc-readme-links` in `openspec/issue-map.json`.
- [x] 1.3 Mirror this task list into #300 under `OpenSpec Tasks`.

## 2. Documentation Workflow

- [x] 2.1 Add a `04-Docs` step that creates a root Pages landing page after rustdoc generation.
- [x] 2.2 Link README to the published rustdoc site and key crate entry points.
- [x] 2.3 Update `docs/index.md` with public docs surfaces and README/page freshness closeout rules.
- [x] 2.4 Add PR checklist wording that README/docs/page content is current before merge.

## 3. Verification

- [x] 3.1 Run `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked`.
- [x] 3.2 Run OpenSpec validation and issue checklist validation.
