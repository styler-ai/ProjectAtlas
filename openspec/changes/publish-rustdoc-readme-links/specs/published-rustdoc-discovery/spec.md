## ADDED Requirements

### Requirement: Published Rustdoc Discovery
ProjectAtlas SHALL make generated cargo documentation discoverable from README and the GitHub Pages root.

#### Scenario: Docs workflow publishes rustdoc
- **WHEN** `04-Docs` runs on `main` or manual dispatch
- **THEN** it SHALL build rustdoc with warnings denied.
- **AND** it SHALL upload the rustdoc artifact to GitHub Pages.

#### Scenario: GitHub Pages root is opened
- **WHEN** a reader opens `https://styler-ai.github.io/ProjectAtlas/`
- **THEN** the Pages artifact SHALL include a root `index.html` that links to the ProjectAtlas rustdoc crate entries.

#### Scenario: README docs section is read
- **WHEN** a reader uses README to find documentation
- **THEN** README SHALL link to the published rustdoc site and the Markdown documentation files.

### Requirement: README And Page Freshness
ProjectAtlas PR closeout SHALL keep README and published documentation surfaces aligned with shipped behavior.

#### Scenario: PR changes public behavior or workflow
- **WHEN** a PR changes installation, CLI behavior, MCP behavior, release process, public API, token reporting, or documented agent workflow
- **THEN** the PR SHALL update README and relevant docs/page content before merge.

#### Scenario: PR has no docs-facing change
- **WHEN** a PR does not require README or page changes
- **THEN** the PR SHALL still confirm the docs freshness checklist item before merge.
