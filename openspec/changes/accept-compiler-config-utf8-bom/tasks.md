## 1. Compiler Configuration UTF-8 BOM Support

- [x] 1.1 Map `accept-compiler-config-utf8-bom` to GitHub issue #408 and keep the local OpenSpec artifacts internally consistent without mutating the remote issue.
- [x] 1.2 Strip only one exact leading UTF-8 BOM at the shared compiler-configuration decode boundary while preserving complete-byte bounds/hash authority, deadline/cancellation behavior, parse failures, and non-UTF-8 rejection.
- [x] 1.3 Add focused loader coverage for equivalent BOM/non-BOM `tsconfig.json` and `jsconfig.json`, malformed and non-UTF-8 input, plus one narrow real CLI/MCP refresh regression.
- [x] 1.4 Run strict OpenSpec validation, the issue-checklist diagnostic, formatting, focused tests, and warnings-denied workspace Clippy with hard timeouts.
