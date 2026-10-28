## Context

See proposal.md for confirmed reproductions. The current producer compacts import source indiscriminately; normalization drops comment-bearing, continued, mixed or nested bindings. Independently, canonical import aliases and summary callers pool bindings by file, while generic local resolution treats same-name declarations as call targets without lexical evidence. Native Go/C/C++/Java/C#/Kotlin probes confirm this beyond the three cross-file source providers. Current public SymbolGraph does not contain enough scope or static-export identity to reconstruct correct bindings.

## Goals / Non-Goals

Preserve static dependencies and truthful callers through extraction, normalization, projection, incremental repair and both query adapters. Apply lexical safety to the registry-derived native parser paths; keep Rust/ECMAScript/Python cross-file provider boundaries unchanged. Preserve public SymbolGraph/CLI/MCP shapes, database schema, source identity, literal contents, trust, cancellation and publication ownership. No runtime execution, wildcard expansion, new providers or arbitrary dynamic default inference.

## Decisions

### One typed source-evidence sidecar

Extend the parser-owned extraction result used by runtime with concrete typed import/export binding, lexical scope/local declaration and call-site evidence from the existing AST. Preserve the public graph-only extraction functions as compatibility wrappers. The sidecar is private derived evidence, not a second database or public graph schema. Keep declaration/import/call ordinals and exact byte spans associated explicitly; never encode semantics in target/context/detail strings or synthetic symbols. Validate correspondence after fact admission/filtering; loss or ambiguity abstains rather than attaching evidence to the wrong ordinal. Use closed enums, borrowed spans while visiting, and bounded owned facts at the runtime boundary. No parser-per-import or trait framework. A pinned canonical ECMAScript syntax helper supplies identifier character and reserved-word predicates without introducing another parser.

### Syntax normalization belongs to its grammar owner

Walk grammar-owned comments and token nodes before display compaction; preserve strings, escapes, Unicode spelling, token separation and source context. Python explicit line continuations are syntax trivia. Rust use trees flatten into independent existing import facts by module path, preserving aliases/self/namespace siblings. ECMAScript mixed clauses retain independent module, named, namespace and default binding identities. A default binding is a named default slot, not a namespace import. Only explicit unambiguous static export evidence may resolve it to an existing declaration; named direct default functions/classes are the initial static forms, while anonymous/default-expression cases remain unresolved unless exact declaration evidence exists. Do not infer from sole exports or names.

### Validate ECMAScript bindings beyond tolerant syntax

Tree-sitter's TypeScript identifier token can admit reserved binding words without
an ERROR node. Grammar shape and has_error therefore do not prove a valid local
BindingIdentifier. Use pinned oxc_syntax 0.156.0 (MIT, Rust 1.97 compatible with the
pinned toolchain), default features disabled, solely for canonical Unicode
identifier and reserved-word predicates. Keep Tree-sitter as the only AST and
source-role/span authority. This intentionally refines the original no-new-
dependency preference: Unicode XID is not ECMAScript ID_Start/Continue, and a
local partial character/keyword implementation would duplicate the language
standard. The allocator/span/macro dependency fan-out is a real compile/supply-
chain cost; review its locked graph, licenses and installed size/resource impact.
Do not add the OXC parser or parse each import again.

Decode identifier Unicode escapes through the existing UnicodeEscapeSequence
owner, accepting only Unicode escapes and scalar code points (no UTF-16 surrogate
pair folding for identifiers). Validate the cooked name with the canonical
predicates and the two strict binding early errors eval/arguments. Apply that
policy to local binding roles, not an aliased imported ModuleExportName. Preserve
raw source spelling/spans. Invalid bindings abstain per leaf; separately proven
static module dependencies and supported sibling leaves survive. Escape handling,
keyword policy and the identifier profile belong in semantic invalidation.

Preserve every explicit default-slot occurrence, including unsupported expressions
and export clauses, as typed unresolved evidence when no declaration is proved.
Competing slots invalidate default target proof across the module. Filtering a
declaration removes its target, not its export-slot occurrence; host correspondence
must preserve that distinction. Named direct function/class export facts also
carry their source-proved roles through fallback host display rows.

