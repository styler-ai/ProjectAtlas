## Context

The symbols crate now admits only compact non-empty raw names and immediate parents up to 240 Unicode scalars. The language-neutral graph projection subsequently reconstructs fully qualified containment strings so equal local names under different nested scopes remain distinct. That derived string is currently unbounded and is converted to `GraphIdentityText`, whose 4,096-byte ceiling correctly protects canonical keys and SQLite rows. A valid deep symbol tree can therefore fail after parser admission and abort the publication transaction.

## Goals / Non-Goals

**Goals:**

- Keep every derived parent admitted by `GraphIdentityText` without losing valid nested symbols.
- Retain exact readable qualification while it fits and deterministic scope distinction after it does not.
- Apply one language-neutral rule to full and incremental graph publication.
- Keep allocation and hashing bounded by the existing graph identity ceiling.

**Non-Goals:**

- Raise or remove the graph identity limit.
- Relax raw symbol-name admission or hash malformed parser identities into validity.
- Change containment discovery, parser grammars, source positions, or SQLite schema.

## Decisions

### Make the graph byte ceiling an observable domain contract

`projectatlas-core` will expose the existing maximum graph identity bytes as a documented constant. The constructor remains the final validator. Projection can therefore distinguish the one expected composition overflow from other invalid text instead of treating every constructor error as an invitation to synthesize a value.

Repeating the literal `4_096` in the CLI is rejected because it would create the same cross-owner drift class as #471. Adding a new validated identity type is rejected because `GraphIdentityText` already owns the invariant.

### Compact only the derived qualification chain

`qualified_symbol_parents` will carry validated `GraphIdentityText` values. A readable `parent::name` remains byte-for-byte unchanged when it fits. When composition alone exceeds the ceiling, projection will replace that derived scope name with a bounded domain-separated BLAKE3 form that retains the nearest symbol name plus the digest of the complete candidate. Descendants continue from that bounded scope value and may compact again only when required.

The compact form uses one core-owned reserved prefix. Built-in source-symbol admission omits raw names or parents in that namespace, and the shared projection boundary rejects a directly constructed graph that bypasses parser admission. Exact raw qualification therefore cannot reproduce a compact derived identity.

This does not hash an invalid raw symbol into admission: every component has already passed parser-side admission and graph validation. The digest exists only to preserve stable scope distinction when the exact composed display cannot fit the established key boundary.

Dropping the parent, reverting to the immediate name, or omitting the child is rejected because repeated deep branches could collapse or lose valid symbols. Truncating the prefix is rejected because distinct scopes can share the retained suffix. Raising the ceiling is rejected because it transfers unbounded input into canonical keys and storage.

### Validate before entity construction

The qualifier will return `Result<Vec<Option<GraphIdentityText>>, CliError>`. Entity projection consumes the validated parent directly, so no later `GraphIdentityText::new` call can turn a valid symbol tree into a repository-wide failure. Any non-size contract violation still propagates fail-closed.

The existing ordering and active-containment algorithm remains `O(n log n)`. Every retained identity is bounded to the graph ceiling and each temporary candidate is bounded to one retained parent plus one parser-admitted component, so total qualification work is `O(n log n + n * limit)` rather than the current quadratic cumulative-string retention. A 4,000-deep regression at the existing per-file symbol ceiling protects the intended resource model. No new collection, worker, lock, I/O, or SQLite cost is added.

## Risks / Trade-offs

- [A compacted parent is less human-readable] -> Preserve the nearest name and compact only after the exact chain cannot be represented; exact shallow identities remain unchanged.
- [Two deep scopes collapse] -> Reserve the compact namespace from raw source identities, hash the complete bounded predecessor plus current name with a domain separator, and prove equal suffixes under different ancestors remain distinct and stable.
- [Hashing masks invalid raw input] -> Run raw admission first and use fallback only when validated components compose beyond the public byte ceiling; all other validation errors still fail.
- [Full and incremental keys diverge] -> Keep one deterministic projection function and compare both publication modes plus clean-scan convergence.
- [Deep valid input exhausts memory before validation] -> Collapse each step immediately and exercise the existing 4,000-symbol/file ceiling without retaining cumulative ancestor strings.

## Migration Plan

No schema or durable migration is required. The graph projection contract fingerprint changes so existing derived graph keys are rebuilt under the normal publication path. Authored state and source bytes remain unchanged.

## Dependencies / Cross-Issue Impact

This closes the remaining derived-qualification gap after the raw symbol-admission fix in #467. It is implementation-independent from #471 and #472; the shared RC3 workflow matrix proves all three changes on the same supported source and installed-candidate platforms.

## Open Questions

None.
