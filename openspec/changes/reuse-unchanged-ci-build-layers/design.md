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
- hashes of the optional-parser workflow and contained-construction cache policy.

Keys are exact; there are no prefix restore keys. Grammar bundle inputs are excluded because they never enter the cached Cargo target layer. ProjectAtlas source revision is excluded because all seven owned packages are forcibly cleaned before use.

Caching package archives or grammar binaries was rejected because those bytes are already acquired by pinned digest and would broaden the trust boundary without addressing the dominant Cargo rebuild.

### Clean owned packages before use and before save

On a cache hit, contained construction runs one `cargo clean` invocation naming all seven workspace packages before any build. Cargo then decides which unchanged external dependency artifacts are reusable. After all construction and Windows broker verification, the workflow repeats the owned-package clean and removes the non-Cargo Windows broker from the cache path before saving.

Keeping candidate artifacts and relying only on Cargo fingerprints was rejected because a release proof should not execute a cached ProjectAtlas binary.

### Validate restored trees inside the contained construction boundary

The existing contained-construction script will accept an existing build directory only after checking that:

- the root and every descendant remain beneath the expected output path;
- no symlink or Windows reparse point exists;
- entries are only regular files or directories;
- fixed entry-count and total-byte ceilings are respected.

An invalid tree is atomically renamed to a quarantine sibling and replaced with an empty build directory. It is neither executed nor re-saved. Renaming instead of recursively deleting avoids following attacker-controlled path indirection.

### Separate iterative reuse from clean release acceptance

`workflow_dispatch` gains a boolean clean-construction input. Clean mode skips restore and save on both targets. Pull-request runs may restore exact trusted state but never save. Only non-clean `workflow_dispatch` construction jobs that reach the save step after contained construction, platform verification, candidate revalidation, artifact upload, and cache sanitation are save-eligible. Downstream fresh-runner jobs remain release gates, but cannot govern a runner-local cache write after the construction runner has ended.

The workflow records a bounded JSON receipt per target with the cache disposition and SHA-256 of the key, not machine-local paths or cache contents. GitHub job/step timestamps remain the wall-time authority.

## Risks / Trade-offs

- **A missing key input reuses incompatible native objects** → include toolchain, lock/manifest, target, runner/native identity, and policy hashes; use no prefix fallback.
- **A restored build tree contains hostile filesystem objects** → validate in containment, quarantine without traversal, then rebuild clean.
- **Candidate artifacts survive package cleanup** → name every owned crate in one tested command and assert known candidate binaries are absent before save.
- **Cache transfer and validation cost approaches rebuild cost** → require measured 60 percent contained-step improvement on each target before keeping reuse enabled.
- **Cache eviction causes intermittent misses** → treat every miss as an ordinary clean construction, never as a failure.

## Migration Plan

1. Land the cache contract and focused validation/policy tests with reuse disabled only by the absence of a prior key.
2. Run one trusted non-clean dispatch to establish the cold baseline and populate eligible dependency state.
3. Run an unchanged-input dispatch and compare Linux/Windows contained-step timings.
4. Keep reuse only where the 60 percent threshold is met.
5. Before v0.4.0 acceptance, run and retain one successful clean-construction dispatch.

Rollback removes the restore/save steps and returns the build directory to always-empty behavior; no product data or schema migration is involved.

## Open Questions

None. Expansion to runtime-test targets or unrelated workflows requires separate measured evidence.