### Embedded source evidence follows host admission and merge ownership

Runtime candidate admission honors the existing registry's embedded host capability,
including HTML/HTM aliases and inferred host paths; an explicit supplied language
remains authoritative. A structural-only host parser does not imply that its
admitted script regions lack symbol work. Plain HTML retains structural summaries;
CSS/XML and unsupported script regions gain no JavaScript provider.

Reuse each existing same-length JS/TS projection parse. Remap its declarations,
imports and calls through explicit append-only host ordinals, then seal against the
original host bytes. Keep structural/fallback host provenance. A compatibility row
can stand for one captured import/call occurrence; repeated same-line occurrences
must survive independently. Coalesced declaration identities cannot narrow an
ambiguous native binding. Independent JS/TS projection import ordinals remain
independent. Correspondence failure discards binding proof and retains fallback
coverage rather than inventing an association. Bound merge work with indexed
identities and per-occurrence compatibility consumption, avoiding repeated scans
of growing same-name occurrence lists.

### Binding ownership controls calls

At each call site, select language-valid visible lexical bindings before same-file or imported-name matching. Parameters/locals shadow declarations and imports; local imports are visible only within their scope. Preserve independently scoped aliases. Dynamic or insufficient binding evidence prevents exact local and cross-file fallback while retaining the raw call relation with honest resolution/trust. Preserve explicit Rust crate/self/super qualification and unshadowed standard-toolchain roots through AST-owned static references. Apply the same lexical lookup to a proven bool.then function-item callback's own AST argument and byte extent; the enclosing method's callee binding is not callback proof. Language owners account for function/block scope, declaration visibility and conservative conditional/closure behavior. The registry supplies variants; grammar-only local graphs do not gain cross-file providers.

Binding identity also needs static module/declaration evidence: Rust path length alone does not distinguish a root function from a namespace. Preserve root-function aliases and namespace-qualified calls using the same explicit source/module evidence; ambiguous crate/module ownership must not become a guessed target.

Deferred Python/ECMAScript bodies admit a unique later enclosing binding, using a
precomputed stable suffix of the owning visibility timeline rather than scanning
future transitions for every call. Immediate calls, direct IIFEs, Python definition
headers and the first generator iterable retain source order. Pending ECMAScript
writes link to actual lexical owners after declaration collection; captured writes
with unknown execution timing invalidate both caller and exported-value authority.
No execution or general control-flow analysis is introduced.

ECMAScript bare var declarators reserve the existing function/module binding
without appending a value transition. Existing function, parameter and initialized
variable evidence stays authoritative; a previously absent name still shadows an
outer binding. Ordinary for initializers share this rule, while for-in/of targets
retain their actual write transitions. Lexical let/const and other language locals
keep their existing declaration rules. Reuse the bounded NameBindings entry and
existing AST fields rather than introducing another resolver or execution pass.
Function body declarations share the body's var environment; parameter defaults
use their distinct parameter environment and cannot see body locals. Static class
blocks and admitted TypeScript namespaces own their vars. A var declaration
reserves its body owner separately from its lexical initializer/iteration write,
so a valid catch parameter receives that write without replacing an outer value.
Ordinary nested block functions remain block-owned. Reuse admitted AST scope and
pattern leaves; no additional parse or source pass is needed.

Malformed declarators cannot supply no-write or direct-callable initializer proof.
Check both their own recovery errors and an immediately following error/missing
node after comments; this catches a recovered equals sign outside the declarator
without poisoning independently valid comma-delimited siblings. Raw recovered
calls remain available with unsupported binding authority withdrawn.


Python global/nonlocal names are selected once from the same AST under the shared
binding limit; a cheap source-keyword prefilter only skips that cooperative cursor
pass and never authorizes a directive. Retain read-order markers only for actual
directive names, so files without directives consume no read-marker capacity.
Link directive owners after enclosing declarations exist. A typed owner redirect
preserves current-scope evaluation order without reserving a false function local.
Actual enclosing mutation invalidates unrelated/deferred readers; one directive
writer may still read its own prior value before its write. Multiple writers stay
conservative. Nonlocal lookup skips class environments and stops at an intervening
function's global declaration. Prior value reads invalidate an illegal late
directive, while annotation scopes and parameter/import/member labels do not.
Bare annotations reserve function locals without overwriting existing values;
annotating the directive's own name is invalid and cannot authorize calls.
Named-expression and match-capture transitions use their grammar-owned target and
actual binding position. This is bounded lexical evidence, not execution analysis.

