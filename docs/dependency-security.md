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
