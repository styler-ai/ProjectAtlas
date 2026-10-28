## Purpose

Preserve reliable static dependencies and truthful caller navigation across registered source semantic providers and native local resolver paths, using syntax and lexical binding evidence rather than matching display names.

## ADDED Requirements

### Requirement: Comments preserve import meaning
Supported static import bindings SHALL retain the same dependencies and alias identity with or without syntactically valid comments and formatting. Comment-like text inside literals SHALL retain its exact meaning. Invalid syntax SHALL NOT fabricate bindings.

#### Scenario: Comment-bearing supported imports
- **WHEN** valid Rust, Python, JavaScript, TypeScript or TSX static import lists contain comments between bindings, including an alias and a supported sibling
- **THEN** persisted dependencies and imported callers match the equivalent comment-free declaration

#### Scenario: Literal markers and invalid syntax
- **WHEN** an import contains a literal with comment markers or malformed binding syntax
- **THEN** literal contents are preserved and no target is inferred by deleting literal content or invalid syntax

#### Scenario: Exact module literal whitespace
- **WHEN** an ECMAScript import names an existing file through a module literal containing repeated spaces, such as `"./dep  space.js"`
- **THEN** the dependency resolves to that exact file spelling and its imported alias resolves the correct caller without collapsing literal whitespace

#### Scenario: Escaped static module literal
- **WHEN** an ECMAScript module specifier encodes its path using escaped quotes, backslashes, hexadecimal or Unicode escapes, or literal line continuation
- **THEN** dependency and caller resolution use its strict static string value while source evidence retains its spelling; malformed escapes and non-lossless UTF-8 values do not fabricate targets

#### Scenario: Python explicit line continuation
- **WHEN** a valid Python static import uses backslash line continuation between an imported name and its `as` alias
- **THEN** the module dependency, alias identity and imported caller match the equivalent single-line declaration

### Requirement: Grouped static bindings remain independent
Rust nested use trees and ECMAScript mixed default/named or default/namespace imports SHALL preserve independently supported bindings and module dependencies. Symbol targets SHALL require explicit static evidence; unsupported, ambiguous or dynamic targets SHALL remain unresolved without discarding valid siblings.

#### Scenario: Rust nested aliases
- **WHEN** a nested Rust use tree imports aliased symbols from different module paths
- **THEN** each supported binding resolves its own dependency and caller target as its equivalent flat import

#### Scenario: ECMAScript mixed imports
- **WHEN** a module is imported through a mixed default/named or default/namespace declaration
- **THEN** the module dependency and supported named or namespace bindings survive and the default binding resolves only with sufficient unambiguous static export evidence

#### Scenario: Unsupported default expression
- **WHEN** a mixed import refers to an unsupported runtime-produced default export and a supported named export
- **THEN** the default symbol target remains unresolved and the named export's imported callers remain available

#### Scenario: Competing explicit default slots
- **WHEN** a module contains multiple explicit default export slots, including declaration, expression, local export clause, export-from or namespace-export forms
- **THEN** no default declaration is published as a unique target, filtering an omitted declaration cannot narrow the remaining slot to an exact target, and independently valid named bindings retain their proof

#### Scenario: Tolerant grammar admits an invalid binding
- **WHEN** a TypeScript or ECMAScript import has an invalid local binding, including a reserved word without an ERROR node, an escaped reserved word, strict eval/arguments, or an invalid identifier character
- **THEN** it creates no binding proof or exact caller for that leaf; independently proven static module dependencies and supported sibling bindings survive

#### Scenario: Binding names and imported export names have different roles
- **WHEN** a valid local alias imports a reserved export name, or a valid local binding uses a global-object name, Unicode identifier or Unicode escape
- **THEN** the alias retains its language-valid cooked identifier while raw source evidence stays exact, without treating imported export names as local reserved bindings

### Requirement: Registered adapters preserve the same contract
The import contract SHALL apply to admitted embedded ECMAScript regions through their registered semantic owner and parser trust. Cargo structured dependencies and languages without a registered source semantic provider SHALL retain their existing support boundaries.

#### Scenario: Embedded scripts
- **WHEN** an admitted JavaScript or TypeScript region in Vue, Svelte or HTML contains a supported comment-bearing or mixed import
- **THEN** outbound dependencies and supported imported callers retain host source positions and the same semantics as the standalone equivalent