The same source-owned module export facts withhold Python callable keys for
rebound declarations, including valid global writers and ordinary module writes.
Keep unrelated public exports and the existing private-name policy intact. Reuse
the existing unresolved-declaration marker, remapping and budget owners. Public
Python declaration-only graphs therefore need captured-source recovery when their
export evidence is consumed; cache each required source once and retain actual
parse/work accounting. This does not add Python runtime re-export inference.

Bound native traversal depth at 256 before recursive refinement. Bound retained
lexical environments and binding transitions/pending writes across all namespaces independently at 16,384
per file, since empty blocks and un-emitted locals need not consume symbol or
relation capacity. Other correspondence maps remain bounded by admitted facts.
On exhaustion, preserve raw facts and proven prefix ownership, suppress all binding
and export authority, and allow normal navigation, reduction and refresh repair.
Concurrent workers multiply these fixed per-file bounds; no unbounded scope vector
or per-call timeline scan is admitted. Contract 11/11 additionally repairs function
body/parameter/static/namespace environments in unchanged private10/10 indexes,
retaining bare-var reservation, malformed initializer, Python directive and earlier
private9/9,8/8,7/7 and6/6 repairs
without changing schema, wire shapes or authored state.

Rust qualified paths use the type/module namespace independently of local values.
Register imported paths and module/type declarations in that environment; type
parameters block incompatible fallback while value parameters do not. Keep direct
function-call lookup in the value environment. Preserve every static component of
an imported member path. Export a parented declaration only under its owning
namespace scopes, never additionally at the unparented source root.

### Static re-exports use bounded explicit alias chains

Retain explicit Rust pub-use and ECMAScript export-from alias facts in the sidecar. Resolve a consumer's binding through a bounded module/export chain to an existing declaration, retaining module dependencies for every required step. Do not synthesize declarations or attach alias exports to unrelated stable entity owners. Preserve ambiguity and detect cycles using bounded visited identities; no unbounded fixed-point pass or global display-name relinking. Sidecar lookup is cached once per exact captured source file and limited by the existing indexing work budget.

Both Rust and ECMAScript forwarding use the existing operation staging byte ceiling
and 10,000-row dependency limit. Reserve each unique cause before cloning it and
check the row limit before insertion; duplicate keys need no new reservation.
Scoped construction releases its bytes on cancellation/error and transfers them
once into a completed cached owner. Rust frames account both query copies; root,
slot and candidate-call indexes admit growth before insertion. Shared source
inventory uses one immutable Arc, not a second map. Generic forwarding borrows
cached module keys, admits missing lookup keys before database access and retains
them once in loaded state. Generic cycle suffixes add causes directly to their
parent workspace, avoiding an uncached result whose charge outlives that result.
Visited query copies are admitted before insertion and removed on every exit.
Preserve completed child caches when a parent refuses. Fixed per-entry estimates
are conservative accounting, not an exact allocator/RSS limit; single requests,
validated paths and per-source derivation retain existing source/identity bounds.

### Persist canonical outcomes and query them consistently

Reuse the existing canonical key, entity, relation, occurrence and dependency tables and atomic publication owner. The sidecar controls which scoped keys/targets are admissible and which raw calls must remain unresolved; generic local matching must obey that decision too. File-summary called_by consumes resolved incoming canonical calls using the existing indexed bounded adjacency/entity APIs rather than independently rebuilding aliases or same-name targets from raw persisted text. Missing normalized evidence must not fabricate callers or trigger a query-time parse/mutation. Preserve output shapes and honest coverage behavior. The summary bridges display metadata only to a unique already-canonical selector in the exact owning file, including kind, normalized signature and parent; a qualified-parent suffix can narrow candidates but cannot choose among multiple full selectors. Query only resolved incoming Calls, cache duplicate entity keys, retain at most the existing20 callers per symbol and propagate entity/candidate/adjacency omissions through the existing summary truncation signal. Use existing typed entity hydration bounds and cap candidate correspondence work to four times the graph row ceiling. Keep all correspondence and adjacency in one read snapshot; preserve a caller-owned snapshot. This is a bounded metadata/adjacency read, never another resolver or source parse.

