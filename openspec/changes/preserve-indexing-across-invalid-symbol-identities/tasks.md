## 1. Contract And Issue Alignment

- [x] 1.1 Map issue #467 to this change, synchronize its metadata and objective checklist, and keep RC2 scope limited to semantic bounded symbol-name admission plus repository publication continuity.
- [x] 1.2 Review the language/index-publication architecture views against the fix; update and render Mermaid only if the durable ownership or flow changes, otherwise record the reasoned N/A.

## 2. Symbol Extraction Boundary

- [x] 2.1 Remove the complete-declaration name fallback and admit only semantic declaration/declarator names in the shared tree-sitter projection path.
- [x] 2.2 Enforce the shared symbol-name bound before persistence without truncation or hashing, preserving exact admitted Unicode names and all other file facts.
- [x] 2.3 Advance the semantic projection fingerprint only if a normal refresh could otherwise retain invalid previously extracted identities.

## 3. Regression Coverage

- [x] 3.1 Add focused parser regressions for the reported 224/225-entry C# boundary, exact `D` field identity, malformed no-name declarations, overbound names, Unicode byte safety, and unaffected sibling symbols.
- [x] 3.2 Add real full and incremental CLI scan regressions proving the 225-entry repository publishes successfully, stays navigable, and preserves unrelated files, symbols, relations, and atomic failure behavior.
- [x] 3.3 Wire the focused scan regression into mandatory Linux/Windows CI and packaged RC proof without adding a new test framework or dependency.

## 4. Verification And Release Proof

- [x] 4.1 Run `cargo test -p projectatlas-symbols`, the owning CLI E2E filters, `cargo fmt --check`, `cargo check --workspace --all-targets --all-features`, warnings-denied workspace Clippy, full workspace/all-feature and doc tests, and warnings-denied docs with explicit timeouts.
- [ ] 4.2 Run strict OpenSpec, IssueOps checklist parity, ProjectAtlas lint, live review/automated-feedback disposition, and exact packaged RC2 scan/readback without displacing `v0.4.4` Latest.
- [x] 4.3 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.
