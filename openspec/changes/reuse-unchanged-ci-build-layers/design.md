## Context

The optional parser-pack workflow currently assigns a fresh `CARGO_TARGET_DIR` under `runner.temp` for each contained construction. Run `30035949564` established the cold baseline: the Linux construction job completed in 5m32s and the Windows job in 27m50s; their contained construction steps took about 4m55s and 25m51s respectively. The 150 grammar libraries are already supplied by pinned native bundles. The repeated work is primarily the ProjectAtlas Rust toolchain graph plus repeated audit and assembly.

The release boundary must remain stricter than ordinary incremental CI. A cache may accelerate dependency compilation, but it cannot become authority for candidate binaries, pack contents, receipts, or release acceptance.

## Goals / Non-Goals

**Goals:**

- Reuse unchanged third-party Cargo build state with exact, inspectable invalidation.
- Freshly rebuild every ProjectAtlas-owned crate from the checked-out candidate.
- Preserve all existing construction, containment, lifecycle, and fresh-runner gates.
- Provide an explicit cache-free release proof and measured Linux/Windows benefit.

**Non-Goals:**

- Caching prebuilt grammar bundles, assembled archives, candidate binaries, or verification receipts.
- Introducing a cache daemon, new ProjectAtlas crate, or generic build framework.
- Optimizing unrelated workflows before their repeat-build cost is measured.

## Decisions

### Use the official GitHub cache action with separate restore and save steps

The workflow will pin `actions/cache/restore` and `actions/cache/save` to the same reviewed commit. Restore occurs before contained construction. Save occurs only after construction, platform verification, exact-candidate revalidation, artifact upload, and removal of candidate-owned outputs.

This uses the platform's existing branch and key scoping and avoids a custom cache service. A custom artifact registry was rejected because cache availability is not a correctness requirement and would add credentials, lifecycle policy, and maintenance.

### Cache only the Cargo target dependency layer

The cache path is `${{ runner.temp }}/parser-pack-output/build`. The key is versioned and includes:

- target ABI;
- pinned `rustc` commit;
- runner image plus native compiler/SDK identity;
- hashes of `Cargo.lock` and all workspace Cargo manifests;
- one explicit cache-policy ABI version, bumped only when reusable artifact
  compatibility changes.

Keys are exact; there are no prefix restore keys. Grammar bundle inputs are excluded because they never enter the cached Cargo target layer. ProjectAtlas source revision is excluded because all seven owned packages are forcibly cleaned before use.

Hashing the whole workflow or construction script was rejected because diagnostics,
containment checks, and candidate assembly can change without changing Cargo
dependency artifact compatibility. Such edits must still run against the exact
candidate, but must not force an unrelated dependency rebuild.

The v3 policy binds the worker-feature construction closure introduced by this
change. After a v3 miss, the workflow may try one exact compatible v2 key composed
from the same target, Rust, native-toolchain, lockfile, and manifest inputs. A hit
crosses the current bounded validation and owned-package cleanup gates, then a
trusted successful dispatch saves the sanitized layer under v3. This migrates the
existing compatible state without a broad fallback or another dependency compile.

Caching package archives or grammar binaries was rejected because those bytes are already acquired by pinned digest and would broaden the trust boundary without addressing the dominant Cargo rebuild.

### Clean owned packages before use and before save

On a cache hit, contained construction runs one `cargo clean` invocation naming all seven workspace packages before any build. Cargo then decides which unchanged external dependency artifacts are reusable. After all construction and Windows broker verification, the workflow repeats the owned-package clean and removes the non-Cargo Windows broker from the cache path before saving.

Keeping candidate artifacts and relying only on Cargo fingerprints was rejected because a release proof should not execute a cached ProjectAtlas binary.

### Schedule required candidate work without weakening proof

Candidate-owned targets remain fresh, but they do not need separate serial Cargo
graphs or the unrelated main-CLI dependency closure. The `projectatlas` executable
keeps its full default `cli` feature, while parser-pack construction disables default
features and activates only the worker/supervisor closure. The release verifier is a
normal binary target, not an example target that pulls test-only dependencies into
construction. Windows builds the worker, assembler, and verifier in one Cargo
invocation. Linux keeps the worker's required linker-specific `cargo rustc`
invocation separate, then builds both release tools with that same worker feature
closure so Cargo reuses the already-built CLI library instead of compiling a second
supervisor-only variant.

Windows Cargo runs through the existing SID-restricted protected jobserver with a
fixed two-worker budget. A four-worker hosted trial increased elapsed construction
and did not complete before cancellation, so the lower measured bound avoids
oversubscribing concurrent Rust, linker, and native work. This uses the hosted runner
without restoring ambient jobserver authority or allowing unbounded host-wide
parallelism.

The two required assembly and archive passes run concurrently with separate inputs,
staging directories, and output paths. Both lanes still perform the complete native,
license, manifest, and archive audit, and the workflow still requires byte-identical
archives before publication. Concurrency is fixed at two lanes; it changes elapsed
time, not the proof boundary or output authority.

