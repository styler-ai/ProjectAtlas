# Broad parser source evidence

`tree-sitter-language-pack-1.13.2.json` is the pinned, content-bearing intake
snapshot used to generate ProjectAtlas's logical broad-parser capability
manifest. It contains only accepted candidate rows whose exact upstream
revision, compile inputs, applicable license text, ABI/export identity, and
natural fixture inputs were collected and validated.

The source documents preserve two distinct upstream identities: the VCS revision
embedded in the published Cargo archive and the later release-tag revision that
owns the parser-source and native release assets. They must never be collapsed
into one ambiguous revision claim.

`tree-sitter-language-pack-1.13.2-platform-bundles.json` binds that release
revision to the upstream release manifest and the exact Linux x86-64 and Windows
x86-64 native bundle assets ProjectAtlas may repackage. Its per-platform URLs, byte lengths, and
SHA-256 values are source-intake authority only; the repackager must still
select exactly the accepted logical rows and independently verify every copied
library, export, ABI, dependency, and fixture result.

The snapshot is input evidence, not a support claim. A language becomes
advertised grammar-backed capability only after the generated logical manifest
and both accepted optional-pack artifacts pass their independent gates. macOS
keeps ProjectAtlas's complete built-in parser surface but has no v0.4.0 optional-
pack artifact because the accepted containment contract cannot be proved there.
Built-in ProjectAtlas parser owners always retain precedence.

Positive fixtures retain exact natural upstream cases. Negative fixtures are
either exact upstream error cases or deterministic incomplete editor-state forms
of those real cases, modeling saved editor state rather than invented language
samples. Their origin and transformation stay explicit. Every negative must
still produce its declared outcome through the selected grammar on every
accepted optional-pack target before the language can be advertised as grammar-backed.

The version in the filename is the external upstream package version. Updating
the source requires a new pinned snapshot rather than rewriting the historical
input in place.
