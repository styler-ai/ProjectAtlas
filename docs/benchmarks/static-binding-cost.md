# Static binding cost and release handoff

This Linux measurement checks the bounded cost of the repaired static bindings and
source occurrences. It compares optimized default-feature binaries on the same
Ryzen 7 5700U host. The published 0.5.0 archive and extracted binary were checksum
verified. Exact identities, fixture hashes, work counters and protocol are in
[the compact result](static-binding-cost.json).

This comparison uses the reviewed contract10/10 source, including bare-var/recovered-initializer,
Python directive/export, deferred-body and lexical-retention corrections. It remains separate from release acceptance.

Three paired rounds alternate baseline/candidate order. Each sample initializes
a fresh non-Git fixture in a fresh process; filesystem/page caches remain warm.
Telemetry is disabled. Wall time includes the GNU time wrapper; peak RSS is the
process maximum. These affected-work cases do not replace #708's declared release
SLOs, unaffected navigation/upgrade comparisons, or #715's installed platform proof.

| Fixture | 0.5.0 median seconds | Candidate median seconds | 0.5.0 max RSS KiB | Candidate max RSS KiB |
| --- | ---: | ---: | ---: | ---: |
| Four Cargo roots, 256 callers | 0.2812 | 0.3447 | 27952 | 33204 |
| 64 Cargo roots, 256 callers | 0.3270 | 0.3935 | 28616 | 35140 |
| Dense/comment/nested source | 0.1647 | 0.1632 | 26648 | 29328 |

The root cases add actual ownership and caller resolution. Each timing sample
checks that its selected candidate call resolves. A separate post-measurement probe
confirms the selected baseline caller is unresolved and the candidate targets its
owning root's `run`; the permanent root E2Es also check exact target identity.
Their measured initial time increases by about 20–23%, and maximum RSS by 19–23%.
The dense case improves median time by less than 1%, with about 10% higher peak RSS.
These observations do not establish a general speedup or a root-count asymptotic
bound beyond the measured inputs.

The root fixtures use explicit binary targets at `targets/tN/main.rs`, each
containing `pub fn run()` and declared worker modules. Each worker imports
`crate::run as launch` and calls it. Holding 256 callers constant while raising
root count also adds 60 root files; total parsed candidates are 261 and 321.
The dense fixture contains one library, 12 nested inline modules, 256 flat aliases,
one nested grouped alias, a large block comment, and 257 calls on one physical
line. Its library and manifest are the two parsed candidates. All three candidate
samples retain every dense call occurrence behind the correct logical edge.
Every fresh sample reports one parse per admitted symbol candidate; every following
unchanged refresh reports zero parsed candidates.

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

The root-alias selector queries persisted imported callers and canonical summaries
through real MCP at both manifest-root checkpoints. The lexical-capacity selector
indexes four fact-free stress files, retains exact source navigation and unresolved
raw caller evidence, then proves normal CLI refresh/MCP repair, identity and no-op.
The Python-directive selector checks global/nonlocal read-before-write callers,
imported aliases, module/global writes, unaffected exports, canonical summaries,
unchanged-consumer refresh and rebuild convergence through real CLI and MCP.
The bare-var selector preserves local and exported callable bindings through
JS/TS alias chains, real-write invalidation and restoration, unchanged importers,
canonical summaries, incremental/full repair and no-op. Recovery-error source
controls retain raw unresolved calls and independently valid bare-var siblings.

The source unit matrix separately checks all 14 native grammar families, the Rust
build-script alias, admitted HTML/Vue/Svelte mappings, Unicode and CRLF coordinates,
multiline imports, qualified callbacks, filtering and cancellation. Static source
providers remain Rust, Python and ECMAScript; other native grammar paths do not
gain cross-file semantic providers. Grouped leaves can share their owning import
declaration range. Graph-only, optional or fallback facts without captured ranges
retain their existing evidence and trust boundaries.

## Combined release requirements

#708 must consume the final accepted #731, #732 and #738 source, retain its declared
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
approved-purpose status, no-op and rollback. Derivation and semantic contracts10/10
repair preceding private9/9 initializer and8/8 reservation facts, retain7/7 directive/
export repair and earlier6/6 deferred-binding and5/6 occurrence repair. The combined #738 derivation must be strictly
newer than this source contract to repair unchanged private10 document projections. Keep the separate0.4.5 schema-upgrade proof. Reconcile the
complete installed surface, clean parser-pack build, exact RC version/pins,
checksums and prerelease/Latest classification before publication acceptance.
