# ProjectAtlas Language Support

This document is generated from the versioned Rust language capability registry. Do not edit the capability table or totals by hand. Canonical rows count once; aliases and extensions never increase a capability total.

Registry version: `4`. Accepted capability-set version: `11`. Detection policy version: `1`. Registry digest: `3776f19c62b3debfcae13715e3bdc3ec3029978a4f7ba1428b7a06d433524915`. Accepted-set digest: `3776f19c62b3debfcae13715e3bdc3ec3029978a4f7ba1428b7a06d433524915`. Semantic-provider digest: `b26c4aa768ebe3bb185d5929350d2f41c6b1b2f93630080248dec7eb6ec00e82`.

Optional catalog input: `tree-sitter-language-pack@1.13.2` revision `6258abac30304283763a0d2dc8a48cb87fbcf438` under `MIT` metadata license. This catalog identity is not a grammar-license or runtime-support claim.

The registry contains **271** canonical rows: **63** default-core rows and **208** optional-pack candidates. Detection is supported for 271 rows. Parsing is supported for 30, fallback for 241, and unavailable for 0. Symbols are supported for 21, fallback for 241, and unavailable for 9. Semantic resolution and benchmark coverage are reported independently.

Rows marked `broad-parser` are detected and, when explicitly admitted to the scan policy, remain usable through the conservative default-core fallback while the optional pack is absent. Catalog recognition alone does not add these extensions to the default scan surface. The pinned catalog is provenance for detection metadata only. A row becomes grammar-backed parsed support only after its exact grammar binary, subtree license, ABI/export, fixtures, and every accepted optional-pack target pass the separate acceptance gates. The v0.4 optional-pack targets are Linux x86-64 and Windows x86-64; macOS keeps the full built-in surface and reports `unsupported_containment` for optional-pack activation. Built-in owners always retain precedence.

Broad candidate rows are admitted only when the pinned catalog supplies a stable canonical grammar identity and at least one ordinary extension that does not conflict with an already accepted detector owner. Extensionless, ambiguous, duplicate, pseudo, or conflicting catalog entries remain unadvertised until a separate deterministic rule and evidence exist.