### Reuse only evidence sufficient for the requested projection

Freshly parsed source reuses its sidecar without another parse. Untouched canonical graph rows outside an affected closure stay authoritative and are not reparsed. Where full or affected projection needs source evidence absent from a reconstructed legacy SymbolGraph, recover it once per required file through the existing captured inventory, bounded source read, hash validation and controlled extraction path (as existing Markdown fact recovery does). Only requested closure paths and explicit static import/export targets are admitted; cache by exact source identity and account bytes/parsing/work honestly. Refuse changed/missing source, cancellation or exhausted bounds without publishing. Never silently derive exact lexical/default facts from old flat relations. Advance parser and semantic contract fingerprints so prior indexes cannot bypass repair. Benchmark this affected-only recovery; do not hide extra parsing as reuse.

## Risks / Trade-offs

- Removing comments or normalizing text can merge tokens/damage strings -> AST-owned trivia and literal-marker, whitespace, continuation and Unicode causal tests.
- Sidecar ordinals can drift after graph admission -> explicit spans/identities and validation; malformed or filtered facts cannot inherit another binding.
- Lexical rules differ across grammars -> provider-owned rules, registry-derived positive/negative controls, parameter/local/closure/block probes and honest fallback abstention.
- Re-export/default chains can over-resolve or cycle -> explicit export evidence, bounded visited work, alias/dependency ownership and ambiguity negatives.
- Summary and detailed graph can disagree -> one canonical incoming-call authority, tested through both public adapters.
- Recovered source evidence can amplify refresh cost -> affected-only per-file caching, exact hash checks, accounting and sparse/dense/full/upgrade measurements with #708.

## Migration Plan

Advance existing parser/extraction and semantic fingerprints; ordinary refresh regenerates stale derived facts even with unchanged source. Preserve schema, identity, approved purposes, root isolation and the previous publication. Verify installed 0.5.0 and prior development-contract repair, convergence with a fresh full graph, and second-refresh no-op. Revalidate all recovered bytes against the staged source inventory. Exercise failed/cancelled recovery and publication rollback plus compatible prior-runtime reopen behavior without authored-state migration or reset.

The contract bump invalidates all old-contract file projections: first compatible-index repair can require a full bounded source parse. Old-contract canonical rows cannot be carried forward as if current. Only subsequent current-contract operations may reuse untouched accepted owners outside the affected projection.

## Architecture Disposition

The existing parser, runtime publication and SQLite owners remain. The source-evidence and caller-query flow is clarified in docs/static-binding-resolution.md#static-binding-evidence-and-publication, including failure termination and read-only query ownership. Reconcile that view against final code.

## Dependencies / Cross-Issue Impact

#731 has no direct implementation blocker on accepted main containing #722. It directly belongs to and blocks #715 in v0.5.1-00. #708 may proceed independently but must consume combined accepted source, prove affected-only recovery costs and distinguish one-time contract reconciliation from current-contract warm admission. #715 runs permanent installed regressions on exact RC packages for all supported platforms. Retain #721 bounded navigation and #725's deferred zero-source-work target. Closed #722 history remains inert.

## Open Questions

None.

## Verification

Causal producer/normalizer and lexical-binding tests, including malformed syntax, comments/literals/continuations, nested/mixed/independent aliases, exact module spelling, default/re-export chains and cycles. Registry-derived local-call tests cover confirmed native languages plus supported positive and fallback/optional controls. Real CLI/MCP persisted module/caller and summary queries, host positions/trust, incremental/full and upgrade/no-op convergence, identity/purpose/root preservation, changed-source/fault/cancellation/rollback outcomes. Verify indexed bounded caller-query plans, dense/high-degree resource behavior and source recovery accounting. Run locked affected Rust/docs/security, source inventory, strict OpenSpec, scoped IssueOps and mandatory native/hosted gates; independent review covers all five acceptance outcomes and explicit untested limits.
