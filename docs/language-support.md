# ProjectAtlas Language Support

This document is generated from the versioned Rust language capability registry. Do not edit the capability table or totals by hand. Canonical rows count once; aliases and extensions never increase a capability total.

Registry version: `3`. Accepted capability-set version: `3`. Detection policy version: `1`. Registry digest: `a4b69ce4aed2ebf8d28f7b237ead76a53e5363e34c8c97ea5980776ea4217ef4`. Accepted-set digest: `a4b69ce4aed2ebf8d28f7b237ead76a53e5363e34c8c97ea5980776ea4217ef4`. Semantic-provider digest: `b26c4aa768ebe3bb185d5929350d2f41c6b1b2f93630080248dec7eb6ec00e82`.

Optional catalog input: `tree-sitter-language-pack@1.13.2` revision `6258abac30304283763a0d2dc8a48cb87fbcf438` under `MIT` metadata license. This catalog identity is not a grammar-license or runtime-support claim.

The registry contains **271** canonical rows: **63** default-core rows and **208** optional-pack candidates. Detection is supported for 271 rows. Parsing is supported for 30, fallback for 241, and unavailable for 0. Symbols are supported for 20, fallback for 241, and unavailable for 10. Semantic resolution and benchmark coverage are reported independently.

Rows marked `broad-parser` are detected and, when explicitly admitted to the scan policy, remain usable through the conservative default-core fallback while the optional pack is absent. Catalog recognition alone does not add these extensions to the default scan surface. The pinned catalog is provenance for detection metadata only. A row becomes grammar-backed parsed support only after its exact grammar binary, subtree license, ABI/export, fixtures, and every accepted optional-pack target pass the separate acceptance gates. The v0.4.0 optional-pack targets are Linux x86-64 and Windows x86-64; macOS keeps the full built-in surface and reports `unsupported_containment` for optional-pack activation. Built-in owners always retain precedence.

Broad candidate rows are admitted only when the pinned catalog supplies a stable canonical grammar identity and at least one ordinary extension that does not conflict with an already accepted detector owner. Extensionless, ambiguous, duplicate, pseudo, or conflicting catalog entries remain unadvertised until a separate deterministic rule and evidence exist.

