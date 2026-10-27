# Dependency compatibility and security

Dependency updates retain ProjectAtlas's existing owners and capabilities. The
workspace manifest and lockfile own Cargo versions; workflow invocations retain
reviewed upstream commit pins. Scanner alerts require supported-boundary evidence
and compatibility checks before changing product behavior.

## Parser and identifier upgrades

The Markdown parser uses its existing full option profile. The retained
`pulldown-cmark` patch fixes malformed-wikilink panics and repeated-wikilink event
amplification. The production extraction wrapper has regressions for malformed
input, linear candidate counts and exact source positions. Ordinary Markdown
navigation and existing byte, fact and cancellation bounds remain available; the
fix does not blacklist legitimate syntax or disable wikilinks.

Python import normalization now uses the canonical dependency's Unicode 18 XID
tables. Underscore starts, combining continuations, ordinary ASCII/Unicode aliases,
hard-keyword rejection and join-control rejection retain their rules. Source
spelling remains exact; this profile does not select a target interpreter version
or introduce NFKC name equivalence. Historical release specifications describe
their historical table version.

The semantic fingerprint includes the dependency's reported Unicode table version.
Extraction and semantic contract changes make compatible unchanged indexes repair
through the existing controlled publication owner. Identity and approved purposes
must survive; subsequent unchanged refresh and compatible rollback remain part of
acceptance. No database reset or new persistent metadata authority is needed.

## Diagram validation and monitoring

The diagram validator retains its pinned Mermaid grammar and Node range while
selecting the patched KaTeX dependency through a scoped override. See its
[dependency notes](../.github/mermaid-parser/README.md) for the published advisory,
behavior checks and override-removal condition. Existing npm audit gates include
low-severity advisories, and Dependabot covers its actual manifest directory.

Repository settings enable dependency alerts/update proposals, secret protections,
managed CodeQL analysis and online Codex security reviews. Configuration, completed
coverage and individual findings are separate evidence. Parser fixtures without an
application build are not claimed as successfully scanned application languages.
An alert on an intentional operator-selected path, test helper or unreachable
dependency gadget requires evidence-based assessment; it is not a reason to remove
normal CLI/MCP, indexing, plugin or release functionality.

The initial local commit-subject checker finding identified avoidable repeated
suffix scanning for malformed nested issue markers. The checker now rejects that
input with a non-overlapping marker expression. Its existing single-owner grammar
and support for long legitimate subjects remain intact. This repair concerns local
validation complexity; it does not establish a remotely exploitable CLI defect.

## RC dependency baseline and acceptance handoff

The reviewed dependency and workflow baseline is
[the source tree used by PR #733](https://github.com/styler-ai/ProjectAtlas/tree/59a990c20c177afeefc519c9b24de4611b734b21).
Its workspace manifest and generated Cargo lockfile, Mermaid manifest and generated
npm lockfile, parser/semantic fingerprint owners and SHA-pinned workflow callers
are the immutable input authorities. Subsequent documentation or checklist changes
do not replace that source evidence. Integration must retain those accepted
dependency/workflow inputs and use the actual accepted merge as its Git baseline.

This baseline advances the index derivation contract to **4** and semantic
resolution contract to **5**, including the canonical Unicode 18 XID data version.
The import-binding repair must build from these changes and advance its own
changed extraction and semantic contracts; it must not restore the earlier
constants, omit the identifier profile or treat old projections as current.

| Owner | Required next boundary |
| --- | --- |
| [#731](https://github.com/styler-ai/ProjectAtlas/issues/731), tasks 2.6–2.7 | Integrate this baseline, reconcile the final parser/semantic contracts, prove unchanged-index repair and preservation, and supply permanent installed import/caller regressions. |
| [#708](https://github.com/styler-ai/ProjectAtlas/issues/708), remaining performance acceptance | Rerun required work, latency and resource measurements against the combined accepted source; include affected-only source recovery and distinguish first contract repair from subsequent refreshes. |
| [#715](https://github.com/styler-ai/ProjectAtlas/issues/715), installed RC acceptance | Construct the clean parser pack for the exact combined commit, verify its artifact handoff and exact RC packages on all four platforms, and exercise installed CLI/MCP/plugin/host routes, upgrade, identity/purposes, no-op, failure and compatible rollback before publication. |

The dependency baseline's real CLI check reproduced the stable malformed-link
panic and verified successful repair. A separate existing-index check retained a
nonempty project identity and the exact approved purpose, including its authored
timestamps, through repair, a second refresh with no new generation, and reopening
with the compatible stable runtime. These checks use the unchanged source owners
linked above. Permanent causal parser controls are
`markdown_parser_handles_malformed_wikilinks_without_panicking` and
`markdown_parser_preserves_linear_wikilink_occurrences` in
[the Markdown extractor](../crates/projectatlas-symbols/src/markdown.rs);
`semantic_resolution_contract_tracks_identifier_data_version` in
[the semantic contract owner](../crates/projectatlas-symbols/src/resolution_keys.rs)
protects the identifier invalidation boundary. This complements the existing
publication and lifecycle tests; it does not replace installed RC acceptance.

[Source CI](https://github.com/styler-ai/ProjectAtlas/actions/runs/40541767793)
passed the Rust, repository and four-platform smoke checks for this baseline.
The [clean parser diagnostic](https://github.com/styler-ai/ProjectAtlas/actions/runs/40535426134)
verified Linux construction, fresh-runner download and normal-runtime lifecycle
through the changed artifact actions. That diagnostic is not the final combined
all-platform parser-pack or packaged release proof. Those downstream checks remain
mandatory under their owners above; completing the dependency handoff does not
claim that they have run.