#### Scenario: Unsupported or excluded regions
- **WHEN** host markup contains an unadmitted script region or a parser-only language lacks a source semantic provider
- **THEN** normalization does not infer new project-wide semantic support or upgrade fallback facts to trusted evidence

#### Scenario: Registry-declared host admission
- **WHEN** a host has the existing registered embedded-script capability, including HTML/HTM with a structural-only direct parser
- **THEN** runtime symbol admission reaches the existing embedded extraction path, honors explicit language selection and preserves plain-HTML structural summaries without adding providers to CSS/XML or unsupported script regions

#### Scenario: Distinct occurrences and coalesced declarations
- **WHEN** admitted embedded calls share a flat display identity or multiple native declarations coalesce to one host symbol row
- **THEN** distinct captured call/import occurrences retain independent binding evidence, while every ambiguous declaration correspondence loses its proof rather than selecting one declaration

### Requirement: Captured occurrence identity survives projection
Native and admitted-host call/import facts with exact captured source evidence SHALL retain distinct persisted occurrence ranges even when they share a physical line, target, display name or context length. Logical relationships SHALL remain deduplicated independently of their individual occurrences. Coordinates SHALL retain original source lines and Unicode-scalar columns; missing source evidence SHALL preserve existing fallback trust rather than invent an exact span.

#### Scenario: Repeated same-line source sites
- **WHEN** multiple captured calls or imports on one physical line resolve to one canonical target
- **THEN** one logical relationship retains every distinct captured occurrence, including equal-length aliases and identical repeated callees

#### Scenario: Original source coordinates and bounded work
- **WHEN** captured sites include Unicode, CRLF, multiline imports, callbacks or admitted embedded scripts
- **THEN** occurrence ranges follow the existing original-byte mapping and scalar-column convention in bounded per-source work, survive ordinal remapping and incremental/full repair, and do not rescan each source prefix per occurrence

### Requirement: Existing indexes repair atomically
Refreshing an existing compatible index SHALL repair stale import facts and projections even when source bytes are unchanged. Identity, approved purposes, root isolation and the last valid publication SHALL survive failed or cancelled repair. A subsequent unchanged refresh SHALL avoid redundant publication.

#### Scenario: Upgrade and convergence
- **WHEN** an existing index produced before the normalization contract is refreshed
- **THEN** its dependencies and imported callers converge with a fresh full index, identity and approved purposes remain unchanged, and a second unchanged refresh does not publish again

#### Scenario: Failure and compatible rollback
- **WHEN** repair fails or is cancelled, or the previously supported runtime reopens preserved state
- **THEN** no partial graph or cross-root target is exposed and existing compatibility refusal or rollback behavior preserves the last valid index and authored state

### Requirement: Import processing remains bounded
Normalization SHALL reuse bounded indexing work and preserve cancellation, parser trust and literal/source provenance. Equivalent comment or grouping handling SHALL NOT require reparsing each import or weaken resource limits.

#### Scenario: Dense imports and excessive input
- **WHEN** dense imports, large comments or deeply grouped input reach existing work or size limits
- **THEN** processing terminates within the existing cooperative bounds, reports incomplete evidence honestly and preserves the last valid publication on refusal

#### Scenario: Omitted binding or export dependency evidence
- **WHEN** a native declaration, import or source-bearing re-export is omitted at a capture resource ceiling
- **THEN** retained facts cannot establish exact call or exported-target authority from the incomplete environment, while raw navigation and module dependency evidence remains available

#### Scenario: Forwarding workspaces share the existing staging boundary
- **WHEN** Rust or ECMAScript forwarding copies dependency causes, grows lookup indexes, enters a recursive frame or loads additional canonical lookup keys
- **THEN** each new retained dependency and query copy is admitted before growth under the existing operation ceiling, unique dependency sets reject excess rows before insertion, duplicate keys do not consume another reservation, and failures/cancellation release temporary ownership without erasing completed caches
- **AND** cycles retain their exact file causes without leaking uncached-result charges, successful caches account completed owners once, and ordinary Rust/module/export navigation remains available

### Requirement: Call resolution respects lexical bindings
Resolved callers SHALL require binding evidence valid at the call site. A local import SHALL NOT create a binding outside its language-defined scope. Parameters and local bindings SHALL shadow imported or same-name file declarations only in the language-defined namespace they occupy. Unknown dynamic targets SHALL remain unresolved rather than become false exact links. Detailed graph navigation and summary caller output SHALL agree.

