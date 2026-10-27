# Design

## Context

See proposal.md for scope. GitHub alert #2 identifies KaTeX 0.16.47 through Mermaid 11.16.1 in the repository's parse-only diagram validator. The upstream advisory fixes 0.18.2 and requires additional prototype-pollution and rendered-output conditions; package exposure does not establish exploitation of the Rust CLI.

Existing CI/release/IssueOps npm audit commands fail only at moderate severity. Dependabot versions cover Cargo and Actions but omit the npm validator. Five open bot PRs propose seven grouped Cargo updates, whichdisk 0.7.0 and three Action updates. Cargo versions have one workspace owner; Actions are SHA-pinned. Source changes and dependency changes invalidate downstream package/performance evidence.

Managed CodeQL was absent. The authorized configuration uses extended queries, remote/local sources and automatic language discovery. Its initial setup succeeded after retaining seven successful language analyses, including Rust; Java/Kotlin autobuild failed because the checkout has only parser fixtures without a build command. Online Codex security review now explicitly covers all PRs on every push with a low reporting threshold. Configuration readback and actual scan outcomes are distinct evidence.

## Goals / Non-Goals

**Goals:** remove the reported affected package, accept reviewed updates with causal compatibility/security proof, establish continuing monitoring and reconcile exact release evidence.

**Non-Goals:** implement another scanner, invent a Java/Kotlin application build for parser fixtures, treat alerts as proven bugs, disclose exploit details prematurely or change runtime/storage architecture.

## Decisions

### Prefer compatible upstream dependency graphs

Inspect the current upstream Mermaid maintenance lines first. Prefer the smallest supported upstream version that selects patched KaTeX; use a narrowly pinned override only if the maintained dependency range cannot admit the fix and explicit API/Node compatibility proof supports it. Do not hand-edit lockfile package identities or silence the alert. Review lockfile provenance, integrity, license and engine changes, then use normal locked installation and the existing Mermaid/IssueOps validator.

Include low npm advisories in the existing audit threshold and add weekly npm Dependabot coverage for the actual validator directory. This addresses both the vulnerable package and the missing update/admission mechanisms; a one-time lockfile bump alone would leave those gaps.

### Keep dependency ownership and semantic contracts

Review each bot diff and upstream release evidence before applying its intended update in the workspace owner. Regenerate the lockfile normally. Trace whichdisk's filesystem/WAL classification callers, Markdown/document parsing, identifier-table version, MCP schemas and runtime/resource behavior. Preserve existing public behavior and safety controls; changes to extracted or resolved facts require the existing parser/semantic fingerprint mechanism so unchanged indexes repair. Coordinate that mechanism with #731 rather than carrying old-contract projections forward. Reject stale proof from earlier dependency or workflow identities.

Verify Action commits against their upstream release refs and update all retained invocations together. Retain least privilege, supported runners, exact source/artifact handoff and SHA pins. Old bot branches are superseded only after their reviewed changes are accepted, with truthful linked disposition.

### Verify managed monitoring without another workflow engine

Use GitHub's managed CodeQL configuration and existing Dependabot/secret protections. Preserve the existing branch gates. Verify initial analyses, retained languages, weekly schedule and later PR/push activation; record unsupported fixture-only languages rather than weakening successful source coverage or fabricating a build. Online Codex security settings are verified in their authenticated repository UI; do not substitute local review or assume a missing PR review means a setting is off.

Import every initial alert with stable provenance and assess its actual source, control, sink, supported boundary and counterevidence. Test-only diagnostics, trusted operator paths and unreachable gadgets require evidence-backed disposition, not blanket dismissal. Fix confirmed failures at their owning boundary with broader RCA and independent review; new coherent fixes can receive direct #715 child issues, but all confirmed RC1 fixes remain required. Do not claim a clean repository merely because a job succeeded.

### Preserve intended product capabilities

A scanner label is not a product policy. Establish the supported trust boundary,
including explicit operator-selected paths and intentional CLI/MCP capabilities,
before changing runtime admission. Fix the demonstrated weakness at its owning
boundary and prove legitimate indexing, navigation, plugin, filesystem and release
workflows alongside negative/failure controls. Do not blanket-deny normal use to
make an alert disappear or silently narrow documented compatibility.

Inventory the complete open Dependabot security-alert set at admission and repeat
that readback at closure. Keep each alert's provenance and accepted fix or
evidence-backed not-applicable disposition; unresolved alerts remain explicit RC
blockers. The five update PRs and initial known advisory do not define the whole
security-alert set. The initial paginated REST snapshot contains only alert #2.

## Risks / Trade-offs

- Upstream package/API/engine changes can break validation or native behavior -> focused real parser, filesystem, MCP and platform controls, then required combined gates.
- Identifier/Markdown changes can leave stale stored semantics -> trace extraction differences and advance the existing owning fingerprint when necessary.
- Security scans can include intentionally incomplete fixtures or test helpers -> source-boundary assessment per finding, retained raw provenance and no blanket ignore.
- New dependencies or workflow refs invalidate prior installed/performance results -> #708/#715 consume the exact final accepted source and clean parser-pack identities.

## Migration Plan

Land reviewed manifests, generated locks, audit/update policy and documentation through the ordinary PR gates. Preserve databases, native identity and authored purposes; use ordinary fingerprint-driven repair where required. Re-run affected dependency, parser, MCP, filesystem, installed-upgrade and platform proof. A compatible rollback keeps existing preservation/refusal behavior; never reset derived state as a migration shortcut. Read back hosted alert/scanner/review state after acceptance.

## Architecture Disposition

N/A: dependencies and managed security settings remain within existing parser, filesystem, MCP, CI and publication owners. No new runtime owner, service, storage model or dataflow is introduced.

## Dependencies / Cross-Issue Impact

#732 directly belongs to and blocks #715 in v0.5.1-00, with no direct implementation prerequisite on #708 or #731. Their final proof must use the combined accepted dependencies. Any separately owned confirmed security repair remains a direct release child/blocker. Existing closed issue history is unchanged.

## Open Questions

None.

## Verification

Demonstrate the current low advisory before the update and its absence afterward using the actual npm audit output, locked installation and dependency tree. Pass the locked Mermaid parser, valid/invalid diagram and IssueOps regressions. Review and exercise affected Markdown, Unicode, filesystem/WAL, MCP, archive/resource and Action handoff boundaries. Use the required locked Rust formatting/check/Clippy/tests/doctests/rustdoc, strict strings, cargo-deny, dependency inventory, OpenSpec/IssueOps, mandatory native push and hosted supported-platform gates. Reconcile final scan findings, review state and installed/performance identities without claiming unrun proof.
