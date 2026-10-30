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

That strict-module predicate remains appropriate for import binding leaves, but it
must not be reused as a context-free filter for every local declaration or callee.
Native execution and parser negatives confirm that ordinary scripts lose legal
`eval`/`arguments` bindings; the pinned reserved-word set also contains contextual
and strict-only names. Separate BindingIdentifier and IdentifierReference validation
using the existing AST walk's script/module goal, strictness and Await/Yield grammar
context. Strict code can reference a valid outer non-strict `eval` binding. A function's
own strict directive affects its binding/formals, while async/generator grammar does
not itself make unrelated bindings strict. Recognize actual directive prologues,
not decoded escaped strings or matching text elsewhere. Keep lexical-declaration
early errors and invalid tolerant-parser leaves conservative.

Retain each admitted cooked declaration binding by its graph ordinal for later
resolution/export linking; do not revalidate it later without its original context.
Non-arrow functions/methods/generators own implicit `arguments` shadows, while arrows
inherit their enclosing binding. Preserve explicit local/parameter/write ownership
and do not replace unsupported runtime values with a same-name file declaration.
Use bounded context and ordinal state in the existing lexical walk, without
per-call ancestor scans, a grammar reimplementation or runtime execution analysis.
The parser and binder share one source-context owner for declaration names,
formals, bodies, computed method keys, class expressions and field/static
initializers. Invalid parameter/local/import leaves retain negative shadows so a
valid outer Script binding cannot become their guessed target. Import aliases
select the actual local leaf; type-only imports do not enter the value namespace.

The pinned grammars also misread legal Script `yield()` calls, and TypeScript
`await()`/`interface()` calls, including concise arrows in containers. Native Node
and the official TypeScript 5.9.3 Script compiler confirm the language-valid cases;
this is not compiler configuration or framework-transform emulation.

Do not reconstruct arrow/conditional grammar from recovered tokens. Traverse the
original native AST with the shared source contexts; error wrappers make only
Await/Yield context uncertain, preserving known module/strict restrictions. A
relaxed anonymous keyword must still have original native call/binding evidence:
complete adjacent formals/parenthesized-call syntax (including a bounded nested
keyword prefix) or an exact keyword-only native parameter binding error. Reject
intervening operands/stars and unknown recovery. Bound sibling evidence lookahead
by the existing AST-depth ceiling. Preserve strings, comments and ordinary types.

Lazily normalize only the final ASCII byte of an admitted token in one private
same-length buffer. The same pinned parser supplies the actual candidate AST,
which must be error-free. A second bounded context walk then proves every changed
byte corresponds to one exact identifier leaf valid under its actual source,
function-name, formal, body and class context; property names retain IdentifierName
rules. Compare the admitted leaf count with changed bytes in the exact program
span so merged, swallowed, opaque or unaccounted changes cannot gain authority.
Enforce actual arrow no-line-terminator header boundaries alongside grammar shape.

Restore a token proved reserved as an operator in the candidate context, then
allow at most one final native reparse and validation. Thus each affected program
has at most two additional parses, never a per-call parse or fixed-point loop.
Reject incomplete correspondence or a failed final proof and retain the original
tree/raw evidence. The native parser owns calls, arguments and surrounding syntax;
no synthetic call facts, argument parser or second grammar engine is introduced.

Reuse that grammar's parser and single included range. Clone at most once per
affected projection, not per token/region; original source supplies every name,
signature, hash and span. Final-byte writes do not splice or move suffixes as token
density rises. Prefilter only program bytes through bounded cooperative chunks;
keep source-size/depth/deadline/cancellation ownership across scanning, allocation,
parsing and validation. Measure all added AST/copy/parse work, including dense
keywords and region counts, before release acceptance.

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

Reuse the existing same-length JS/TS buffers and one native parser per grammar.
Parse each actual program through a single included source range, retaining
its tree until the shared collector has bound all roots. Both scanner variants
retain original offsets; separate parses prevent cross-region recovery and ASI
ambiguity, including the older TypeScript scanner without an included-range-start
ASI rule. Aggregate parse/traversal cancellation and limits across the file;
parsed bytes are the sum of admitted regions plus only the affected programs'
bounded contextual-keyword reparses (at most two), not the whole host per region.
Map sorted range endpoints in one pass. Remap the collected declarations,
imports and calls through explicit append-only host ordinals, then seal against the
original host bytes. Keep structural/fallback host provenance. A compatibility row
can stand for one captured import/call occurrence; repeated same-line occurrences
must survive independently. Coalesced declaration identities cannot narrow an
ambiguous native binding. Independent JS/TS projection import ordinals remain
independent. Correspondence failure discards binding proof and retains fallback
coverage rather than inventing an association. Bound merge work with indexed
identities and per-occurrence compatibility consumption, avoiding repeated scans
of growing same-name occurrence lists.

