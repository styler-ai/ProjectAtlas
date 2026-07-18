## Why

ProjectAtlas already builds rustdoc in CI and publishes `target/doc` through GitHub Pages, but README does not clearly link the generated cargo documentation. The Pages upload also needs a root landing page so the published documentation URL is useful even when rustdoc does not generate `target/doc/index.html`.

The user also wants README and the published page to stay current after PRs close, not drift behind shipped behavior.

## What Changes

- Add a GitHub Pages root `index.html` generation step after `cargo doc`.
- Link the published rustdoc site and key crate entry points from README.
- Update `docs/index.md` to document the public docs surfaces and closeout freshness rule.
- Add a PR checklist item requiring README/docs/Pages-relevant content to be refreshed before PR closeout when behavior changes.

## Capabilities

### New Capabilities
- `published-rustdoc-discovery`: Makes generated cargo documentation discoverable from README and GitHub Pages root.

### Modified Capabilities
- Documentation workflow.
- README and documentation closeout process.

## Release Scope

This change is scheduled for v0.3.25 and is documentation/workflow only.

## Non-Goals

- Do not replace rustdoc with a separate static-site generator.
- Do not publish private workspace memory or local agent state.
- Do not require manual editing of generated `target/doc` files.

## Pre-Mortem

Likely failure modes:
- README links to a rustdoc path that is not published.
- GitHub Pages root is blank or 404 because rustdoc did not emit `index.html`.
- A future PR changes behavior but ships stale README/docs.
- The workflow overwrites rustdoc assets or depends on local-only files.

Mitigations:
- Generate `target/doc/index.html` after `cargo doc` and before Pages upload.
- Link stable rustdoc crate paths under `https://styler-ai.github.io/ProjectAtlas/`.
- Add explicit PR checklist and docs wording that README/docs/page freshness is a closeout requirement.
- Keep the generated landing page independent of local workspace state.
