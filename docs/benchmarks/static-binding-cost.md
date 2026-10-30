# Static binding cost and release handoff

This Linux measurement checks the bounded cost of the repaired static bindings and
source occurrences. It compares optimized default-feature binaries on the same
Ryzen 7 5700U host. The published 0.5.0 archive and extracted binary were checksum
verified. Exact identities, fixture hashes, work counters and protocol are in
[the compact result](static-binding-cost.json).

The measurements use the optimized contract 17/17 source, including source goals,
host roles, contextual-keyword/arrow recovery and exact module identities, alongside
the preceding declaration, binding, occurrence and bounded-forwarding repairs,
shared re-export module storage before staging admission, and reopened native
container/identifier ownership, deferred instance-field values/writes, and shared
module storage through runtime forwarding queries, plus native member/statement
and Kotlin construction/linked-scope ownership.
The source and binary identities are recorded in the compact result. The original
contract-12 fixtures and raw measurements remain preserved; the three shared fixture
source sets are unchanged. This evidence remains separate from release acceptance.

Three paired rounds alternate baseline/candidate order. Each sample initializes
a fresh non-Git fixture in a fresh process; filesystem/page caches remain warm.
Telemetry is disabled. Wall time includes the GNU time wrapper; peak RSS is the
process maximum. These affected-work cases do not replace #708's declared release
SLOs, unaffected navigation/upgrade comparisons, or #715's installed platform proof.

| Fixture | 0.5.0 median seconds | Candidate median seconds | 0.5.0 max RSS KiB | Candidate max RSS KiB |
| --- | ---: | ---: | ---: | ---: |
| Four Cargo roots, 256 callers | 0.2837 | 0.3469 | 27796 | 33124 |
| 64 Cargo roots, 256 callers | 0.3312 | 0.3860 | 28496 | 34480 |
| Dense/comment/nested source | 0.1664 | 0.1647 | 26920 | 29572 |
| 256 contextual-keyword calls | 0.1003 | 0.1141 | 23100 | 25848 |
| 64 script programs, 63 callers | 0.1122 | 0.1428 | 23932 | 27084 |

The root cases add actual ownership and caller resolution. Each timing sample
checks that its selected candidate call resolves. A separate post-measurement probe
confirms the selected baseline caller is unresolved and the candidate targets its
owning root's `run`; the permanent root E2Es also check exact target identity.
The root initial-time ratios are 1.223 and 1.166; maximum-RSS ratios
are 1.192 and 1.210. Dense Rust time/RSS ratios are
0.990/1.099. Native contextual time/RSS ratios are
1.138/1.119, and host ratios are 1.273/1.132.
Both contextual fixtures lack the selected named caller anchor in the baseline;
these compare different successfully extracted work. They establish neither a
general speedup nor a root-count asymptotic bound beyond these inputs.

The root fixtures use explicit binary targets at `targets/tN/main.rs`, each
containing `pub fn run()` and declared worker modules. Each worker imports
`crate::run as launch` and calls it. Holding 256 callers constant while raising
root count also adds 60 root files; total parsed candidates are 261 and 321.
The dense fixture contains one library, 12 nested inline modules, 256 flat aliases,
one nested grouped alias, a large block comment, and 257 calls on one physical
line. Its library and manifest are the two parsed candidates. All three candidate
samples retain every dense call occurrence behind the correct logical edge.
Every fresh sample reports all admitted source-file candidates parsed; these are
file counters, not native parser invocation counts. Contextual repair may use the
bounded additional native parses described by the source contract. Every following
unchanged refresh reports zero parsed candidates.

The native contextual fixture has 256 `await()` calls in one non-strict TypeScript
function plus a large comment. Every candidate sample retains all 256 resolved
occurrences. The host has one shared classic TypeScript declaration and 63 separate
classic script programs, each with a distinct caller and an approximately 8 KiB
comment. Each sample checks its selected caller; a post-measurement probe checks all
63 named callers against the exact shared declaration. Dots, source roles and program
ranges retain their ordinary contracts; this adds no framework or compiler inference.

## Permanent installed selectors

Run these tests through the existing installed-runtime override, against the exact
candidate package on Linux, Windows, macOS Intel and macOS ARM:

- `e2e_navigation::parenthesized_python_imports_resolve_and_converge_after_refresh`
- `e2e_navigation::rust_root_aliases_follow_manifest_and_module_changes`
- `e2e_navigation::rust_namespace_aliases_keep_values_types_and_toolchain_identity`
- `e2e_navigation::rust_absolute_and_bare_use_paths_follow_the_owning_edition`
- `e2e_navigation::rust_added_automatic_roots_and_broken_cycles_repair_unchanged_consumers`
- `e2e_navigation::same_line_call_occurrences_keep_exact_ranges_after_refresh_and_rebuild`
- `e2e_navigation::lexical_capacity_preserves_navigation_and_repairs_after_source_reduction`
- `e2e_navigation::python_directive_calls_agree_across_cli_mcp_and_refresh`
- `e2e_navigation::bare_var_redeclarations_preserve_cli_mcp_calls_and_export_refresh`
- `e2e_navigation::ecmascript_source_contexts_preserve_cli_mcp_targets_and_host_roles`
- `e2e_navigation::reopened_container_calls_agree_across_cli_mcp_and_refresh`

