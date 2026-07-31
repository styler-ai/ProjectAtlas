## 1. Recovery Harness Correction

- [x] 1.1 Give only the post-failure healthy recovery probe a platform-tolerant no-progress allowance while retaining one launch attempt and its existing attempt deadline.
- [x] 1.2 Preserve hostile-case no-progress bounds, exact typed failures, containment checks, and cleanup requirements.
- [x] 1.3 Verify repeated adversarial-suite success on Windows and, before the post-stall healthy launch, probe its configured no-progress allowance at a synthetic 550 ms progress age so the suite deterministically rejects the hostile 500 ms bound without consuming any of the real two-second recovery window.
- [x] 1.4 Run `cargo test --locked -p projectatlas-cli --test parser_supervisor_adversarial --all-features`, `cargo test --workspace --all-targets --all-features --locked`, and `cargo test --workspace --doc --all-features --locked` with a 20-minute timeout per command, then run `gh pr checks --watch --fail-fast` with a 60-minute timeout against the ordinary `01-CI` Windows/Linux/macOS x64/macOS arm64 gates without changing production behavior.
