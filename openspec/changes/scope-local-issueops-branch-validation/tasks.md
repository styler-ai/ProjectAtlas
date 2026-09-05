## 1. Specification

- [x] 1.1 Specify candidate-branch task authority, accepted-base comparison, fail-closed ownership, preserved global scopes, release ownership, non-goals, and architecture-view applicability.

## 2. Candidate Branch Authority

- [x] 2.1 Add a local candidate-branch mode that reuses the existing owner-scoped accepted-base task comparison without requiring a hosted pull request.
- [x] 2.2 Route pre-push through that mode with exactly one mapped owning issue and an accepted `main` base; fail closed on zero or multiple owners, closed or unmapped ownership, and missing or unreadable base authority while leaving `main` global.

## 3. Proof and Documentation

- [x] 3.1 Cover concurrent task drift and failure behavior: the owner must match live state, every unrelated slice must equal the accepted base, and owner/base ambiguity or unrelated candidate edits must fail.
- [x] 3.2 Reconcile workflow and architecture documentation, then pass the IssueOps self-test, focused hook contract, strict OpenSpec validation, and repository gates.