The root-alias selector queries persisted imported callers and canonical summaries
through real MCP at both manifest-root checkpoints. The lexical-capacity selector
indexes four fact-free stress files, retains exact source navigation and unresolved
raw caller evidence, then proves normal CLI refresh/MCP repair, identity and no-op.
The Python-directive selector checks global/nonlocal read-before-write callers,
imported aliases, module/global writes, unaffected exports, canonical summaries,
unchanged-consumer refresh and rebuild convergence through real CLI and MCP.
The bare-var selector preserves local and exported callable bindings through
JS/TS/TSX alias chains, real-write invalidation and restoration, unchanged importers,
canonical summaries, incremental/full repair and no-op. Recovery-error source
controls retain raw unresolved calls and independently valid bare-var siblings.
The same selector checks separate body/parameter environments, class static blocks
and TypeScript namespaces through persisted CLI/MCP graph and summary queries.
Generator declarations and their async forms retain body/parameter ownership and
canonical callers; default/named generator exports survive explicit re-export
chains. Admitted Vue/Svelte/HTML script hosts preserve the same ownership.

The source-context selector queries exact targets and canonical called-by summaries
through both public CLI and MCP after initialization, a source-goal edit and full
rebuild, then checks identity and no-op publication. It covers Script names,
strict references/invalid bindings, implicit arguments, HTML source goals/global
visibility, Vue/Svelte scope direction and role validity, static import admission
and explicit-language TypeScript compatibility, and supported versus unsupported host exports.
Same-stem HTML/Svelte targets retain their explicit suffix identities. Source-key
controls additionally cover configured aliases/baseUrl, native output substitutions,
unknown explicitly selected suffixes and wrong-directory basename negatives.
Concise-arrow grammar recovery and async/module/header negatives have causal
source-unit coverage; those CLI fixture arrow names reach the same native path.

The source unit matrix separately checks all 14 native grammar families, the Rust
build-script alias, admitted HTML/Vue/Svelte mappings, Unicode and CRLF coordinates,
multiline imports, qualified callbacks, filtering and cancellation. Static source
providers remain Rust, Python and ECMAScript; other native grammar paths do not
gain cross-file semantic providers. Grouped leaves can share their owning import
declaration range. Graph-only, optional or fallback facts without captured ranges
retain their existing evidence and trust boundaries.

## Combined release requirements

#708 must consume the final accepted #731, #732, #738, #739 and #741 source, retain its declared
latency/RSS targets, and measure affected source recovery, initial/full/rebuild,
sparse/dense edits, current-contract no-ops, unaffected navigation and supported
upgrades on all four platforms. Report genuine one-time full contract repair
separately from current-contract reuse. Fresh source must reuse its parsed sidecar;
unchanged required sources are recovered once per operation and charged honestly.
The four-root and 64-root measurements above are additional affected-work evidence,
not exceptions to those release targets.

#715 must include these installed selectors in its existing candidate package
matrix and fixture inventory, preserve the public CLI/MCP/plugin/host routes, and
exercise stable 0.5.0 plus preceding development-contract repair, identity,
approved-purpose status, no-op and rollback. Current derivation and semantic
contracts 17/17 repair the source-context, occurrence-owner, arrow recovery and
explicit module-identity defects and reopened-container/identifier and deferred-field/write defects while
retaining the preceding ownership repairs and correcting native member, constructor,
nested-class and C++ condition ownership.
The current optimized default-feature candidate passes actual native upgrades from
stable 0.5.0 and contracts 5 through 16, including unchanged-source repair, approved
purposes and identity, full convergence, no-op, early deadline refusal, rollback
and re-upgrade. An early deadline does not establish mid-publication cancellation.
The combined #738/#739/#741 derivation must be strictly newer
than the final #731 derivation, retaining its semantic contract, so unchanged
private candidates cannot bypass the document/exclusion repairs. Keep the separate 0.4.5 schema-upgrade proof. Reconcile the
complete installed surface, clean parser-pack build, exact RC version/pins,
checksums and prerelease/Latest classification before publication acceptance.


The re-export storage regression has a separate paired comparison against the
preceding optimized contract-14 source. One 65,538-byte missing module literal is
shared by 512 explicit export slots; an independent ordinary imported caller must
still resolve to `target.js::work`. Every sample also verifies a zero-parsed no-op.
The long module is retained as source evidence without inventing a filesystem target.

Across three alternating paired rounds, initialization median fell from
0.1610 to 0.1365 seconds and maximum peak RSS fell from
56,752 to 25,444 KiB. This is about
15.2% less elapsed time and
55.2% less peak RSS for this bounded regression fixture.
The source hash set and both executable identities are in the compact result.
Causal source tests additionally check shared module allocation, clone behavior and
retained-byte accounting across JavaScript, TypeScript, TSX, Vue, Svelte and HTML.
This targeted repair does not waive the separate release performance gates.


