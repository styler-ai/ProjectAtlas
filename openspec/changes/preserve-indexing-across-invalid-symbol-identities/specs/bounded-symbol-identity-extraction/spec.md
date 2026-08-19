## ADDED Requirements

### Requirement: Declaration identities are semantic and bounded

Built-in symbol extraction SHALL derive a declaration name from semantic name or declarator fields and MUST NOT substitute the complete declaration statement when no such name exists. The shared extraction boundary SHALL omit, rather than truncate or hash, a declaration whose compact semantic name exceeds the product bound.

#### Scenario: Large C# field initializer retains its declared name

- **WHEN** a C# static field named `D` contains a dictionary initializer large enough for the complete declaration to exceed the graph-identity byte limit
- **THEN** extraction records the semantic name `D` and never records the initializer statement as the symbol name

#### Scenario: Declaration has no semantic name

- **WHEN** a parser declaration node exposes no admissible semantic name or declarator
- **THEN** extraction omits only that declaration and does not synthesize an identity from its source text

#### Scenario: Semantic name exceeds the product bound

- **WHEN** a declaration exposes a semantic name beyond the shared symbol-name bound
- **THEN** extraction omits that declaration without truncation, hashing, collision, panic, or unbounded retained output

#### Scenario: Unicode name stays inside the graph byte contract

- **WHEN** a multibyte UTF-8 semantic name is admitted by the symbol-name bound
- **THEN** its graph identity remains within the durable byte contract and preserves the exact admitted name

### Requirement: One unadmittable declaration cannot abort repository publication

Full and incremental indexing SHALL preserve the file, every other admissible symbol and relation, and the repository's atomic publication when one declaration has no admissible graph identity.

#### Scenario: Reported 225-entry registry scans successfully

- **WHEN** a repository containing the reported 225-entry C# static dictionary is scanned
- **THEN** the scan succeeds, the source file is indexed, the field is addressable as `D`, and unrelated repository facts remain available

#### Scenario: Invalid declaration is isolated

- **WHEN** one source file contains an unadmittable declaration alongside admissible declarations and other files
- **THEN** only that declaration is omitted and the scan does not degrade the complete file or repository

#### Scenario: Incremental edit crosses the former failure boundary

- **WHEN** an indexed C# initializer grows from the former 224-entry success case to 225 entries
- **THEN** incremental indexing publishes the new generation successfully without losing prior unrelated facts
