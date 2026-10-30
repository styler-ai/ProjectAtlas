# Static binding cost and release handoff

The current Linux measurements bind reviewed contract19 source `f7daa7a4fb96ec083322afd2acabe992f31bbccb` and
optimized binary `cf89e368c04bba0f5578167ca565756342edc312c14915b53c433d452ba31fbf`. Exact baseline identities, fixture hashes, work counters
and protocol are in [the compact result](static-binding-cost.json). The published
0.5.0 baseline archive and extracted binary were checksum verified.

There are66 initialization measurements in33 baseline/candidate pairs: three
alternating pairs for each of11 fixtures. Each initializes a fresh non-Git fixture
in a fresh process on the same Ryzen7 5700U host. Telemetry is disabled; ordinary
filesystem/page caches remain warm. Wall time includes GNU time; RSS is the process
maximum. Per-sample no-op and target probes are separate from that sample count.
Source-file counters do not claim native parser invocation counts.

These are bounded affected-work measurements, not exceptions to issue708 SLOs or
issue715 installed release acceptance. Baselines can perform incorrect or incomplete
work, so timing differences do not establish a general speedup or asymptotic bound.

| Fixture | 0.5.0 median seconds | Current19 median seconds | 0.5.0 max RSS KiB | Current19 max RSS KiB |
| --- | ---: | ---: | ---: | ---: |
| Four Cargo roots, 256 callers | 0.2852 | 0.3553 | 29644 | 33408 |
| 64 Cargo roots, 256 callers | 0.3258 | 0.3883 | 30640 | 34672 |
| Dense/comment/nested Rust | 0.1671 | 0.1609 | 28924 | 29392 |
| 256 contextual-keyword calls | 0.1018 | 0.1162 | 25352 | 25296 |
| 64 script programs, 63 callers | 0.1169 | 0.1429 | 25744 | 27640 |

The root fixtures hold256 worker callers constant and add60 root files when moving
from four to64 explicit Cargo targets (261 versus321 parsed candidates). Each
worker imports its actual root's run. Every candidate sample checks resolution;
a fresh current-artifact probe confirms exact root targets and baseline unresolved
state. Dense Rust has12 nested modules,256 flat aliases, one nested grouped alias,
a large comment and257 same-line call occurrences. Candidate samples retain every
occurrence. All following unchanged refreshes report zero parsed candidates.

The native contextual fixture has256 await calls in one non-strict TypeScript
function. The host fixture has one classic declaration and63 classic script callers,
each with a large comment. Candidate samples retain the expected resolved calls;
a fresh probe checks all63 exact host targets. The selected named caller anchors
are absent in the baseline contextual cases, rather than merely returning no calls.
These cases preserve existing roles and recovery bounds without compiler inference.

## Additional current comparisons

| Fixture | Baseline contract | Baseline median seconds | Current19 median seconds | Baseline max RSS KiB | Current19 max RSS KiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| 256 reopened C++ namespaces | 14 | 0.1877 | 0.1871 | 32516 | 31604 |
| 256 deferred instance fields | 15 | 0.1175 | 0.1162 | 27116 | 25528 |
| Shared forwarding query modules | 16 | 3.7152 | 2.0663 | 131880 | 130720 |

The namespace fixture checks selected exact callers and no-op. The field fixture
checks256 resolved occurrences to a unique later callable and no-op; its older
baseline does incorrect unresolved work. Source controls retain immediate class
syntax, captured writes, decorators and host roles.

The forwarding fixture has8,000 aliases, a1,902,910-byte exporter and1,800,002-byte
module shared by7,997 namespace-member calls plus one ordinary positive call. Every
sample checks the complete7,998-call summary, normal incoming caller and no-op.
A bounded one-row query reports an at_least total and is not a completeness oracle.
The improvement removes cumulative module copying; it does not claim a material
peak-memory reduction. The resource fixture also passes native18→19 repair, full
convergence, identity/all-approved-purpose preservation and current19 no-op.

