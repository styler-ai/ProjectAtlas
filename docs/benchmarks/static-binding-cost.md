# Static binding cost and release handoff

The current Linux measurements bind reviewed contract21 source `3e81518048dce82a987a53daed5381028fe2dfae` and
optimized binary `4bca2b1b2ea623d946183c5a511e563873a8d2bd592b17572048c2841da62339`. Exact baseline identities, fixture hashes, work counters
and protocol are in [the compact result](static-binding-cost.json). The published
0.5.0 baseline archive and extracted binary were checksum verified.

There are 84 initialization measurements in 42 baseline/candidate pairs: three
alternating pairs for each of 14 fixtures. Each initializes a fresh non-Git fixture
in a fresh process on the same Ryzen 7 5700U host. Telemetry is disabled; ordinary
filesystem/page caches remain warm. Wall time includes GNU time; RSS is the process
maximum. Per-sample no-op and target probes are separate from that sample count.
Source-file counters do not claim native parser invocation counts.

These are bounded affected-work measurements, not exceptions to issue708 SLOs or
issue715 installed release acceptance. Baselines can perform incorrect or incomplete
work, so timing differences do not establish a general speedup or asymptotic bound.

| Fixture | 0.5.0 median seconds | Current21 median seconds | 0.5.0 max RSS KiB | Current21 max RSS KiB |
| --- | ---: | ---: | ---: | ---: |
| Four Cargo roots, 256 callers | 0.2851 | 0.3478 | 27260 | 32876 |
| 64 Cargo roots, 256 callers | 0.3297 | 0.3932 | 27876 | 34712 |
| Dense/comment/nested Rust | 0.1691 | 0.1604 | 26152 | 29232 |
| 256 contextual-keyword calls | 0.1020 | 0.1159 | 22992 | 25848 |
| 64 script programs, 63 callers | 0.1164 | 0.1429 | 23320 | 27060 |

The root fixtures hold 256 worker callers constant and add 60 root files when moving
from four to 64 explicit Cargo targets (261 versus321 parsed candidates). Each
worker imports its actual root's run. Every candidate sample checks resolution;
a fresh current-artifact probe confirms exact root targets and baseline unresolved
state. Dense Rust has 12 nested modules, 256 flat aliases, one nested grouped alias,
a large comment and 257 same-line call occurrences. Candidate samples retain every
occurrence. All following unchanged refreshes report zero parsed candidates.

The native contextual fixture has 256 await calls in one non-strict TypeScript
function. The host fixture has one classic declaration and 63 classic script callers,
each with a large comment. Candidate samples retain the expected resolved calls;
a fresh probe checks all 63 exact host targets. The selected named caller anchors
are absent in the baseline contextual cases, rather than merely returning no calls.
These cases preserve existing roles and recovery bounds without compiler inference.

## Additional current comparisons

| Fixture | Baseline contract | Baseline median seconds | Current21 median seconds | Baseline max RSS KiB | Current21 max RSS KiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| 256 reopened C++ namespaces | 14 | 0.1911 | 0.1941 | 32384 | 31152 |
| 256 deferred instance fields | 15 | 0.1191 | 0.1203 | 26176 | 25020 |
| Shared forwarding query modules | 16 | 3.7906 | 2.0344 | 131408 | 130428 |

The namespace fixture checks selected exact callers and no-op. The field fixture
checks 256 resolved occurrences to a unique later callable and no-op; its older
baseline does incorrect unresolved work. Source controls retain immediate class
syntax, captured writes, decorators and host roles.

The forwarding fixture has 8,000 aliases, a 1,902,910-byte exporter and 1,800,002-byte
module shared by 7,997 namespace-member calls plus one ordinary positive call. Every
sample checks the complete 7,998-call summary, normal incoming caller and no-op.
A bounded one-row query reports an at_least total and is not a completeness oracle.
The improvement removes cumulative module copying; it does not claim a material
peak-memory reduction. The resource fixture also passes native20→21 repair, full
convergence, identity/all-approved-purpose preservation and current21 no-op.

## Native owner and admission costs

Each fixture repeats 128 containers. Native owners span C++, Rust, Kotlin, Java and
Zig. Kotlin classes have 16 constructor parameters and initializer/member calls.
C# classes have 16 primary parameters, initializer closures, setter/local functions,
unknown value/parameter calls and normal outer callers. Every candidate sample
checks selected normal exact targets and no-op; C# samples also check shadow
negatives. The C++ qualified fixture has 128 namespace bodies and 128 type bodies;
selected calls must use their exact owner rather than the same-name global. Its
baseline already has correct qualified-owner behavior. The Python comprehension
fixture uses128 functions with a local run declaration, initial iterable call,
post-write unknown call and unrelated keep call. Its baseline drops the valid
iterable target. The ordinary Python control uses128 functions with four calls to
the global run and one to keep, with no walrus or comprehension. Exact source
occurrences and target parents are checked in every candidate sample. These
compare against optimized contract20; only equivalent work supports overhead
comparisons.

