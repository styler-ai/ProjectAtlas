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

#### Scenario: Explicit suffixes and repository paths identify distinct targets
- **WHEN** relative imports, configured aliases or baseUrl refer to an explicit source-file suffix among same-stem native or admitted-host files
- **THEN** dependency and callable keys preserve the exact file identity, including unknown suffixes under explicit language selection, without matching a different host/native suffix or a stripped-root or basename alias in another directory
- **AND** supported JavaScript-output source candidates and extensionless package-entry aliases remain available with conservative ambiguity, while Rust/Python rules and automatic language support remain unchanged

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

#### Scenario: Re-export module literals do not multiply by binding count
- **WHEN** one supported source-bearing re-export contains many independent slots and a long static module literal
- **THEN** extraction and consumer clones share immutable module storage before staging admission, retained-byte accounting charges that allocation once, and exact names, literals, slot independence, cancellation and existing resource ceilings remain intact

#### Scenario: Omitted binding or export dependency evidence
- **WHEN** a native declaration, import or source-bearing re-export is omitted at a capture resource ceiling
- **THEN** retained facts cannot establish exact call or exported-target authority from the incomplete environment, while raw navigation and module dependency evidence remains available

#### Scenario: Forwarding workspaces share the existing staging boundary
- **WHEN** Rust or ECMAScript forwarding copies dependency causes, grows lookup indexes, enters a recursive frame or loads additional canonical lookup keys
- **THEN** each new retained dependency and query copy is admitted before growth under the existing operation ceiling, unique dependency sets reject excess rows before insertion, duplicate keys do not consume another reservation, and failures/cancellation release temporary ownership without erasing completed caches
- **AND** cycles retain their exact file causes without leaking uncached-result charges, successful caches account completed owners once, and ordinary Rust/module/export navigation remains available

### Requirement: Call resolution respects lexical bindings
Resolved callers SHALL require binding evidence valid at the call site. A local import SHALL NOT create a binding outside its language-defined scope. Parameters and local bindings SHALL shadow imported or same-name file declarations only in the language-defined namespace they occupy. Unknown dynamic targets SHALL remain unresolved rather than become false exact links. Detailed graph navigation and summary caller output SHALL agree.

#### Scenario: ECMAScript names retain their actual syntax roles and context
- **WHEN** a supported script declares or references names such as eval, arguments, await, yield or strict-only contextual words
- **THEN** declarations and identifier references use their own language-valid script/module, strict and async/generator grammar contexts, preserving legal non-strict bindings and strict references to legal outer values without admitting invalid declarations
- **AND** strict import bindings remain strict, actual directive prologues and classes retain their rules, escaped directive text does not invent strict mode, and declaration/export linking retains the original admitted binding evidence

#### Scenario: Sole-statement wrappers do not claim the statement occurrence
- **WHEN** a native program or transparent wrapper has the same byte extent as its sole import or call
- **THEN** lexical occurrence matching also requires the raw producer's node owner, so a hoisted import is not bound twice and a call is not claimed again by wrappers
- **AND** distinct real import competitors and repeated raw call occurrences retain their existing ambiguity and identity

#### Scenario: Implicit arguments ownership does not become an outer caller
- **WHEN** an ordinary, async, generator or method body references arguments while an outer declaration has the same name, or an arrow inherits that reference
- **THEN** non-arrow callable arguments ownership prevents a false outer target, arrows inherit the correct enclosing owner, and valid top-level or explicitly established local bindings retain their own callers

#### Scenario: Contextual keyword calls survive the pinned grammar ambiguity
- **WHEN** a language-valid Script uses yield as an identifier, or the pinned TypeScript grammar recovers an await/interface call through its closed keyword/parentheses/formals shape
- **THEN** a bounded same-length private normalization and native reparse with actual candidate-context and complete changed-token validation preserve the ordinary raw calls, nested arguments and lexical targets with original source spelling, positions and identity
- **AND** actual interfaces, strings/comments, module/async/generator restrictions and arbitrary malformed recovery gain no fabricated call authority; unchanged predecessor indexes repair through the updated extraction and semantic contracts

#### Scenario: HTML script goals and lexical roots remain distinct
- **WHEN** admitted HTML contains classic and module script regions, including multiple regions of the same grammar or a strict directive in only one classic script
- **THEN** classic regions retain their shared global bindings with region-local strictness, each module retains its own strict lexical root, and module locals cannot create callers from classic or sibling module regions
- **AND** module code may retain callers to proven classic globals; isolation does not remove that supported outward lookup
- **AND** original host coordinates, conservative exported-target ambiguity, the existing projection count, source-work bounds and failure behavior remain intact

#### Scenario: Static imports require a supported module source position
- **WHEN** a classic HTML script, explicit JavaScript CJS source or nested non-program position contains a static import
- **THEN** raw occurrences remain visible but invalid module/alias authority cannot create dependencies or callers, including a false fallback to an outer namesake
- **AND** valid module script, component and TypeScript import-transform workflows retain their supported targets; explicitly selected TypeScript does not inherit JavaScript CJS restrictions; automatic CTS detection remains outside the registered surface

