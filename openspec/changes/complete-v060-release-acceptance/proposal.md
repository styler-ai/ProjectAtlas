## Why

v0.6.0 is not accepted merely because its feature issues merge. It changes the installed CLI/MCP compatibility boundary through #310 and adds authored Memory Atlas state through #314, so one feature-free release owner must prove that the complete installed ProjectAtlas surface—including unchanged routes—still works together on exact artifacts and hosted state.

## What Changes

- Make #493 the native GitHub parent of every other accepted `v0.6.0-00` issue and directly blocked by each child; the hierarchy communicates milestone scope/progress while direct `blocked by` relationships control execution order.
- Freeze one exact candidate only after every child issue, required review, accepted compatibility disposition, and local specification is complete.
- Reconcile and safely execute the complete installed CLI command/nested-command and MCP tool inventory, including unchanged routes, against isolated fixtures.
- Run one holistic installed-product workflow spanning installation, initialization, indexing, navigation, #310 surface behavior, #314 storage/recovery/reflection/host behavior, upgrade, rollback, privacy, concurrency, pressure, and failure recovery.
- Publish and independently read back `v0.6.0-rc1`, return confirmed defects to their owning issues, repeat the proof after any changed revision/artifact, then promote and read back stable v0.6.0.
- Close #493 last; it implements no feature or bug and never substitutes for owning tests or reviews.

Non-goals:

- No duplicate #310/#314 implementation, new public route, schema change, product workaround, or release-only compatibility exception.
- No mutation or administration against developer or production state; all executable proof uses isolated homes, repositories, databases, registries, caches, and host configuration.
- No help/schema-only acceptance, workflow-green shortcut, direct stable publication, premature Latest movement, or closure while any child/review/route/artifact/readback is partial.

## Capabilities

### New Capabilities

- `v060-release-acceptance`: native release hierarchy, exact-candidate public-surface regression, holistic installed-product E2E, prerelease remediation, stable promotion, and closes-last truth.

### Modified Capabilities

None. The release owner composes accepted child contracts and does not redefine them.

## Impact

- Read-only composition of the complete installed CLI/MCP/host/plugin/skill/runtime/database surface on Windows, Linux, macOS x64, and macOS arm64 as supported.
- Release workflow, assets, checksums, installers, npm/plugin/runtime identity, prerelease/Latest metadata, IssueOps/OpenSpec/review/milestone state, and independent hosted readback.
- No product schema or feature ownership. Confirmed failures return to #310, #314, or another accepted sanitized owner.
