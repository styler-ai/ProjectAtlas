## 1. Recovery Harness Correction

- [x] 1.1 Give only the post-failure healthy recovery probe a platform-tolerant no-progress allowance while retaining one launch attempt and its existing attempt deadline.
- [x] 1.2 Preserve hostile-case no-progress bounds, exact typed failures, containment checks, and cleanup requirements.
- [x] 1.3 Verify repeated adversarial-suite success on Windows.
- [x] 1.4 Run `cargo test --locked -p projectatlas-cli --test parser_supervisor_adversarial --all-features`, `cargo test --workspace --all-targets --all-features --locked`, `cargo test --workspace --doc --all-features --locked`, and `gh pr checks --watch --fail-fast` against the ordinary `01-CI` Windows/Linux/macOS x64/macOS arm64 gates without changing production behavior.
