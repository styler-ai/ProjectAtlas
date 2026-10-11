# Design

## Context

The existing Unix fixture selects `sha256sum` or `shasum`, canonicalizes the host tool, and links its generic name into a curated tool directory. The installed macOS ARM regression fails when Apple's relocated generic `shasum` script cannot dispatch to its real implementation. Apple's [Perl versioner](https://github.com/apple-oss-distributions/perl/blob/main/versioner/versioner.c) derives its prefix from the interpreter's canonical location and rewrites only script arguments beneath that prefix. Staging siblings beside a relocated script therefore does not solve the execution boundary.

## Goals / Non-Goals

Execute the real host checksum implementation through the existing curated command name, preserving every checksum, curated-path, readiness, stale-state, and non-mutation assertion. Keep production hook routing unchanged and distinguish native macOS fixture proof from real Nix-host acceptance.

## Decisions

Repair only the existing fixture's selected target. For the affected macOS `shasum` launcher, discover the active host's real versioned implementation and bind the curated `shasum` name directly to that implementation. Do not derive the script suffix by assuming it equals the full Perl runtime version; inspect the actual native host. After the absolute system command succeeds with the expected known-vector digest, exactly empty dispatcher diagnostics permit its canonical system path as a direct implementation. Nonempty diagnostics still require one valid mapping and reject an unresolved dispatcher. The staged command must then pass the same known vector under the curated PATH. This preserves standalone native tools without treating malformed or ambiguous dispatcher output as a direct implementation.

Keep the curated PATH and real checksum computation. A fake checksum tool or a full ambient PATH would remove the isolation boundary. Reuse the existing fixture and frozen-source owners; no product selector, workflow, dependency, or new harness is required.

The focused Cargo check remains `cargo test --locked -p projectatlas-cli --all-features --test e2e_lifecycle bundled_hook_distinguishes_ready_and_stale_mcp_without_mutation -- --exact --nocapture`. Also run the existing frozen-contract checks after updating both lifecycle digest rows.

On both native macOS architectures, prove that the curated `sha256sum` command is absent, record the resolved real `shasum` target, and compute a known SHA-256 vector through curated `shasum -a 256`. Then execute the standalone regression against the checksum-verified installed package with Cargo's binary variable absent and the failing job's isolated tool-selection inputs. A Cargo run that selects `sha256sum` is compatibility evidence, not proof of the affected path. Required affected local and hosted gates remain mandatory.

## Risks / Trade-offs

- An assumed version suffix could choose a missing or inactive script: discover the real active-host implementation and prove its resolved target on both Mac architectures.
- A fake tool or ambient PATH fallback could conceal the failure: preserve real checksum computation and every existing isolation assertion.
- Frozen metadata could drift: refresh only both existing lifecycle digest rows and run their causal checks.
- Synthetic isolation could overstate platform coverage: keep native macOS fixture proof separate from real Nix-host acceptance.

## Migration Plan

Reconcile the corrected issue and OpenSpec contract in the owning fix. Land the reviewed test-only change after native causal proof, then let #492 freeze a fresh main candidate. No installation or database migration is introduced. The earlier sibling-staging hypothesis is superseded by the interpreter-prefix evidence.

## Dependencies / Cross-Issue Impact

#669 has no code prerequisite on #668; delivery finishes the already-started #668 boundary first. #492 depends on both accepted fixes before fresh complete platform acceptance and closes last. Architecture is unchanged; the issue retains its reasoned N/A.

## Open Questions

None.