The current contract-17 comparison adds 256 same-file C++ namespaces, each reopened
with a caller of its original declaration. Three alternating paired rounds against
the preceding optimized contract-14 source measured 0.1855 versus
0.1862 seconds median and 32,572 versus 31,156 KiB maximum RSS.
Every candidate sample verifies the selected resolved call and zero-parsed no-op.
Source-unit and CLI/MCP controls cover nested/anonymous C++ visibility, ambiguity,
canonical C# partial owners, TypeScript shared/private owners, source edits, full
convergence, summaries and identity. Generic identities share the existing lexical
budget, with overflow and near-limit controls. No release SLO exception is granted.

The field regression adds 256 classes with instance-field calls to a unique later
enclosing callable. Against contract15, three alternating paired rounds measured
0.1220 versus 0.1169 seconds median and 26,652 versus 25,408 KiB maximum RSS.
Every candidate sample checks all 256 resolved occurrences and a zero-parsed no-op.
The baseline performs incorrect unresolved work, so this establishes no general
speedup. Causal controls cover captured writes/exports, immediate class syntax,
decorators and callbacks across JS/TS/TSX and every admitted host role. The permanent
source-context selector checks 28 field cases through CLI/MCP graph and summaries,
source edits, full convergence, identity and no-op.

Runtime forwarding queries now retain the source module Arc through construction,
cache hits and frame clones. A 1,902,910-byte exporter with 8,000 aliases and a
1,800,002-byte module feeds 7,997 namespace-member calls plus one normal positive
call; two imports keep the caller exactly within its existing 8,000-relation cap.
Three alternating paired optimized rounds against the preceding contract16 source
measured 3.7392 versus 2.0427 seconds median
(45.4% less elapsed time), with maximum RSS 130,084 versus 130,244 KiB.
This removes cumulative copying; it does not claim a material peak-memory reduction.
Every sample verifies all 7,998 calls via the canonical summary, a normal exact
caller and a zero-parsed no-op. A one-row graph query correctly reports an
`at_least` total, not a full count; it was not used as the completeness oracle.

The causal pointer test fails before the fix and passes afterward; a separate
value-ordering control preserves cache identity across distinct allocations.
Budget refusal, cycle/depth, cleanup and cancellation tests pass. The permanent
bare-var forwarding selector adds a smaller long-module workload across JS/TS/TSX,
including unresolved and normal positive CLI/MCP navigation and changed-source
MCP indexing. Ordinary native imports retain their existing 240-character
canonical-admission boundary; they cannot deliver this long module to the
per-leaf import producer or import-scope cache. Those adjacent paths need no new
restriction or speculative rewrite. Source-fact/cache ownership charges shared
bytes; query-owned text and headers remain accounted before retained growth.

The storage-only repair preserved16 key values. Native member/statement changes
now require DER17/SEM17. The large resource fixture passes16-to17 repair, then
same17 no-op/full rebuild with identity/all-approved-purpose preservation and
normal navigation. The earlier re-export storage comparison above keeps
its exact contract-14 source/binary identities as historical causal evidence.
All current table/namespace/field/query comparisons bind the source and binary
recorded in the compact result. Full combined #708 and installed #715 proof remains
mandatory before RC1 publication.


The contract17 owner comparison adds128 repeated container groups across C++, Rust,
Kotlin, Java and Zig, plus a separate128-class Kotlin fixture with16 constructor
parameters, initializer closures and member calls per class. These compare against
the preceding optimized contract16 source and verify exact selected targets plus
zero-parsed no-ops in every candidate sample.

| Current owner fixture | Contract16 median seconds | Contract17 median seconds | Contract16 max RSS KiB | Contract17 max RSS KiB |
| --- | ---: | ---: | ---: | ---: |
| Native member containers | 0.2220 | 0.2090 | 41120 | 39664 |
| Kotlin constructor views | 0.1556 | 0.1612 | 29932 | 28448 |

All60 current paired samples bind the same contract17 source/binary. The owner
selector preserves its default32KiB output envelope and requests1MiB intermediate
headroom for dense symbol-anchor hydration. The initial dense Java query correctly
refused its default128KiB intermediate budget on both contract16 and17; this is a
bounded refusal, not a binding regression or timing sample. Existing32MiB maximum
intermediate admission remains unchanged. The source/resource and native-CLI proofs
do not waive issue708's combined performance or issue715's installed release gates.

The permanent container selector now also checks native member bodies, associated
constant qualification, Kotlin constructor/nested/property shadows and C++ while
scope through CLI/MCP targets and summary callers, incremental/full convergence,
identity preservation and no-op. Native source controls cover additional body and
capture variants and aggregate constructor-view overflow. No Kotlin/Java compiler
oracle or arbitrary receiver/inheritance analysis is claimed. The existing diagram's
source-fact, bounded recovery, publication and canonical read boundaries still apply.