#### Scenario: Deferred enclosing bindings and immediate evaluation
- **WHEN** a supported Python or ECMAScript deferred body references a unique later enclosing declaration or import
- **THEN** its canonical call and summary caller retain that target, while immediate expressions, direct IIFEs, definition headers and a generator's first iterable retain source order and competing future writes remain unresolved

#### Scenario: ECMAScript parameter and body environments retain their owners
- **WHEN** body function declarations are reassigned by var initializers or iteration, or parameter initializers reference names also declared in the body
- **THEN** body function declarations share the body var environment, parameter evaluation retains parameter bindings without seeing body locals, and genuine nested block declarations remain distinct
- **AND** static class blocks and admitted TypeScript namespaces retain their own var environments, while var declarations reserve their owner separately from lexical writes through a valid catch binding, including destructuring and iteration

#### Scenario: Bare ECMAScript var redeclarations preserve values
- **WHEN** a bare var declaration repeats an existing function, parameter or initialized variable name, including a function-scoped block or ordinary for initializer
- **THEN** it reserves the function-local name without replacing the existing value; canonical callers and explicit exported aliases retain proven callable targets
- **AND** an otherwise uninitialized local still shadows outer bindings, while actual initializers, assignments and for-in/of writes retain their invalidation behavior in standalone and admitted embedded scripts

#### Scenario: Malformed initializer recovery cannot invent callable identity
- **WHEN** a malformed initializer contains a recovery error or leaves a recovery error immediately following its declarator, including a following function absorbed as an initializer
- **THEN** it cannot authorize a no-write reservation or direct callable declaration, recovered calls retain honest unresolved evidence, and any surviving explicit export slot remains unresolved
- **AND** independently valid comma-delimited bare declarations before or after another malformed declarator preserve their existing values

#### Scenario: Bounded lexical state with fact-free syntax
- **WHEN** parallel source files exhaust lexical scope/value capacity or native traversal depth without exhausting symbol or relation caps
- **THEN** retention remains bounded, all binding/export authority is withdrawn, raw admitted evidence and proven prefix ownership remain navigable, and normal source reduction and refresh repair exact callers without losing project identity


#### Scenario: Scoped imports and independent aliases
- **WHEN** separate functions import the same alias from different modules or a sibling function calls an alias imported only inside another function
- **THEN** each in-scope call retains its independent binding and the out-of-scope call does not resolve through the unrelated import

#### Scenario: Python directives select actual binding owners
- **WHEN** Python uses global or nonlocal declarations with assignment, deletion, iteration, context/exception/match captures, imports or nested declarations
- **THEN** lookup follows the declared module or enclosing function owner, reads before a write retain valid targets, and later or ambiguously timed captured writes cannot fabricate callers
- **AND** named expressions use their grammar-owned name field and ordinary function-local reservation remains intact, including comprehension bindings and class namespace exclusions

#### Scenario: Root declarations and namespace bindings
- **WHEN** Rust imports a root function through `use crate::run as launch` or a namespace through an anchored use path
- **THEN** static declaration and namespace evidence determine the dependency and caller targets without classifying the binding solely from its path length

#### Scenario: Parameters shadow imports or declarations
- **WHEN** a function parameter or local binding shadows an imported or file-level function name in an affected supported language, including Rust, Python, ECMAScript, Go, C, C++, Java, C# and Kotlin
- **THEN** the call does not resolve to the shadowed function merely because its name matches, and that function's caller summary excludes the false caller

#### Scenario: Rust value and module namespaces
- **WHEN** Rust value parameters share names with an imported module or a standard-toolchain root used in a qualified call
- **THEN** those values do not hide module-qualified calls; local module/type declarations and type parameters still block incompatible module fallback, while value parameters continue to shadow direct imported function calls

#### Scenario: Qualified namespace members retain their owning path
- **WHEN** a Rust imported namespace has both a root function and a nested module function with the same leaf name
- **THEN** root and nested qualified calls select their respective declarations without dropping intermediate path components or exporting a child declaration in its parent's root scope

#### Scenario: Type and value lookup preserve callable identity
- **WHEN** C or Objective-C struct tags or TypeScript type aliases share a name with a function
- **THEN** the type-only declaration does not replace the callable value target or its canonical summary caller