#### Scenario: Declared component script roles retain directional visibility
- **WHEN** admitted Vue normal/setup scripts or Svelte module/instance scripts share a source file
- **THEN** their known module and instance/setup ownership, supported import ownership and parent visibility retain legal callers without leaking instance/setup locals back into module code
- **AND** unsupported custom transformations or insufficient role evidence cannot fabricate cross-region targets or gain another semantic provider

#### Scenario: Host raw exports do not imply module-target authority
- **WHEN** admitted HTML classic, Vue setup or Svelte instance script contains export syntax, or a Svelte script contains an explicit default slot
- **THEN** raw export occurrences remain available but cannot authorize an ES module target or resurrect one through declaration fallback
- **AND** Vue normal-script and HTML module-script static exports and Svelte module-script named exports retain their supported targets, while a compiler-generated component default remains unproved

#### Scenario: Ambiguous or incomplete host programs cannot authorize exact targets
- **WHEN** Vue/Svelte script role attributes conflict or duplicate, a component repeats a script role, or host reconciliation stops at a malformed remainder or region ceiling
- **THEN** retained raw navigation remains available but the incomplete host environment cannot authorize exact call/export targets
- **AND** valid unique component role pairs, compatible Svelte module markers and HTML multi-script behavior retain their positive scope/export evidence

#### Scenario: Deferred enclosing bindings and immediate evaluation
- **WHEN** a supported Python or ECMAScript deferred body references a unique later enclosing declaration or import
- **THEN** its canonical call and summary caller retain that target, while immediate expressions, direct IIFEs, definition headers and a generator's first iterable retain source order and competing future writes remain unresolved

#### Scenario: ECMAScript parameter and body environments retain their owners
- **WHEN** body function declarations are reassigned by var initializers or iteration, or parameter initializers reference names also declared in the body
- **THEN** body function declarations share the body var environment, parameter evaluation retains parameter bindings without seeing body locals, and genuine nested block declarations remain distinct
- **AND** static class blocks and admitted TypeScript namespaces retain their own var environments, while var declarations reserve their owner separately from lexical writes through a valid catch binding, including destructuring and iteration

#### Scenario: Generator declarations retain function and declaration ownership
- **WHEN** supported JavaScript, TypeScript, TSX or admitted script hosts contain synchronous or asynchronous generator declarations
- **THEN** their parameters and body vars retain function-local ownership without shadowing unrelated outer calls, parameter defaults cannot see body locals, and the generator declaration retains its graph, static export and canonical caller identity
- **AND** callable local initializers do not become outer callable declarations, while generator expressions, starred methods, arrows and ordinary functions preserve their existing ownership

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

#### Scenario: Reopened containers preserve their logical member owner
- **WHEN** supported same-file C++ namespaces, including nested shorthand and anonymous namespaces, or explicitly partial C# class, struct, interface or record declarations contribute separate blocks to the same logical owner
- **THEN** proven unqualified calls retain that owner's visible declarations without merging unrelated parents, ordinary types or different generic identities
- **AND** anonymous namespace members remain visible from their enclosing namespace while source order, local shadows, constructor/value precedence and competing candidates preserve conservative authority; cross-file merging, arbitrary using-directive expansion, inline namespace promotion and qualified C++ dispatch remain outside this bounded contract

#### Scenario: Instance field values retain deferred outer bindings
- **WHEN** JavaScript, TypeScript, TSX or an admitted script host initializes an instance field from a unique later enclosing callable
- **THEN** only its value uses deferred lookup; static field values, static blocks, computed member names, decorators and class headers retain immediate source order, direct construction of a class expression retains immediate field evaluation, competing writes remain conservative, and graph and caller summaries agree after unchanged-index repair

#### Scenario: Container names exclude declaration bodies
- **WHEN** native C# namespace or Java module declarations have a body
- **THEN** their grammar-owned name field determines package, parent and selector identity without incorporating that body or inventing a name from a missing/malformed required field
- **AND** existing body-free Java, Go and Kotlin package fallback remains compatible

#### Scenario: Partial type identities share the lexical retention budget
- **WHEN** one or several partial type identities exhaust the existing retained lexical-name limit through generic parameter components
- **THEN** admission stops before retaining each excess component, all binding/export authority is withdrawn and raw navigation survives
- **AND** a near-limit valid source retains exact callable evidence

#### Scenario: TypeScript namespace exports preserve private lexical closures
- **WHEN** same-file TypeScript namespace parts export callable declarations or values, including nested exported namespaces
- **THEN** exported members are shared across matching parts, their private declarations remain local to each original part and exported closures retain that private lexical parent
- **AND** writes invalidate the actual shared exported owner without leaking non-exported names or combining unrelated namespace parents

#### Scenario: Java member syntax does not become a class call
- **WHEN** Java invokes a qualified method such as `Service.run()`, including a same-name field receiver or overloaded methods without member-target proof
- **THEN** extraction retains the full method callee, neither detailed resolution nor summary callers fabricate a call to the `Service` class, and existing unqualified method navigation remains available