Preserve bounded per-region source-goal and lexical-owner evidence alongside each
of the existing same-length JS/TS projections, under the existing 64-region ceiling.
Do not create or reparse a whole-host copy per script. Retain at most 64 region
trees across the existing two buffers, and perform source sealing/hash work once
per projection and final host rather than once per region. A causal current-source HTML
fixture proves module locals currently leak into a classic script through the merged
root. HTML classic regions share the document's global lexical environment but each
region has its own directive prologue; each module region is strict and isolated
from sibling module locals. Modules may read the shared classic global environment;
classic scripts cannot read module locals. Keep original offsets, ordinals, dependency
facts, conservative export ambiguity and resource/cancellation behavior.

Static imports/re-exports cannot obtain module dependency or alias authority from
an actual Script program (HTML classic or explicit JavaScript CJS), or from a
static import nested outside a program root. Preserve raw occurrences and negative
local shadows, plus ordinary module, Vue/Svelte and TypeScript import-transform
positives. When TypeScript is explicitly supplied to the source API, CTS alone does not
prohibit ES import syntax; keep content-based module detection there. Automatic
CTS/MTS detection remains outside the current registered extension surface. Dynamic import/require
and unspecified compiler transforms do not gain or lose supported capabilities.

Use the declared host's known roles: Vue normal script has module scope and script
setup has its setup scope; Svelte module declarations are visible to instance code,
while instance locals cannot flow back into module code. Retain the corresponding
bounded parent relationship and admitted import ownership, including supported
compile-time import hoisting where established by the host contract. Do not infer
arbitrary framework transforms, execution, package-loader configuration or custom
host semantics. Unsupported or insufficient role evidence must not authorize a
cross-region target. Prove normal/setup and module/instance positives and negatives
before claiming their directional support; do not silently narrow admitted hosts.