### Validate restored trees inside the contained construction boundary

The existing contained-construction script will accept an existing build directory only after checking that:

- the root and every descendant remain beneath the expected output path;
- no symlink or Windows reparse point exists;
- entries are only regular files or directories;
- fixed entry-count and total-byte ceilings are respected.

An invalid tree is atomically renamed to a quarantine sibling and replaced with an empty build directory. It is neither executed nor re-saved. Renaming instead of recursively deleting avoids following attacker-controlled path indirection.

### Separate iterative reuse from clean release acceptance

`workflow_dispatch` gains a boolean clean-construction input and an explicit target selector. Clean mode skips restore and save on both targets. The selector defaults to both release targets but may run exactly one target for a bounded platform diagnostic; the unselected construction, fresh-verifier, and runtime jobs do not exist in that matrix, and the aggregate release proof is skipped. Pull-request runs always retain both targets and may restore exact trusted state but never save. Only non-clean `workflow_dispatch` construction jobs that reach the save step after contained construction, platform verification, candidate revalidation, artifact upload, and cache sanitation are save-eligible. Downstream fresh-runner jobs remain release gates, but cannot govern a runner-local cache write after the construction runner has ended. A single-target diagnostic never satisfies final clean release acceptance.

Each construction-matrix target declares whether Cargo-layer reuse is enabled.
Disabled targets still perform the same empty-target construction and all proof
gates, publish a `disabled` disposition, and cannot restore or save cache state.

The workflow records a bounded JSON receipt per target with the cache disposition and SHA-256 of the key, not machine-local paths or cache contents. GitHub job/step timestamps remain the wall-time authority.

## Measured Target Decision

Hosted step timestamps and uploaded `parser-pack-<target>-cargo-layer-receipt`
artifacts establish the same-key cold/hit comparison:

| Target | Exact candidate and key SHA-256 | Cold contained construction | Exact hit | Improvement | Reuse decision |
| --- | --- | ---: | ---: | ---: | --- |
| `x86_64-unknown-linux-gnu` | `2b6df9d69c621200f0b8d991d185f978b3c1a654`, `68a445ba24eb3078bf918ae6d4a25c4c7902eae57ded07602d4d04800599ba93` | 134s in [run 30059696560](https://github.com/styler-ai/ProjectAtlas/actions/runs/30059696560), miss and save-eligible | 71s in [run 30059921214](https://github.com/styler-ai/ProjectAtlas/actions/runs/30059921214), exact hit, 2,033 entries and 481,216,004 restored bytes | 63s, 47.0% | enabled |
| `x86_64-pc-windows-msvc` | `5d6299615982c0014e5638d6838f659076511167`, `bac2b06283ba4121a5b9126764135317c8d6a6376ec96c19cd7d594894c11ab0` | 1,152s in [run 30056598826](https://github.com/styler-ai/ProjectAtlas/actions/runs/30056598826), miss and save-eligible | 1,064s in [run 30057665887](https://github.com/styler-ai/ProjectAtlas/actions/runs/30057665887), exact hit, 1,737 entries and 468,278,562 restored bytes | 88s, 7.6% | disabled |

The retained Linux v3 cache for the measured runner image is 129,196,905
compressed bytes. The measured Windows cache was 98,991,674 compressed bytes.
Windows saved more absolute seconds but only 7.6 percent of a long fixed proof
stage, so its cache lifecycle and trust surface do not meet the materiality
contract. The Linux proof runs were cancelled only after the Linux construction
job and receipt completed successfully, avoiding a knowingly unnecessary parallel
Windows build; the Windows comparison runs likewise retained successful construction
jobs and receipts before downstream cancellation.

## Risks / Trade-offs

- **A missing key input reuses incompatible native objects** → include toolchain, lock/manifest, target, runner/native identity, and an explicit reviewed cache-policy ABI; use no prefix fallback.
- **A restored build tree contains hostile filesystem objects** → validate in containment, quarantine without traversal, then rebuild clean.
- **Candidate artifacts survive package cleanup** → name every owned crate in one tested command and assert known candidate binaries are absent before save.
- **Cache transfer and validation cost approaches rebuild cost** → require at least 30 percent and 30 seconds of measured contained-step improvement on each target before keeping reuse enabled.
- **Cache eviction causes intermittent misses** → treat every miss as an ordinary clean construction, never as a failure.

## Migration Plan

1. Land the cache contract and focused validation/policy tests with reuse disabled only by the absence of a prior key.
2. Run one trusted non-clean dispatch to establish the cold baseline and populate eligible dependency state.
3. Run an unchanged-input dispatch and compare Linux/Windows contained-step timings.
4. Keep reuse only where both the 30 percent and 30 second materiality thresholds are met.
5. Before v0.4.0 acceptance, run and retain one successful clean-construction dispatch.

Rollback removes the restore/save steps and returns the build directory to always-empty behavior; no product data or schema migration is involved.

## Open Questions

None. Expansion to runtime-test targets or unrelated workflows requires separate measured evidence.