#### Scenario: C++ constructor and function visibility
- **WHEN** a class or struct and a function share a name, or a call precedes a competing declaration
- **THEN** language-visible ordinary values take precedence, constructor fallback requires visible unambiguous type evidence, partially visible overloads or ambiguous correspondence remain unresolved, and wholly visible overloads remain ambiguous

#### Scenario: Java member syntax does not become a class call
- **WHEN** Java invokes a qualified method such as `Service.run()`, including a same-name field receiver or overloaded methods without member-target proof
- **THEN** extraction retains the full method callee, neither detailed resolution nor summary callers fabricate a call to the `Service` class, and existing unqualified method navigation remains available

#### Scenario: Member names retain their language-defined namespace
- **WHEN** Rust impl or trait items, Go receiver methods, ECMAScript class or object methods, or Objective-C selectors share an ordinary function's name
- **THEN** bare calls do not acquire those member declarations as lexical values, receiver parameters retain their local shadows, and supported implicit member lookup in Java, C++, C# and Kotlin remains available

#### Scenario: Declaration correspondence uses existing identity evidence
- **WHEN** declarations share a name, node kind and source-line range but have distinct emitted parents or signatures
- **THEN** their actual AST identities select the corresponding graph ordinals without per-call candidate rescans, and indistinguishable metadata remains ambiguous

### Requirement: Static re-export aliases retain explicit evidence
Supported static Rust and ECMAScript re-export aliases SHALL preserve dependency and downstream imported-binding identity. Resolution SHALL abstain for ambiguous or unsupported runtime exports and terminate conservatively within existing work bounds for cyclic chains.

#### Scenario: Static aliased re-export
- **WHEN** a module explicitly re-exports a static function under another name and a consumer imports and calls that name
- **THEN** the dependency and imported caller retain the explicit alias chain to the unambiguous source declaration

#### Scenario: Existing callable variable declarations retain forwarding identity
- **WHEN** an existing source function declaration is produced from a named arrow or function initializer and is imported directly or through an explicit re-export alias
- **THEN** the actual declared function retains its target in both routes, while ordinary unproved values, destructuring property keys and arbitrary default expressions do not acquire callable identity through metadata fallback

#### Scenario: Host rescue rows do not absorb following statements
- **WHEN** an admitted host script has an import followed by another statement on the same physical line, including multiline or comment-bearing import syntax
- **THEN** the whole-line rescue row is reconciled only with the first import proven by its exact source byte span, later import occurrences stay independent, exact AST-owned occurrences retain native parser provenance without upgrading graph-level host coverage, and unprojected regions retain their existing fallback provenance

#### Scenario: Export namespaces and independent source leaves
- **WHEN** TypeScript type-only export slots share a name with a runtime export, or one local/export-from specifier has malformed syntax beside an independently valid sibling
- **THEN** type-only slots cannot become callable targets or suppress the runtime namespace, and the independently proven runtime sibling retains its source declaration or forwarding evidence

#### Scenario: Exported type-only declarations cannot become callers' targets
- **WHEN** a consumer calls a name imported from an exported Rust type alias or trait, or an ECMAScript type alias or interface
- **THEN** exported-symbol fallback does not fabricate a callable target, while actual supported function and constructor declarations retain their existing static eligibility

#### Scenario: Ambiguous or cyclic re-export
- **WHEN** a re-export chain has ambiguous targets or reaches a cycle without sufficient declaration evidence
- **THEN** resolution terminates without fabricating a target or dropping independently supported bindings

#### Scenario: Cargo root and source namespace ownership
- **WHEN** supported Rust imports, module calls or explicit re-exports cross actual Cargo roots, inline modules or default module files, including a manifest-only custom-root switch
- **THEN** dependency and caller targets follow unique current manifest and AST ownership, retain missing and intermediate invalidation causes, preserve existing supported inherent-method and constructor workflows, and abstain for unsupported or ambiguous ownership

#### Scenario: Toolchain names follow actual bindings
- **WHEN** a local Rust module or imported alias shares a toolchain-root spelling, or an absolute path has edition-dependent ownership
- **THEN** static external identity follows source binding and owning target edition rather than the alias spelling, preserves genuine standard-toolchain positives, and abstains when absolute ownership is unproved