Require unique role attributes and at most one of each known Vue/Svelte script
role, including scripts excluded from native parsing by source/language selection.
Svelte's documented module flag is valueless and its legacy context is exactly
module; compatible use of both retains support. Reject conflicting/duplicate role
attributes rather than choosing whichever grants module authority. See the
[compiler's script reader](https://github.com/sveltejs/svelte/blob/main/packages/svelte/src/compiler/phases/1-parse/read/script.js).
Keep raw graph facts for tolerant navigation, but propagate an incomplete host
batch to call/export authority instead of ignoring it. This also prevents a safe
prefix from becoming a complete lexical environment after an omitted-region limit
or malformed remainder. HTML multi-script programs and valid two-role components
retain their existing source evidence. The bounded mechanism is two role bits
plus existing incomplete flags, not a framework compiler or another scope registry.

Preserve every raw host export slot but admit module-target authority only for a
role that establishes it: Vue normal script, HTML module script and Svelte module
script named exports. Vue setup, Svelte instance and HTML classic export syntax
cannot establish an ES module target. Svelte's component default is compiler-owned;
even an explicit default source slot cannot identify it. Reconcile older generic
host-default positive fixtures using the real module-bearing roles, retaining raw
slots through existing unresolved/declaration markers so fallback cannot resurrect
an unsupported target. This corrects the accepted fixtures' source-role assumption
without adding component-constructor or framework-transform inference.

### Explicit files and inferred module aliases have different identities

Real CLI/MCP testing found that removing every terminal suffix merged an explicit
`./target.svelte` with `target.html` and native same-stem files. Relative imports,
configured `paths` substitutions and `baseUrl` all used this shared assumption.
The target projection also emitted basename and `src/`-stripped aliases, allowing
`src/caller.ts` importing `../target` to select an unrelated `src/target.js`.

Keep this repair at ECMAScript resolution-key ownership. Frame private scope text
as `file:` for explicit terminal suffixes and `module:` for extensionless lookup;
both namespaces use repository-exact paths. Source module scopes retain only the
exact extensionless stem and existing package-entry aliases. Explicit file keys
never gain basename, stripped-root, Rust separator or display-name aliases, and
preserve literal path bytes. Dots in directory names do not select a file lookup.
Unknown explicit suffixes retain exact identities for explicitly selected source
languages without adding automatic language detection or parser support.

Retain the supported conservative JavaScript-output candidates: `.js` may select
`.ts`, `.tsx`, `.d.ts`, `.js` or `.jsx`; `.jsx` may select `.tsx`, `.d.ts` or `.jsx`.
These are candidate identities, not compiler search-order emulation; competitors
remain ambiguous. `.mjs`, `.cjs`, direct TypeScript suffixes and embedded-host
suffixes remain distinct. This agrees with the relevant extension-substitution
families in the [TypeScript module reference](https://www.typescriptlang.org/docs/handbook/modules/reference#file-extension-substitution)
without adding unsupported CTS/MTS, arbitrary transforms or runtime module loading.

The existing key schema, configured-module admission, per-fact limits and cached
once-per-import expansion remain unchanged. Concrete helper functions and two
closed namespace constants avoid another resolver abstraction or filesystem probe.
Work is bounded by existing per-source key limits; fewer broad aliases offset the
small explicit-file candidate fan-out, with final source-bound costs still measured.
The public relative-path helper keeps its legacy result; internal ECMAScript lookup
uses exact normalized components. Rust/Python projection and normalization retain
their provider-owned rules. Matrix tests cover relative/configured/baseUrl module
and callable keys, registered suffixes, explicit unknown suffixes, entry aliases,
literal file names and wrong-directory/compound-stem negatives. Real CLI/MCP,
unchanged-native upgrade, full rebuild and no-op convergence are required proof.

### Binding ownership controls calls

Occurrence lookup must require the raw producer's native node-owner predicate as
well as byte extent. A program/wrapper can share the whole span of its sole
import or call. Claiming that occurrence again creates false competing bindings
and repeated call work, as confirmed by an import-only component block hoisted to
a preceding module callable. Reuse the existing registry-derived producer guards
for imports/re-exports and calls; do not deduplicate genuinely distinct competitors
or add public fact fields merely to repair a same-span wrapper collision.


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
or per-call timeline scan is admitted. Contract 14/14 repairs concise-arrow and
container keyword ambiguity in unchanged private13/13 indexes, including actual
candidate-context/operator validation and exact AST occurrence ownership. Both
extraction and semantic contracts advance because raw and resolved facts change.
Contract 13/13 repairs source-context and
host-role authority, contextual parser ambiguities, implicit/private owners and
invalid-binding shadows in unchanged private12/12 indexes. Actual native12 repair,
full convergence, authored state, no-op and failure/rollback proof remains required.
Contract 12/12 additionally repairs missing
synchronous/asynchronous generator declaration scopes, callable graph/export
identity and generator-local value ownership in unchanged private11/11 indexes.
All locked ECMAScript grammar variants use their existing AST-owned function
boundary; expressions, ordinary/async functions, arrows and starred methods retain
their established owners. This includes the earlier contract11 repair of function
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

### Shared module literal ownership before staging admission

Current-head security review confirms that a source-bearing ECMAScript re-export
clones its complete module literal into every independent export slot before
post-extraction retained-byte admission. The relation display limit does not bound
these source export facts. Preserve those independent slots and exact literals
with one statement-owned immutable module allocation shared by ImportReference
and its consumer clones. Charge shared module storage once in the owning source
retained-byte estimate; retain per-slot imported/local names and existing caps,
checkpoints and unsupported-syntax behavior. Reuse Arc and concrete constructors,
without a global interner, schema, parser or new dependency. Public graph and
semantic values do not change, so this allocation repair alone needs no extraction
or semantic invalidation bump. Verify that judgment against the finished delta.

The separate scanner claim of 200,000 named import leaves times 64 configured
scopes is not reachable through the current native source pipeline: valid source
imports project their module scopes once per relation, while graph-only fallback
imports are bounded by the existing display-target admission. Preserve that
producer/consumer counterevidence rather than changing the key contract on the
basis of the alert's multiplication. Reassess only if another reachable path is
causally established.

### Reopened native namespace environments

Authoritative lexical evidence must preserve a same-file namespace's identity across
separate declaration blocks. Current PR feedback causally targets C++ reopened
namespaces: a later block must see earlier declarations in that namespace without
seeing unrelated namespaces or later undeclared values. Reuse bounded semantic
environments, keep source-order visibility and ambiguity, and inspect nested, inline
and anonymous namespace forms plus actual analogous registry-owned containers. Do
not share arbitrary classes/blocks or infer cross-file/runtime namespace behavior.
Any changed binding facts require existing extraction/semantic fingerprint repair.

The confirmed analogous cases are explicitly partial C# class/struct/interface/
record environments and exported TypeScript namespace members. A concrete key of
parent-scope ordinal and grammar-owned child name avoids storing qualified prefixes
per declaration. Generic parameter identity and type family separate partial owners.
TypeScript keeps one private lexical block per part with an additional shared export
lookup layer; explicit export binding and subsequent writes use the shared owner,
while an exported closure retains its original private parent. Anonymous C++ children
are linked once per logical namespace; lookup borrows candidates with cooperative
checks rather than copying every name into ancestor maps. Distinct candidates
abstain and future-only declarations cannot hide already visible outer targets.
The existing 16,384 scope/transition and AST depth bounds remain authoritative.
No schema, provider, runtime model or global interner is introduced. C# identifier
equality uses the already locked unicode-properties general-category tables through
an explicit direct dependency; no dependency version changes. A small shared
fixed-width Unicode escape decoder serves the C#/C++ grammar-owned binding and
reference names, while each language retains its own identifier rules. C# removes
verbatim prefixes and formatting characters; C++ preserves ordinary raw names and
admits non-basic universal character names. Neither applies case folding or Unicode
normalization, and graph display text remains unchanged.
Extraction and semantic contracts advance from 14 to 15; actual unchanged native
14-to-15 repair, full convergence, authored-state preservation and no-op are required.

The actual namespace CLI/MCP proof additionally reproduced stable0.5.0 C# namespace
and Java module names containing complete bodies. Prefer their AST name field in
the existing shared declaration producer, refusing malformed/missing required
names. Preserve Java/Go/Kotlin package prefix fallback (those nodes have no body
or name field) and original identifier display spelling. This repairs package,
parent, contains-edge and selector identity at the source owner under the same
unpublished extraction15 invalidation. Partial generic identity components charge
the existing collector-wide lexical budget before allocation; many individually
bounded keys cannot exceed the shared retained-name ceiling. Causal aggregate and
single overflow cases withdraw all binding/export authority while preserving raw
navigation, and a near-limit positive retains exact calls.


## Instance field evaluation ownership

ECMAScript field syntax has two evaluation phases. Only an ordinary instance-field
value executes at later construction; its existing initializer environment receives
the same bounded deferred enclosing lookup used by function bodies. Static fields
and blocks remain immediate. Computed names, decorators and type syntax visit the
enclosing class environment, and direct construction of a class expression keeps
its value immediate through the existing transparent-wrapper invocation check.
Nested callbacks retain their own function boundary. This repairs the shared
JS/TS/TSX and admitted host collector without runtime constructor-flow inference,
new providers, retained state or parsing. Competing later writes remain unresolved.
The existing architecture diagram is unchanged. Extraction/semantic contract16
invalidates unchanged15 indexes; native prior-contract repair, canonical CLI/MCP
summary agreement, no-op, state preservation and affected costs must be repeated.

Writes crossing an initializer with a nonzero deferred visibility floor use the
existing captured-mutation owner, just like writes crossing a function boundary.
They must not be inserted at class-declaration source time and then be overwritten
by a later declaration transition. This keeps later callers conservative after
possible construction without changing the function flag or var ownership. The
within-field write-then-call control already abstains through the stable-tail
guard; the causal defect is a later enclosing caller or later class-field call.


## Shared forwarding query modules

The source reference already owns an immutable shared module, but converting it
back to an owned String in BindingQuery duplicated the full literal before cache
lookup for every alias call. Preserve the same Arc through query construction and
frame clones. Shared module bytes remain accounted by source facts/source cache;
query accounting retains its own text and node/handle headers. Value Eq/Ord remains
canonical across distinct allocations. This changes allocation and work only, not
facts, keys, schemas or semantic outcomes, so DER16/SEM16 remain unchanged. Existing
resource/cancellation/cycle tests and actual long-module CLI/MCP controls protect
the consumer boundary; no module-length rejection or capability removal is added.
