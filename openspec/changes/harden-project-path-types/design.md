## Context

The nearest-project routing feature intentionally lets absolute path calls use another indexed ProjectAtlas root only when explicitly enabled. The current implementation uses helper functions, but the type system still sees many values as raw `PathBuf` or `String`. Rust can prevent future mistakes by making path role explicit.

## Goals / Non-Goals

**Goals:**
- Make selected-project roots, indexed roots, absolute paths, and repository keys distinct at helper boundaries.
- Preserve current CLI/MCP behavior while making unsafe conversions harder to write.
- Add path edge-case tests that cover Windows and Unix syntax regardless of the host platform where possible.

**Non-Goals:**
- Do not rewrite all path handling across every crate.
- Do not change public CLI path syntax.
- Do not make nearest-project routing default-on.
- Do not add async MCP task progress, background task persistence, or cross-process job management in this routing-hardening change.

## Decisions

- Start at the MCP/runtime boundary. This is where selected roots, explicit `project_path`, nearest indexed roots, and repository-relative file keys meet.
- Use newtypes with private fields and fallible constructors for role-changing conversions. Keep the wrappers local until a cross-crate API need is proven.
- Property-test pure conversion helpers where possible; use table-driven tests for filesystem-backed cases requiring actual `.projectatlas/projectatlas.db`.

## Risks / Trade-offs

- Newtypes can add boilerplate -> keep wrappers narrow and conversion methods obvious.
- Platform path behavior differs on Windows and Unix -> include host-agnostic tests for normalized strings plus host-specific tests behind `cfg`.
- Refactoring routing code can regress behavior -> keep #276 edge-case tests green throughout.