| Language | Aliases | Detection rules | Parser owner | Parsed | Symbols | Semantic | Embedded source | Benchmarked | Optional pack | Provenance | License |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `rust` | `rs` | `.rs` | tree-sitter-rust@0.24.2 | supported | supported | supported (rust) | — | unavailable | — | `tree-sitter-rust@0.24.2` | `MIT` |
| `rust-build-script` | — | exact `build.rs` | tree-sitter-rust@0.24.2 | supported | supported | supported (rust) | — | unavailable | — | `tree-sitter-rust@0.24.2` | `MIT` |
| `python` | `py` | `.py`, `.pyw`, shebang `python`, shebang `pythonw` | tree-sitter-python@0.25.0 | supported | supported | supported (python) | — | unavailable | — | `tree-sitter-python@0.25.0` | `MIT` |
| `javascript` | `js` | `.js`, `.jsx`, `.mjs`, `.cjs`, shebang `node`, shebang `deno` | tree-sitter-javascript@0.25.0 | supported | supported | supported (ecma-script) | — | unavailable | — | `tree-sitter-javascript@0.25.0` | `MIT` |
| `typescript` | `ts` | compound `.d.ts`, `.ts`, `.d.ts` | tree-sitter-typescript@0.23.2 | supported | supported | supported (ecma-script) | — | unavailable | — | `tree-sitter-typescript@0.23.2` | `MIT` |
| `tsx` | — | `.tsx` | tree-sitter-typescript@0.23.2 | supported | supported | supported (ecma-script) | — | unavailable | — | `tree-sitter-typescript@0.23.2` | `MIT` |
| `java` | — | `.java` | tree-sitter-java@0.23.5 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-java@0.23.5` | `MIT` |
| `kotlin` | `kt` | `.kt`, `.kts` | tree-sitter-kotlin-ng@1.1.0 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-kotlin-ng@1.1.0` | `MIT` |
| `csharp` | `c#`, `cs` | `.cs` | tree-sitter-c-sharp@0.23.5 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-c-sharp@0.23.5` | `MIT` |
| `go` | — | `.go` | tree-sitter-go@0.25.0 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-go@0.25.0` | `MIT` |
| `objective-c` | `objc` | `.m`, `.mm` | tree-sitter-objc@3.0.2 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-objc@3.0.2` | `MIT` |
| `zig` | — | `.zig`, `.zon` | tree-sitter-zig@1.1.2 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-zig@1.1.2` | `MIT` |
| `c` | — | `.c` | tree-sitter-c@0.24.2 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-c@0.24.2` | `MIT` |
| `cpp` | `c++` | `.cpp`, `.cxx`, `.cc` | tree-sitter-cpp@0.23.4 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-cpp@0.23.4` | `MIT` |
| `h` | — | `.h` | tree-sitter-c@0.24.2 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-c@0.24.2` | `MIT` |
| `hpp` | — | `.hpp`, `.hxx`, `.hh` | tree-sitter-cpp@0.23.4 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-cpp@0.23.4` | `MIT` |
| `cargo-manifest` | — | exact `Cargo.toml` | projectatlas:cargo-manifest | supported | supported | supported (cargo) | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `cargo-lock` | — | exact `Cargo.lock` | projectatlas:cargo-manifest | supported | supported | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `vue` | — | `.vue` | projectatlas:vue | supported | supported | unavailable | component → ecma-script | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `markdown` | `md` | `.md`, `.mdx` | unavailable | supported | unavailable | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `json` | — | `.json`, `.jsonc` | unavailable | supported | unavailable | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `yaml` | `yml` | `.yml`, `.yaml` | unavailable | supported | unavailable | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `css` | — | `.css`, `.scss`, `.sass`, `.less`, `.stylus`, `.styl` | unavailable | supported | unavailable | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `html` | — | `.html`, `.htm` | unavailable | supported | unavailable | unavailable | html-like → ecma-script | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `toon` | — | `.toon` | unavailable | supported | unavailable | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `dockerfile` | — | exact `Dockerfile` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `makefile` | — | exact `Makefile` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `text` | `txt` | `.txt` | unavailable | supported | unavailable | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `toml` | — | `.toml` | unavailable | supported | unavailable | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `xml` | — | `.xml` | unavailable | supported | unavailable | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `svelte` | — | `.svelte` | projectatlas:fallback | fallback | fallback | unavailable | template → ecma-script | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `astro` | — | `.astro` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `jsp` | — | `.jsp`, `.jspx`, `.jspf` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `jsp-tag` | — | `.tag`, `.tagx` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `gsp` | — | `.gsp` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `groovy` | — | `.gradle`, `.groovy` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `protobuf` | `proto` | `.proto` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `handlebars` | `hbs` | `.hbs`, `.handlebars` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `ejs` | — | `.ejs` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `pug` | — | `.pug` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `freemarker` | `ftl` | `.ftl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `mustache` | — | `.mustache` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `liquid` | — | `.liquid` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `erb` | — | `.erb` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `sql` | — | `.sql`, `.ddl`, `.dml`, `.mysql`, `.postgresql`, `.psql`, `.sqlite`, `.mssql`, `.oracle`, `.ora`, `.db2`, `.proc`, `.procedure`, `.func`, `.function`, `.view`, `.trigger`, `.index`, `.migration`, `.seed`, `.fixture`, `.schema`, `.cql`, `.cypher`, `.sparql`, `.liquibase`, `.flyway` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `graphql` | `gql` | `.gql` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `config` | — | `.ini`, `.cfg`, `.conf`, `.properties`, `.env`, `.gitignore`, `.dockerignore`, `.editorconfig` | unavailable | supported | unavailable | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `ruby` | `rb` | `.rb`, shebang `ruby` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `php` | — | `.php` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `swift` | — | `.swift` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `scala` | — | `.scala` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `shell` | `sh` | `.sh`, `.bash`, `.zsh`, shebang `sh`, shebang `bash`, shebang `dash`, shebang `ash`, shebang `zsh`, shebang `ksh`, shebang `mksh`, shebang `fish` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `powershell` | `pwsh` | `.ps1`, `.psm1`, `.psd1`, shebang `powershell`, shebang `pwsh` | projectatlas:powershell | supported | supported | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `batch` | — | `.bat`, `.cmd` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `r` | `rscript` | `.r`, `.R`, shebang `rscript` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `perl` | — | `.pl`, `.pm`, shebang `perl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `lua` | — | `.lua`, shebang `lua` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `dart` | — | `.dart` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `haskell` | `hs` | `.hs` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `ocaml` | — | `.ml`, `.mli` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `fsharp` | `f#` | `.fs`, `.fsx` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `clojure` | `clj` | `.clj`, `.cljs` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `vim` | `vimscript` | `.vim` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.0` | `MIT` |
| `abl` | — | `.p` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `actionscript` | — | `.as` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `ada` | — | `.ada` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `agda` | — | `.agda` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `al` | — | `.al` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `arduino` | — | `.ino` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `asciidoc` | — | `.adoc` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `asm` | — | `.s` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `awk` | — | `.awk` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `beancount` | — | `.beancount` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `bibtex` | — | `.bib` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `bicep` | — | `.bicep` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `bitbake` | — | `.bb` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `blade` | — | `.blade` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `brightscript` | — | `.brs` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `bsl` | — | `.bsl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `c3` | — | `.c3` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `caddy` | — | `.caddyfile` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cairo` | — | `.cairo` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `capnp` | — | `.capnp` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cedar` | — | `.cedar` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cedarschema` | — | `.cedarschema` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cel` | — | `.cel` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cfml` | — | `.cfc` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `chatito` | — | `.chatito` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `chuck` | — | `.ck` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `circom` | — | `.circom` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `clarity` | — | `.clar` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cmake` | — | `.cmake` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cobol` | — | `.cobol` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `commonlisp` | — | `.lisp` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cooklang` | — | `.cook` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `corn` | — | `.corn` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cpon` | — | `.cpon` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `crystal` | — | `.cr` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cst` | — | `.cst` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `csv` | — | `.csv` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cuda` | — | `.cu` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cue` | — | `.cue` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cylc` | — | `.cylc` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `d` | — | `.d` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `desktop` | — | `.desktop` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `devicetree` | — | `.dts` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `dhall` | — | `.dhall` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `diff` | — | `.diff` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `djot` | — | `.dj` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `dot` | — | `.dot` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `dtd` | — | `.dtd` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `ebnf` | — | `.ebnf` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `eds` | — | `.eds` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `eex` | — | `.eex` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `elisp` | — | `.el` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `elixir` | — | `.ex` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `elm` | — | `.elm` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `elsa` | — | `.lc` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `elvish` | — | `.elv` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `enforce` | — | `.enforce` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `erlang` | — | `.erl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `facility` | — | `.fsd` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `faust` | — | `.dsp` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `fennel` | — | `.fnl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `fidl` | — | `.fidl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `firrtl` | — | `.fir` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `fish` | — | `.fish` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `forth` | — | `.fth` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `fortran` | — | `.f90` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `fsharp_signature` | — | `.fsi` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `func` | — | `.fc` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gap` | — | `.g` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gdscript` | — | `.gd` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gdshader` | — | `.gdshader` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gherkin` | — | `.feature` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gitattributes` | — | `.gitattributes` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gleam` | — | `.gleam` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `glsl` | — | `.glsl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gn` | — | `.gn` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gnuplot` | — | `.gp` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `godot_resource` | — | `.tres` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gomod` | — | `.mod` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gotmpl` | — | `.gotmpl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gren` | — | `.gren` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `hack` | — | `.hack` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `hare` | — | `.hare` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `haxe` | — | `.hx` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `hcl` | — | `.hcl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `heex` | — | `.heex` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `hjson` | — | `.hjson` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `hlsl` | — | `.hlsl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `hocon` | — | `.hocon` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `hoon` | — | `.hoon` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `http` | — | `.http` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `hurl` | — | `.hurl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `idris` | — | `.idr` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `ispc` | — | `.ispc` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `jai` | — | `.jai` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `janet` | — | `.janet` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `jinja2` | — | `.j2` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `jq` | — | `.jq` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `json5` | — | `.json5` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `jsonnet` | — | `.jsonnet` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `julia` | — | `.jl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `just` | — | `.just` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `kcl` | — | `.k` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `kdl` | — | `.kdl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `latex` | — | `.tex` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `lean` | — | `.lean` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `ledger` | — | `.ldg` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `linkerscript` | — | `.lds` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `llvm` | — | `.ll` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `luau` | — | `.luau` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `magik` | — | `.magik` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `make` | — | `.mk` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `matlab` | — | `.matlab` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `mermaid` | — | `.mmd` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `meson` | — | `.meson` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `mlir` | — | `.mlir` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `mojo` | — | `.mojo` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `move` | — | `.move` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `nasm` | — | `.nasm` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `netlinx` | — | `.axs` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `nginx` | — | `.nginx` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `nickel` | — | `.ncl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `nim` | — | `.nim` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `ninja` | — | `.ninja` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `nix` | — | `.nix` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `norg` | — | `.norg` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `nqc` | — | `.nqc` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `nushell` | — | `.nu` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `ocamllex` | — | `.mll` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `odin` | — | `.odin` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `openscad` | — | `.scad` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `org` | — | `.org` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `pascal` | — | `.pas` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `pem` | — | `.pem` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `pgn` | — | `.pgn` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `pkl` | — | `.pkl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `po` | — | `.po` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `poe_filter` | — | `.filter` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `pony` | — | `.pony` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `postscript` | — | `.ps` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `prisma` | — | `.prisma` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `prolog` | — | `.pro` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `promql` | — | `.promql` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `prql` | — | `.prql` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `psv` | — | `.psv` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `puppet` | — | `.pp` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `purescript` | — | `.purs` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `ql` | — | `.ql` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `qmljs` | — | `.qml` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `racket` | — | `.rkt` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `rasi` | — | `.rasi` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `razor` | — | `.razor` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `rbs` | — | `.rbs` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `re2c` | — | `.re` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `rego` | — | `.rego` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `rescript` | — | `.res` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `robot` | — | `.robot` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `roc` | — | `.roc` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `ron` | — | `.ron` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `rst` | — | `.rst` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `rtf` | — | `.rtf` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `scheme` | — | `.scm` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `slang` | — | `.slang` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `smali` | — | `.smali` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `smalltalk` | — | `.st` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `smithy` | — | `.smithy` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `sml` | — | `.sml` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `snakemake` | — | `.smk` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `solidity` | — | `.sol` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `souffle` | — | `.dl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `sourcepawn` | — | `.sp` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `sql_bigquery` | — | `.bq` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `squirrel` | — | `.squirrel` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `stan` | — | `.stan` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `starlark` | — | `.star` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `superhtml` | — | `.shtml` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `sway` | — | `.sw` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `systemverilog` | — | `.sv` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `tablegen` | — | `.td` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `tact` | — | `.tact` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `tcl` | — | `.tcl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `teal` | — | `.tl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `templ` | — | `.templ` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `tera` | — | `.tera` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `terraform` | — | `.tf` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `textproto` | — | `.textproto` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `thrift` | — | `.thrift` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `tlaplus` | — | `.tla` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `todotxt` | — | `.todotxt` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `tsv` | — | `.tsv` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `turtle` | — | `.ttl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `twig` | — | `.twig` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `typespec` | — | `.tsp` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `typoscript` | — | `.typoscript` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `typst` | — | `.typst` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `uxntal` | — | `.tal` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `v` | — | `.v` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `vb` | — | `.vb` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `verilog` | — | `.verilog` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `vhdl` | — | `.vhdl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `vhs` | — | `.tape` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `vrl` | — | `.vrl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `wast` | — | `.wast` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `wat` | — | `.wat` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `wgsl` | — | `.wgsl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `wit` | — | `.wit` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `yuck` | — | `.yuck` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `ziggy` | — | `.ziggy` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