#### Scenario: Java member bodies retain their own method environments
- **WHEN** Java class, interface, enum, record or anonymous member bodies declare homonymous methods
- **THEN** each unqualified call uses its own structural member environment without leaking sibling methods, while named declaration identity, normal class behavior and existing resource bounds remain compatible; no inheritance or generated-member target inference is introduced

#### Scenario: Native member containers isolate ordinary names
- **WHEN** C++ union, Kotlin class/object/companion/enum-entry or Zig enum/union/opaque bodies contain members sharing a file-level function name
- **THEN** the members remain in their owning body and do not compete with an outer free call; C++ union methods and prototypes retain method classification

#### Scenario: Kotlin construction and member scopes remain distinct
- **WHEN** Kotlin primary constructor parameters or classifier members share a callable name used in defaults, delegation, property initialization, initializer blocks or member callables
- **THEN** headers do not see their own body members, plain constructor parameters bind construction-time expressions and their closures, property parameters also bind members, and ordinary member/accessor bodies do not inherit plain constructor parameters
- **AND** secondary-constructor and setter parameters retain their own local shadowing, while lexical scope/name caps still withdraw all binding authority on exhaustion

#### Scenario: Kotlin nested classifiers preserve allowed captures
- **WHEN** a non-inner member class or named object has an enclosing class member sharing a file-level callable name
- **THEN** lookup excludes the unavailable enclosing instance member, while explicit inner classes, object literals, singleton members and enclosing function locals retain their admitted lexical captures; declaration-scope property shadows apply independently of source order

#### Scenario: C++ condition declarations remain statement-local
- **WHEN** a while condition declares a callable value sharing an outer function name
- **THEN** the condition binding shadows only within the loop, the subsequent free call resolves normally, and existing if/switch/for initializer scope behavior remains compatible

#### Scenario: Native accessors retain implicit and declared parameters
- **WHEN** Kotlin accessors reference their backing field or C# set/init/add/remove accessors invoke their implicit value parameter, or C# indexers, local/anonymous functions and operator or conversion-operator declarations use declared callable parameters
- **THEN** the actual parameter/implicit value blocks an unrelated same-name method, getter contexts without that parameter keep ordinary callers, calls to local functions select their emitted local-function declaration when its canonical selector identifies one logical owner, and independently valid outer calls remain available

#### Scenario: C# primary constructor precedence stays phase-correct
- **WHEN** class/struct primary parameters share a name with a same-type member
- **THEN** same-type members win in ordinary member bodies, primary parameters win in instance field/property/event initializers and base arguments; static initializers keep ordinary member lookup, and all partial parts share the correct views regardless of source order
- **AND** positional record parameters conservatively block unrelated outer targets without inventing generated property symbols, while explicit field/property/event value names retain declaration-scope shadowing

#### Scenario: Native declarations retain real type and local owners
- **WHEN** C++ unions, Java/C# records including C# record structs, or C# local functions own callable declarations
- **THEN** emitted symbols retain the real source type/local parent and correct existing symbol family, distinguishing homonymous members of different qualified source types while remaining conservative for local owners whose canonical selectors still coincide
- **AND** Java record component names and enum values/valueOf names without emitted method evidence block unrelated outer targets, explicit record accessors and disjoint-arity source overloads retain source targets, same-arity generated-method competition stays conservative, and no generated method/property entity is invented

#### Scenario: Rust associated constants do not occupy ordinary lexical value scope
- **WHEN** an impl or trait-associated constant has the same name as a free function used unqualified in a method
- **THEN** the associated constant requires qualification and does not shadow the free call; module/function/block constants, local imports and const parameters retain their ordinary lexical blocking behavior

#### Scenario: Member names retain their language-defined namespace
- **WHEN** Rust impl or trait items, Go receiver methods, ECMAScript class or object methods, or Objective-C selectors or C# explicit interface implementations share an ordinary function's name
- **THEN** bare calls do not acquire those member declarations as lexical values, receiver parameters retain their local shadows, and supported implicit member lookup in Java, C++, C# and Kotlin remains available

#### Scenario: Declaration correspondence uses existing identity evidence
- **WHEN** declarations share a name, node kind and source-line range but have distinct emitted parents or signatures
- **THEN** their actual AST identities select the corresponding graph ordinals without per-call candidate rescans, and indistinguishable metadata remains ambiguous

#### Scenario: Distinct lexical declarations share one canonical selector
- **WHEN** declarations in distinct source lexical owners have the same canonical file, name, kind, qualified parent and signature
- **THEN** calls to those declarations and calls from colliding caller owners remain unresolved, raw source declarations remain navigable, and export metadata cannot re-admit an unsafe exact target
- **AND** actual logical namespace redeclarations, C++ free prototype/definition and existing in-class prototype calls beside separately represented qualified definitions, and C# partial members retain their compatible shared ownership; unrelated qualified type owners remain distinct

#### Scenario: Runtime forwarding queries preserve shared source module storage
- **WHEN** many native calls reach aliases from one explicit re-export statement with a long static module literal
- **THEN** query construction, cache hits and recursive frame clones retain shared module storage without copying its bytes, source ownership accounts those bytes once, value-based query identity and existing budget/cancellation refusal remain compatible, and ordinary valid navigation continues through CLI and MCP

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