## Native owner and admission costs

Each fixture repeats128 containers. Native owners span C++, Rust, Kotlin, Java and
Zig. Kotlin classes have16 constructor parameters and initializer/member calls.
C# classes have16 primary parameters, initializer closures, setter/local functions,
unknown value/parameter calls and normal outer callers. Every candidate sample
checks selected normal exact targets and no-op; C# samples also check shadow
negatives. These compare against the preceding optimized contract18 binary.

| Fixture | Contract18 median seconds | Current19 median seconds | Contract18 max RSS KiB | Current19 max RSS KiB |
| --- | ---: | ---: | ---: | ---: |
| Native member containers | 0.2232 | 0.2162 | 44304 | 42872 |
| Kotlin constructor views | 0.1605 | 0.1645 | 33700 | 31960 |
| C# accessor and constructor views | 0.2271 | 0.2271 | 39496 | 38072 |

The per-record admission vector replaces the previous all-row sort order; it does
not copy source graphs. Additional borrowed field validation remains bounded by
the existing raw-symbol cap and identity limits. Timing/RSS variations over these
three pairs are local observations, not a general optimization claim.

Dense owner queries preserve the default32KiB output envelope and request1MiB
intermediate headroom for anchor hydration. Earlier default128KiB intermediate
refusals on contract16/17 remain bounded-refusal controls. No public default,
maximum resource limit or normal navigation capability was relaxed.

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

The existing container selector additionally preserves an explicit cross-file
worker.run caller and its summary beside invalid NUL import identities, while
actual same-selector local collisions in another invalid-neighbor file remain
unresolved. It uses the installed-runtime override through CLI/MCP, source edits,
full rebuild, identity preservation and no-op. The broader typed-rejection E2E
separately checks JSON/TOON/MCP coverage and unchanged full reuse. Bad/reserved
name/parent, invalid/oversized/padded signature and rejected-ancestor source-model
controls prevent rejected rows from poisoning valid declarations or parent stacks.
No Java/Kotlin/C# compiler oracle or receiver/inheritance inference is claimed.
Qualified C++ definitions remain separately represented; compatibility selects the
actual in-class Method prototype rather than guessing class-vs-namespace ownership.

## Combined release requirements

Issue708 must consume final accepted731/732/738/739/741 source and retain its declared
latency/RSS targets across initial/full/rebuild, sparse/dense changes, current no-op,
unaffected navigation and supported upgrades on all four platforms. Report one-time
contract reconciliation separately from current-contract reuse. Recover each required
unchanged source once per operation, reuse captured sidecars and account work honestly.
No measurement here waives that acceptance.

Current DER19/SEM19 repairs compatible private indexes, including the contract18
invalid-neighbor regression caught before publication. The optimized candidate passes
15 actual native baselines: stable0.5.0 and contracts5–18. Those prove unchanged-source
repair, full convergence, all-approved-purpose/project-identity preservation, no-op,
early deadline/publication preservation, compatible rollback and re-upgrade. Early
deadline proof is separate from the unit control that cancels after staging.

Issue715 must run these permanent selectors against exact installed candidate packages
on Linux, Windows, macOS Intel and macOS ARM, retain complete CLI/MCP/plugin/host route
acceptance, and reconcile the final combined source. Its derivation must exceed19 and
retain SEM19 so old private candidates cannot bypass corrected facts. Keep separate
0.4.5 schema-upgrade proof, clean parser-pack construction, exact RC version/pins,
checksums and non-draft prerelease/Latest=false readback. Stable0.5.0 stays Latest.

The retained reexport_storage entry in the compact result is historical contract14
allocation evidence with its original source/binary identities and three paired
rounds. Its shared-module ownership, clone/accounting and cancellation controls
remain useful causal evidence; it is not a current19 timing campaign. The existing
source-evidence/publication/read-boundary architecture diagram remains accurate.