| Language | Classification | Aliases | Detection rules | Parser owner | Parsed | Symbols | Semantic | Embedded source | Benchmarked | Optional pack | Provenance | License |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `rust` | `source` | `rs` | `.rs` | tree-sitter-rust@0.24.2 | supported | supported | supported (rust) | — | unavailable | — | `tree-sitter-rust@0.24.2` | `MIT` |
| `rust-build-script` | `source` | — | exact `build.rs` | tree-sitter-rust@0.24.2 | supported | supported | supported (rust) | — | unavailable | — | `tree-sitter-rust@0.24.2` | `MIT` |
| `python` | `source` | `py` | `.py`, `.pyw`, shebang `python`, shebang `pythonw` | tree-sitter-python@0.25.0 | supported | supported | supported (python) | — | unavailable | — | `tree-sitter-python@0.25.0` | `MIT` |
| `javascript` | `source` | `js` | `.js`, `.jsx`, `.mjs`, `.cjs`, shebang `node`, shebang `deno` | tree-sitter-javascript@0.25.0 | supported | supported | supported (ecma-script) | — | unavailable | — | `tree-sitter-javascript@0.25.0` | `MIT` |
| `typescript` | `source` | `ts` | compound `.d.ts`, `.ts`, `.d.ts` | tree-sitter-typescript@0.23.2 | supported | supported | supported (ecma-script) | — | unavailable | — | `tree-sitter-typescript@0.23.2` | `MIT` |
| `tsx` | `source` | — | `.tsx` | tree-sitter-typescript@0.23.2 | supported | supported | supported (ecma-script) | — | unavailable | — | `tree-sitter-typescript@0.23.2` | `MIT` |
| `java` | `source` | — | `.java` | tree-sitter-java@0.23.5 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-java@0.23.5` | `MIT` |
| `kotlin` | `source` | `kt` | `.kt`, `.kts` | tree-sitter-kotlin-ng@1.1.0 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-kotlin-ng@1.1.0` | `MIT` |
| `csharp` | `source` | `c#`, `cs` | `.cs` | tree-sitter-c-sharp@0.23.5 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-c-sharp@0.23.5` | `MIT` |
| `go` | `source` | — | `.go` | tree-sitter-go@0.25.0 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-go@0.25.0` | `MIT` |
| `objective-c` | `source` | `objc` | `.m`, `.mm` | tree-sitter-objc@3.0.2 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-objc@3.0.2` | `MIT` |
| `zig` | `source` | — | `.zig`, `.zon` | tree-sitter-zig@1.1.2 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-zig@1.1.2` | `MIT` |
| `c` | `source` | — | `.c` | tree-sitter-c@0.24.2 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-c@0.24.2` | `MIT` |
| `cpp` | `source` | `c++` | `.cpp`, `.cxx`, `.cc` | tree-sitter-cpp@0.23.4 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-cpp@0.23.4` | `MIT` |
| `h` | `source` | — | `.h` | tree-sitter-c@0.24.2 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-c@0.24.2` | `MIT` |
| `hpp` | `source` | — | `.hpp`, `.hxx`, `.hh` | tree-sitter-cpp@0.23.4 | supported | supported | unavailable | — | unavailable | — | `tree-sitter-cpp@0.23.4` | `MIT` |
| `cargo-manifest` | `configuration_data` | — | exact `Cargo.toml` | projectatlas:cargo-manifest | supported | supported | supported (cargo) | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `cargo-lock` | `configuration_data` | — | exact `Cargo.lock` | projectatlas:cargo-manifest | supported | supported | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `vue` | `source` | — | `.vue` | projectatlas:vue | supported | supported | unavailable | component → ecma-script | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `markdown` | `documentation` | `md` | `.md`, `.mdx` | projectatlas:markdown | supported | supported | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `json` | `configuration_data` | — | `.json`, `.jsonc` | unavailable | supported | unavailable | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `yaml` | `configuration_data` | `yml` | `.yml`, `.yaml` | unavailable | supported | unavailable | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `css` | `source` | — | `.css`, `.scss`, `.sass`, `.less`, `.stylus`, `.styl` | unavailable | supported | unavailable | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `html` | `source` | — | `.html`, `.htm` | unavailable | supported | unavailable | unavailable | html-like → ecma-script | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `toon` | `configuration_data` | — | `.toon` | unavailable | supported | unavailable | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `dockerfile` | `source` | — | exact `Dockerfile` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `makefile` | `source` | — | exact `Makefile` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `text` | `other_text` | `txt` | `.txt` | unavailable | supported | unavailable | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `toml` | `configuration_data` | — | `.toml` | unavailable | supported | unavailable | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `xml` | `configuration_data` | — | `.xml` | unavailable | supported | unavailable | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `svelte` | `source` | — | `.svelte` | projectatlas:fallback | fallback | fallback | unavailable | template → ecma-script | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `astro` | `source` | — | `.astro` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `jsp` | `source` | — | `.jsp`, `.jspx`, `.jspf` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `jsp-tag` | `source` | — | `.tag`, `.tagx` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `gsp` | `source` | — | `.gsp` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `groovy` | `source` | — | `.gradle`, `.groovy` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `protobuf` | `source` | `proto` | `.proto` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `handlebars` | `source` | `hbs` | `.hbs`, `.handlebars` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `ejs` | `source` | — | `.ejs` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `pug` | `source` | — | `.pug` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `freemarker` | `source` | `ftl` | `.ftl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `mustache` | `source` | — | `.mustache` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `liquid` | `source` | — | `.liquid` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `erb` | `source` | — | `.erb` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `sql` | `source` | — | `.sql`, `.ddl`, `.dml`, `.mysql`, `.postgresql`, `.psql`, `.sqlite`, `.mssql`, `.oracle`, `.ora`, `.db2`, `.proc`, `.procedure`, `.func`, `.function`, `.view`, `.trigger`, `.index`, `.migration`, `.seed`, `.fixture`, `.schema`, `.cql`, `.cypher`, `.sparql`, `.liquibase`, `.flyway` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `graphql` | `source` | `gql` | `.gql` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `config` | `configuration_data` | — | `.ini`, `.cfg`, `.conf`, `.properties`, `.env`, `.gitignore`, `.dockerignore`, `.editorconfig` | unavailable | supported | unavailable | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `ruby` | `source` | `rb` | `.rb`, shebang `ruby` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `php` | `source` | — | `.php` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `swift` | `source` | — | `.swift` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `scala` | `source` | — | `.scala` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `shell` | `source` | `sh` | `.sh`, `.bash`, `.zsh`, shebang `sh`, shebang `bash`, shebang `dash`, shebang `ash`, shebang `zsh`, shebang `ksh`, shebang `mksh`, shebang `fish` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `powershell` | `source` | `pwsh` | `.ps1`, `.psm1`, `.psd1`, shebang `powershell`, shebang `pwsh` | projectatlas:powershell | supported | supported | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `batch` | `source` | — | `.bat`, `.cmd` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `r` | `source` | `rscript` | `.r`, `.R`, shebang `rscript` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `perl` | `source` | — | `.pl`, `.pm`, shebang `perl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `lua` | `source` | — | `.lua`, shebang `lua` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `dart` | `source` | — | `.dart` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `haskell` | `source` | `hs` | `.hs` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `ocaml` | `source` | — | `.ml`, `.mli` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `fsharp` | `source` | `f#` | `.fs`, `.fsx` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `clojure` | `source` | `clj` | `.clj`, `.cljs` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `vim` | `source` | `vimscript` | `.vim` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | — | `projectatlas@0.4.5-rc3` | `MIT` |
| `abl` | `source` | — | `.p` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `actionscript` | `source` | — | `.as` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `ada` | `source` | — | `.ada` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `agda` | `source` | — | `.agda` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `al` | `source` | — | `.al` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `arduino` | `source` | — | `.ino` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `asciidoc` | `documentation` | — | `.adoc` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `asm` | `source` | — | `.s` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `awk` | `source` | — | `.awk` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `beancount` | `configuration_data` | — | `.beancount` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `bibtex` | `configuration_data` | — | `.bib` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `bicep` | `source` | — | `.bicep` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `bitbake` | `source` | — | `.bb` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `blade` | `source` | — | `.blade` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `brightscript` | `source` | — | `.brs` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `bsl` | `source` | — | `.bsl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `c3` | `source` | — | `.c3` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `caddy` | `configuration_data` | — | `.caddyfile` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cairo` | `source` | — | `.cairo` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `capnp` | `source` | — | `.capnp` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cedar` | `source` | — | `.cedar` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cedarschema` | `source` | — | `.cedarschema` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cel` | `source` | — | `.cel` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cfml` | `source` | — | `.cfc` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `chatito` | `source` | — | `.chatito` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `chuck` | `source` | — | `.ck` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `circom` | `source` | — | `.circom` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `clarity` | `source` | — | `.clar` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cmake` | `source` | — | `.cmake` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cobol` | `source` | — | `.cobol` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `commonlisp` | `source` | — | `.lisp` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cooklang` | `source` | — | `.cook` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `corn` | `source` | — | `.corn` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cpon` | `configuration_data` | — | `.cpon` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `crystal` | `source` | — | `.cr` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cst` | `source` | — | `.cst` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `csv` | `configuration_data` | — | `.csv` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cuda` | `source` | — | `.cu` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cue` | `source` | — | `.cue` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `cylc` | `source` | — | `.cylc` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `d` | `source` | — | `.d` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `desktop` | `configuration_data` | — | `.desktop` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `devicetree` | `source` | — | `.dts` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `dhall` | `source` | — | `.dhall` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `diff` | `other_text` | — | `.diff` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `djot` | `documentation` | — | `.dj` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `dot` | `source` | — | `.dot` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `dtd` | `source` | — | `.dtd` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `ebnf` | `source` | — | `.ebnf` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `eds` | `source` | — | `.eds` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `eex` | `source` | — | `.eex` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `elisp` | `source` | — | `.el` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `elixir` | `source` | — | `.ex` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `elm` | `source` | — | `.elm` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `elsa` | `source` | — | `.lc` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `elvish` | `source` | — | `.elv` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `enforce` | `source` | — | `.enforce` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `erlang` | `source` | — | `.erl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `facility` | `source` | — | `.fsd` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `faust` | `source` | — | `.dsp` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `fennel` | `source` | — | `.fnl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `fidl` | `source` | — | `.fidl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `firrtl` | `source` | — | `.fir` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `fish` | `source` | — | `.fish` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `forth` | `source` | — | `.fth` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `fortran` | `source` | — | `.f90` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `fsharp_signature` | `source` | — | `.fsi` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `func` | `source` | — | `.fc` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gap` | `source` | — | `.g` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gdscript` | `source` | — | `.gd` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gdshader` | `source` | — | `.gdshader` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gherkin` | `source` | — | `.feature` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gitattributes` | `configuration_data` | — | `.gitattributes` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gleam` | `source` | — | `.gleam` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `glsl` | `source` | — | `.glsl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gn` | `source` | — | `.gn` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gnuplot` | `source` | — | `.gp` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `godot_resource` | `configuration_data` | — | `.tres` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gomod` | `configuration_data` | — | `.mod` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gotmpl` | `source` | — | `.gotmpl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `gren` | `source` | — | `.gren` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `hack` | `source` | — | `.hack` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `hare` | `source` | — | `.hare` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `haxe` | `source` | — | `.hx` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `hcl` | `configuration_data` | — | `.hcl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `heex` | `source` | — | `.heex` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `hjson` | `configuration_data` | — | `.hjson` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `hlsl` | `source` | — | `.hlsl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `hocon` | `configuration_data` | — | `.hocon` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `hoon` | `source` | — | `.hoon` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `http` | `source` | — | `.http` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `hurl` | `source` | — | `.hurl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `idris` | `source` | — | `.idr` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `ispc` | `source` | — | `.ispc` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `jai` | `source` | — | `.jai` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `janet` | `source` | — | `.janet` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `jinja2` | `source` | — | `.j2` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `jq` | `source` | — | `.jq` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `json5` | `configuration_data` | — | `.json5` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `jsonnet` | `source` | — | `.jsonnet` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `julia` | `source` | — | `.jl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `just` | `source` | — | `.just` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `kcl` | `source` | — | `.k` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `kdl` | `configuration_data` | — | `.kdl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `latex` | `documentation` | — | `.tex` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `lean` | `source` | — | `.lean` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `ledger` | `configuration_data` | — | `.ldg` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `linkerscript` | `source` | — | `.lds` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `llvm` | `source` | — | `.ll` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `luau` | `source` | — | `.luau` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `magik` | `source` | — | `.magik` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `make` | `source` | — | `.mk` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `matlab` | `source` | — | `.matlab` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `mermaid` | `source` | — | `.mmd` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `meson` | `source` | — | `.meson` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `mlir` | `source` | — | `.mlir` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `mojo` | `source` | — | `.mojo` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `move` | `source` | — | `.move` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `nasm` | `source` | — | `.nasm` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `netlinx` | `source` | — | `.axs` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `nginx` | `source` | — | `.nginx` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `nickel` | `source` | — | `.ncl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `nim` | `source` | — | `.nim` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `ninja` | `source` | — | `.ninja` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `nix` | `source` | — | `.nix` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `norg` | `documentation` | — | `.norg` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `nqc` | `source` | — | `.nqc` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `nushell` | `source` | — | `.nu` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `ocamllex` | `source` | — | `.mll` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `odin` | `source` | — | `.odin` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `openscad` | `source` | — | `.scad` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `org` | `documentation` | — | `.org` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `pascal` | `source` | — | `.pas` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `pem` | `configuration_data` | — | `.pem` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `pgn` | `configuration_data` | — | `.pgn` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `pkl` | `source` | — | `.pkl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `po` | `configuration_data` | — | `.po` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `poe_filter` | `source` | — | `.filter` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `pony` | `source` | — | `.pony` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `postscript` | `source` | — | `.ps` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `prisma` | `source` | — | `.prisma` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `prolog` | `source` | — | `.pro` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `promql` | `source` | — | `.promql` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `prql` | `source` | — | `.prql` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `psv` | `configuration_data` | — | `.psv` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `puppet` | `source` | — | `.pp` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `purescript` | `source` | — | `.purs` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `ql` | `source` | — | `.ql` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `qmljs` | `source` | — | `.qml` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `racket` | `source` | — | `.rkt` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `rasi` | `source` | — | `.rasi` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `razor` | `source` | — | `.razor` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `rbs` | `source` | — | `.rbs` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `re2c` | `source` | — | `.re` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `rego` | `source` | — | `.rego` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `rescript` | `source` | — | `.res` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `robot` | `source` | — | `.robot` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `roc` | `source` | — | `.roc` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `ron` | `configuration_data` | — | `.ron` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `rst` | `documentation` | — | `.rst` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `rtf` | `documentation` | — | `.rtf` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `scheme` | `source` | — | `.scm` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `slang` | `source` | — | `.slang` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `smali` | `source` | — | `.smali` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `smalltalk` | `source` | — | `.st` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `smithy` | `source` | — | `.smithy` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `sml` | `source` | — | `.sml` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `snakemake` | `source` | — | `.smk` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `solidity` | `source` | — | `.sol` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `souffle` | `source` | — | `.dl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `sourcepawn` | `source` | — | `.sp` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `sql_bigquery` | `source` | — | `.bq` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `squirrel` | `source` | — | `.squirrel` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `stan` | `source` | — | `.stan` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `starlark` | `source` | — | `.star` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `superhtml` | `source` | — | `.shtml` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `sway` | `source` | — | `.sw` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `systemverilog` | `source` | — | `.sv` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `tablegen` | `source` | — | `.td` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `tact` | `source` | — | `.tact` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `tcl` | `source` | — | `.tcl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `teal` | `source` | — | `.tl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `templ` | `source` | — | `.templ` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `tera` | `source` | — | `.tera` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `terraform` | `configuration_data` | — | `.tf` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `textproto` | `configuration_data` | — | `.textproto` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `thrift` | `source` | — | `.thrift` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `tlaplus` | `source` | — | `.tla` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `todotxt` | `other_text` | — | `.todotxt` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `tsv` | `configuration_data` | — | `.tsv` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `turtle` | `configuration_data` | — | `.ttl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `twig` | `source` | — | `.twig` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `typespec` | `source` | — | `.tsp` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `typoscript` | `source` | — | `.typoscript` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `typst` | `documentation` | — | `.typst` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `uxntal` | `source` | — | `.tal` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `v` | `source` | — | `.v` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `vb` | `source` | — | `.vb` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `verilog` | `source` | — | `.verilog` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `vhdl` | `source` | — | `.vhdl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `vhs` | `source` | — | `.tape` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `vrl` | `source` | — | `.vrl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `wast` | `source` | — | `.wast` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `wat` | `source` | — | `.wat` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `wgsl` | `source` | — | `.wgsl` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `wit` | `source` | — | `.wit` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `yuck` | `source` | — | `.yuck` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |
| `ziggy` | `source` | — | `.ziggy` | projectatlas:fallback | fallback | fallback | unavailable | — | unavailable | broad-parser | `tree-sitter-language-pack@1.13.2` | `MIT` |

## Language & Ecosystem Support

Complete-support schema version: `1`. Ecosystem catalog version: `1`. Catalog digest: `b142590eeb65e4af79d51d76e73342fce6515c3834a2637d8a7aee349c35fa70`.

`Complete` means conformance to the fixed ProjectAtlas navigation contract, not compiler, build-system, runtime, or whole-language completeness. Final v0.4 MCP navigation revalidation retained every runtime candidate at its achieved detected/parsed/symbol/semantic/benchmarked tier: none has the complete schema-bound capability and agent-navigation evidence required for promotion.

Catalog profile counts stay separate: **29** languages, **22** dialects, **11** domain formats, and **21** framework projections. Current assessment: **23** lower-tier runtime candidates, **12** planned documentation rows, **48** unavailable documentation rows, and **0** accepted complete profiles.

### Detection-to-navigation pipeline

ProjectAtlas first applies deterministic registry-owned detection. Built-in or explicitly enabled contained optional parsing then produces honest parse coverage; fact providers retain their own provenance; typed resolution preserves resolved, ambiguous, unresolved, and external outcomes; one atomic SQLite generation publishes exact occurrences and relations; freshness-aware MCP navigation returns bounded source selectors and exact evidence. We reuse maintained license-compatible Tree-sitter grammars, generated parser/node metadata, and trustworthy standard queries before adding bounded ProjectAtlas queries or concrete Rust semantic logic. ProjectAtlas never executes repository code, and an absent optional pack leaves default-core startup and navigation independent.

The `legacy-modernization` tag identifies source where trustworthy dependency and exact-evidence navigation is valuable. It does **not** claim automatic conversion or select a target language. Planned and unavailable rows below are documentation classifications only: they create no runtime registry row and contribute to no capability or complete-support total.

### Architecture paths

- [Canonical Mermaid architecture views](projectatlas-3-architecture.md#architecture-views)
- [System and component ownership](projectatlas-3-architecture.md#system-and-component-architecture)
- [Crate dependency and ownership](projectatlas-3-architecture.md#crate-dependency-and-ownership)
- [Database authority](projectatlas-3-architecture.md#database-authority-and-responsibility)
- [Graph physical model](projectatlas-3-architecture.md#normalized-graph-physical-model)
- [Bounded graph read](projectatlas-3-architecture.md#bounded-graph-read-with-purpose-projection)
- [MCP read communication](projectatlas-3-architecture.md#mcp-read-communication-sequence)
- [Transactional publication](projectatlas-3-architecture.md#index-and-transactional-publication-flow)
- [Language registry to agent navigation](projectatlas-3-architecture.md#language-registry-to-agent-navigation)

### Backend

| Profile | Kind | Host | Assessment | Dialect evidence | Tags |
| --- | --- | --- | --- | --- | --- |
| Perl | `language` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | legacy-modernization |
| ColdFusion / CFML | `language` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | legacy-modernization |

### Frontend and web

| Profile | Kind | Host | Assessment | Dialect evidence | Tags |
| --- | --- | --- | --- | --- | --- |
| ActionScript | `language` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | legacy-modernization |
| Apache Flex | `framework_projection` | actionscript | unavailable: missing-framework-projection | — | legacy-modernization |

### Systems

| Profile | Kind | Host | Assessment | Dialect evidence | Tags |
| --- | --- | --- | --- | --- | --- |
| Other assembler families | `language` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | legacy-modernization |
| Ada | `language` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | legacy-modernization |

### Mobile

| Profile | Kind | Host | Assessment | Dialect evidence | Tags |
| --- | --- | --- | --- | --- | --- |
| Swift | `language` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | — |
| Kotlin | `language` | — | candidate: detected supported, parsed supported, symbols supported, semantic unavailable, benchmarked unavailable | — | — |
| Dart | `language` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | — |

### Data and scientific

| Profile | Kind | Host | Assessment | Dialect evidence | Tags |
| --- | --- | --- | --- | --- | --- |
| Fortran | `language` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | legacy-modernization |
| Fortran fixed form | `dialect` | fortran | unavailable: missing-independent-dialect-evidence | content-signature | legacy-modernization |
| Fortran free form | `dialect` | fortran | unavailable: missing-independent-dialect-evidence | content-signature | legacy-modernization |
| SAS | `language` | — | unavailable: no-runtime-capability | — | legacy-modernization |

### Enterprise and legacy modernization

| Profile | Kind | Host | Assessment | Dialect evidence | Tags |
| --- | --- | --- | --- | --- | --- |
| ABAP | `language` | — | unavailable: no-runtime-capability | — | legacy-modernization |
| OpenEdge ABL | `language` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | legacy-modernization |
| COBOL | `language` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | legacy-modernization |
| COBOL fixed form | `dialect` | cobol | unavailable: missing-independent-dialect-evidence | content-signature | legacy-modernization |
| COBOL free form | `dialect` | cobol | unavailable: missing-independent-dialect-evidence | content-signature | legacy-modernization |
| PL/I | `language` | — | unavailable: no-runtime-capability | — | legacy-modernization |
| RPG | `language` | — | unavailable: no-runtime-capability | — | legacy-modernization |
| ILE RPG | `dialect` | rpg | unavailable: missing-independent-dialect-evidence | project-manifest | legacy-modernization |
| JCL | `domain_format` | — | unavailable: no-runtime-capability | — | legacy-modernization |
| REXX | `language` | — | unavailable: no-runtime-capability | — | legacy-modernization |
| IBM i CL | `language` | — | unavailable: no-runtime-capability | — | legacy-modernization |
| HLASM | `language` | — | unavailable: no-runtime-capability | — | legacy-modernization |
| Pascal | `language` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | legacy-modernization |
| Object Pascal | `dialect` | pascal | unavailable: missing-independent-dialect-evidence | content-signature | legacy-modernization |
| Delphi | `dialect` | pascal | unavailable: missing-independent-dialect-evidence | project-manifest | legacy-modernization |
| Visual Basic source (unqualified) | `language` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | legacy-modernization |
| Visual Basic 6 | `dialect` | visual-basic | unavailable: missing-independent-dialect-evidence | project-manifest | legacy-modernization |
| VB.NET | `dialect` | visual-basic | unavailable: missing-independent-dialect-evidence | project-manifest | legacy-modernization |
| VBA | `dialect` | visual-basic | unavailable: missing-independent-dialect-evidence | project-manifest | legacy-modernization |
| VBScript | `language` | — | unavailable: no-runtime-capability | — | legacy-modernization |
| Classic ASP | `framework_projection` | vbscript | unavailable: missing-framework-projection | — | legacy-modernization |
| Natural | `language` | — | unavailable: no-runtime-capability | — | legacy-modernization |
| MUMPS / M | `language` | — | unavailable: no-runtime-capability | — | legacy-modernization |
| PowerBuilder | `framework_projection` | powerscript | unavailable: missing-framework-projection | — | legacy-modernization |
| PowerScript | `language` | — | unavailable: no-runtime-capability | — | legacy-modernization |
| xBase | `language` | — | unavailable: no-runtime-capability | — | legacy-modernization |
| Clipper | `dialect` | xbase | unavailable: missing-independent-dialect-evidence | project-manifest | legacy-modernization |
| FoxPro | `dialect` | xbase | unavailable: missing-independent-dialect-evidence | project-manifest | legacy-modernization |

### Database and query

| Profile | Kind | Host | Assessment | Dialect evidence | Tags |
| --- | --- | --- | --- | --- | --- |
| SQL (unqualified) | `language` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | — |
| Oracle PL/SQL | `dialect` | sql | unavailable: missing-independent-dialect-evidence | content-signature | — |
| PostgreSQL PL/pgSQL | `dialect` | sql | unavailable: missing-independent-dialect-evidence | content-signature | — |
| T-SQL | `dialect` | sql | unavailable: missing-independent-dialect-evidence | content-signature | — |
| MySQL SQL | `dialect` | sql | unavailable: missing-independent-dialect-evidence | configuration | — |
| MariaDB SQL | `dialect` | sql | unavailable: missing-independent-dialect-evidence | configuration | — |
| SQLite SQL | `dialect` | sql | unavailable: missing-independent-dialect-evidence | configuration | — |
| BigQuery SQL | `dialect` | sql | unavailable: missing-independent-dialect-evidence | configuration | — |
| Snowflake SQL | `dialect` | sql | unavailable: missing-independent-dialect-evidence | configuration | — |
| Redshift SQL | `dialect` | sql | unavailable: missing-independent-dialect-evidence | configuration | — |
| dbt / Jinja SQL | `framework_projection` | sql | unavailable: missing-framework-projection | — | — |

### Infrastructure and cloud

| Profile | Kind | Host | Assessment | Dialect evidence | Tags |
| --- | --- | --- | --- | --- | --- |
| Terraform | `domain_format` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | — |
| OpenTofu | `dialect` | terraform | unavailable: missing-independent-dialect-evidence | project-manifest | — |
| HCL | `domain_format` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | — |
| Bicep | `domain_format` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | — |
| Azure ARM template | `domain_format` | — | unavailable: no-runtime-capability | — | — |
| AWS CloudFormation | `domain_format` | — | unavailable: no-runtime-capability | — | — |
| AWS SAM | `domain_format` | — | unavailable: no-runtime-capability | — | — |
| Pulumi TypeScript constructs | `framework_projection` | typescript | planned (documentation only) | — | — |
| Pulumi Python constructs | `framework_projection` | python | planned (documentation only) | — | — |
| Kubernetes manifests | `domain_format` | — | unavailable: no-runtime-capability | — | — |
| Helm charts | `framework_projection` | yaml | unavailable: missing-framework-projection | — | — |
| Kustomize | `framework_projection` | yaml | unavailable: missing-framework-projection | — | — |
| Crossplane | `framework_projection` | yaml | unavailable: missing-framework-projection | — | — |
| Ansible | `framework_projection` | yaml | unavailable: missing-framework-projection | — | — |
| Nix | `domain_format` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | — |
| CUE | `domain_format` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | — |
| Docker Compose | `framework_projection` | yaml | unavailable: missing-framework-projection | — | — |
| AWS CDK TypeScript constructs | `framework_projection` | typescript | planned (documentation only) | — | — |
| AWS CDK Python constructs | `framework_projection` | python | planned (documentation only) | — | — |

### Build, configuration, and template

| Profile | Kind | Host | Assessment | Dialect evidence | Tags |
| --- | --- | --- | --- | --- | --- |
| Dockerfile | `domain_format` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | — |

### Testing frameworks

| Profile | Kind | Host | Assessment | Dialect evidence | Tags |
| --- | --- | --- | --- | --- | --- |
| Playwright for JavaScript | `framework_projection` | javascript | planned (documentation only) | — | — |
| Playwright for TypeScript | `framework_projection` | typescript | planned (documentation only) | — | — |
| Jest for JavaScript | `framework_projection` | javascript | planned (documentation only) | — | — |
| Jest for TypeScript | `framework_projection` | typescript | planned (documentation only) | — | — |
| Vitest for TypeScript | `framework_projection` | typescript | planned (documentation only) | — | — |
| pytest | `framework_projection` | python | planned (documentation only) | — | — |
| JUnit | `framework_projection` | java | planned (documentation only) | — | — |
| xUnit.net | `framework_projection` | csharp | planned (documentation only) | — | — |

### Hardware design

| Profile | Kind | Host | Assessment | Dialect evidence | Tags |
| --- | --- | --- | --- | --- | --- |
| VHDL | `language` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | — |
| Verilog | `language` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | — |
| SystemVerilog | `language` | — | candidate: detected supported, parsed fallback, symbols fallback, semantic unavailable, benchmarked unavailable | — | — |
