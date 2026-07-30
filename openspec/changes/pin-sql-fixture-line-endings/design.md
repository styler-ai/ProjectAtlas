## Context

`projectatlas-db` uses `include_str!` to embed captured schema-8 SQL fixtures. A focused drift test replaces exact LF-terminated DDL fragments. The repository already pins source and documentation formats in `.gitattributes`, but SQL was omitted and therefore inherited platform checkout conversion.

## Goals / Non-Goals

**Goals:**

- Make embedded SQL fixture bytes deterministic on every supported checkout platform.
- Preserve the strict semantic-drift test unchanged.
- Verify the database suite on Windows.

**Non-Goals:**

- Change production migration code or accepted schema shapes.
- Add runtime normalization or another fixture-loading layer.

## Decisions

- Add `*.sql text eol=lf` to the existing repository line-ending policy.
- Retain the existing test mutations and fixtures so the regression remains observable if checkout normalization drifts again.

## Risks / Trade-offs

- Existing worktrees may retain CRLF files until refreshed; release verification uses a refreshed checkout and explicitly confirms LF checkout state.
- Contributors editing SQL on Windows receive LF working-tree files, matching the embedded test contract and canonical Git blobs.