| Fixture | Contract20 median seconds | Current21 median seconds | Contract20 max RSS KiB | Current21 max RSS KiB |
| --- | ---: | ---: | ---: | ---: |
| Native member containers | 0.2217 | 0.2145 | 41772 | 42856 |
| Kotlin constructor views | 0.1624 | 0.1613 | 31604 | 32096 |
| C# accessor and constructor views | 0.2363 | 0.2303 | 36652 | 37596 |
| C++ qualified namespace and type bodies | 0.1728 | 0.1698 | 30824 | 31868 |
| Python comprehension order | 0.1449 | 0.1433 | 27184 | 28056 |
| Ordinary Python calls without walrus | 0.1376 | 0.1349 | 25932 | 26860 |

The per-record admission vector replaces the previous all-row sort order; it does
not copy source graphs. Additional borrowed field validation remains bounded by
the existing raw-symbol cap and identity limits. Timing/RSS variations over these
three pairs are local observations, not a general optimization claim.

Dense owner queries preserve the default 32KiB output envelope and request 1MiB
intermediate headroom for anchor hydration. Earlier default 128KiB intermediate
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
The existing Python directive selector adds13 comprehension fixtures across
module/global/local/nonlocal/import ownership, repeated/filter/singleton/nested
execution, class/iteration/iterable syntax controls and Unicode CRLF. CLI and MCP
retain every source occurrence, exact parents and called-by summaries through
retarget/restore, full rebuild, identity preservation and no-op. The43-case CPython
3.14 oracle has35 compiler-valid cases (29 executed successfully) and8 deliberate
syntax controls; six valid cases deliberately fail at runtime. Nested bodies that
were only defined are not claimed executed. Shared-name allocation controls prove
one32KiB text allocation across48 Python/C++ ancestor barriers, preserving admitted
slots. Measured x86_64 Scope size grows320→336 bytes, NameBindings remains56; this
is at most256KiB extra per live capped collector, not total RSS or a cross-platform
layout claim.
No Java/Kotlin/C# compiler oracle or receiver/inheritance inference is claimed.
Qualified C++ definitions remain separately represented. Exact source owner paths,
unknown/alias/base/inline barriers, later-directive ordering and parenthesized
wrappers pass the same installed override; compatibility also preserves the actual
in-class Method prototype. No qualified-call or receiver inference is added.

## Combined release requirements

Issue 708 must consume final accepted731/732/738/739/741 source and retain its declared
latency/RSS targets across initial/full/rebuild, sparse/dense changes, current no-op,
unaffected navigation and supported upgrades on all four platforms. Report one-time
contract reconciliation separately from current-contract reuse. Recover each required
unchanged source once per operation, reuse captured sidecars and account work honestly.
No measurement here waives that acceptance.

Current DER21/SEM21 repairs compatible private indexes, including the contract18
invalid-neighbor regression caught before publication. The optimized candidate passes
17 actual native baselines: stable 0.5.0 and contracts5–20. Those prove unchanged-source
repair, full convergence, all-approved-purpose/project-identity preservation, no-op,
early deadline/publication preservation, compatible rollback and re-upgrade. Early
deadline proof is separate from the unit control that cancels after staging.
A separate native comparison reuses each of the17 exact fixture roots, rolls back
to its original baseline and upgrades again, then compares all normalized graph,
symbol/source relation, span, summary, purpose, text and classification projections with a
full scan. Surrogate row IDs, operation timestamps, rejection generations and node
mtime are excluded explicitly; semantic stable keys, source spans, content and
coverage remain compared. Every baseline preserves identity/all approved purposes
and passes a final no-op. This supplements the target-level checks with complete
stored semantic projection equality; publication counters are not used as an
equality oracle.

Issue 715 must run these permanent selectors against exact installed candidate packages
on Linux, Windows, macOS Intel and macOS ARM, retain complete CLI/MCP/plugin/host route
acceptance, and reconcile the final combined source. Its derivation must exceed21 and
retain SEM21 so old private candidates cannot bypass corrected facts. Keep separate
0.4.5 schema-upgrade proof, clean parser-pack construction, exact RC version/pins,
checksums and non-draft prerelease/Latest=false readback. Stable0.5.0 stays Latest.

The retained reexport_storage entry in the compact result is historical contract14
allocation evidence with its original source/binary identities and three paired
rounds. Its shared-module ownership, clone/accounting and cancellation controls
remain useful causal evidence; it is not a current21 timing campaign. The existing
source-evidence/publication/read-boundary architecture diagram remains accurate.
