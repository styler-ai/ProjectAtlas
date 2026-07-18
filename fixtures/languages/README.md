# Structural Summary Fixtures

These fixtures capture representative language and declaration-light files that
must produce deterministic ProjectAtlas summaries instead of byte-count
fallbacks.

Python coverage is generated inside the E2E temporary repository from a Rust
test string instead of being checked in as a `.py` file. That keeps this
repository Rust-only while still proving Python summary extraction.
