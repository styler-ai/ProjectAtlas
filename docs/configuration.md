# Purpose: Document ProjectAtlas configuration files scan policy and legacy import settings.

# ProjectAtlas Configuration

ProjectAtlas reads `projectatlas.toml` or `.projectatlas/config.toml`. All paths are relative to the config file.

```toml
[project]
root = "."
# Optional compatibility export path used only by `projectatlas map`.
map_path = ".projectatlas/projectatlas.toon"
nonsource_files_path = ".projectatlas/projectatlas-nonsource-files.toon"

[scan]
# Shortened documentation example. `projectatlas init` writes the complete
# registry-derived default; see docs/language-support.md for every rule.
source_extensions = [
  ".rs", ".py", ".js", ".ts", ".tsx", ".java", ".go", ".toml"
]
exclude_dir_names = [".git", ".projectatlas", ".venv", "__pycache__", "node_modules", "dist", "build", "target"]
exclude_dir_suffixes = [".egg-info"]
exclude_path_prefixes = []
non_source_path_prefixes = []
max_scan_lines = 80
text_index_max_bytes = 2000000

[scan.language_overrides]
# ".m" = "objective-c"
# "BUILD" = "makefile"

[purpose]
default_style = "javadoc"
line_comment_prefixes = ["//", "#", "--", ";"]
# styles_by_extension = { ".go" = "line-comment", ".c" = "block-comment" }

[summary_rules]
ascii_only = true
no_commas = true
max_length = 140

[untracked]
allowed_filenames = []
allowlist_dir_prefixes = []
allowlist_files = []
asset_allowed_prefixes = []
asset_extensions = [".png", ".jpg", ".jpeg", ".svg", ".gif", ".webp", ".ico", ".pdf", ".ttf", ".woff", ".woff2"]
```

`projectatlas init` writes the complete accepted default-core source-extension
set, not the shortened example above. That default preserves the 0.3.26 scan
surface and does not silently admit data-like optional-catalog extensions when
the broad parser pack is absent. Existing explicit lists remain explicit on
upgrade. Adjust `scan.source_extensions` only when a project needs a narrower or
broader compatibility-map surface; the verified optional-pack lifecycle supplies
its accepted additional scan rules while enabled. The generated
[language support matrix](language-support.md) is the authoritative detector and
capability inventory.

`projectatlas settings` and MCP `atlas_settings` expose the same compact,
content-free diagnostics. The report includes schema/migration compatibility,
the active complete publication generation, linked SQLite and compile-option
identity, the validated filesystem and WAL/synchronous/busy/checkpoint/statistics
profile, a bounded actionable non-complete coverage sample, typed lexical/FTS/
semantic/hybrid search readiness, the optional-parser lifecycle, and separate
language-registry, accepted-set, semantic-provider, and current semantic-relation
contract digests. Settings is read-only: it does not migrate, scan source, build
an index, load parser assets, or record telemetry. It does not inline the complete
per-language matrix; use the generated matrix when every row is required. Lexical
readiness requires one complete nonzero publication with a validated contract
fingerprint; invalid persisted fingerprint text is omitted and reported as invalid
rather than reproduced through the diagnostic surface. FTS acceleration additionally
requires equal transaction-owned source/projection revisions; explicit settings
diagnostics also compare rebuildable FTS document identities with authoritative
`file_texts`. A mismatch disables acceleration without disabling lexical fallback.
Search patterns are capped at 64 KiB and path globs at 4 KiB before matcher
construction. Semantic and hybrid modes remain explicitly unavailable until a
compatible semantic generation is installed, built, and ready.

### Optional broad parser pack

The default core remains self-contained. Normal startup, scanning, configuration,
and MCP use do not download, install, open, hash, or start optional parser-pack
assets. A project opts in only through the explicit lifecycle commands:

```powershell
projectatlas parser-pack verify --archive <completed-pack.tar.zst>
projectatlas parser-pack install --archive <completed-pack.tar.zst>
projectatlas parser-pack enable --artifact <artifact-manifest-blake3>
projectatlas parser-pack update --archive <completed-pack.tar.zst>
projectatlas parser-pack status
projectatlas parser-pack disable
projectatlas parser-pack remove
```

`verify` inspects a completed archive without installing it. On Windows it holds
the pack lifecycle lease while packaged fixtures run, then removes the temporary
artifact profile and access grant before returning; cleanup failure makes verification
fail. The user store may retain only the stable coordination lease file; verification
creates no logical pack root, slot, or project selection. `install` publishes one
immutable versioned slot but does not select it. A
successful atomic slot publication transfers profile-cleanup ownership to that slot;
every failed or reused temporary publication makes one explicit bounded cleanup
attempt before its extraction is discarded. If that attempt fails, the operation
fails, publishes no slot or proof, and makes one best-effort retry during unwind
while the exact broker and extraction still exist. `enable` writes the exact
selected slot to `.projectatlas/optional-parser-pack.json` for the current
project; enabling the artifact reported by `status.rollback` is the explicit
rollback operation and retains the displaced selection as the next rollback.
`update` first verifies the currently selected slot, stages and verifies the
replacement, then atomically changes the project selection while retaining the
previous exact slot as rollback metadata. Selection publication is the commit
point: if it fails after installing a new candidate, the prior selection bytes and
rollback identity stay unchanged, while the immutable candidate remains installed
for deterministic verification and reuse by the next identical update. `disable`
removes only the project-local selection. `remove` disables the current project and removes every
slot owned by this parser-pack ID from the current user's parser-pack store; it is
idempotent and retains any slot whose platform-profile cleanup or filesystem
removal fails so the operation can be retried safely.

The user store is `%LOCALAPPDATA%\ProjectAtlas\parser-packs` on Windows,
`$XDG_DATA_HOME/projectatlas/parser-packs` (or
`$HOME/.local/share/projectatlas/parser-packs`) on Linux, and
`$HOME/Library/Application Support/ProjectAtlas/parser-packs` on macOS. The
hidden `--storage-root` option exists only for controlled verification and test
isolation; ordinary users should keep the platform default.

Windows x86-64 and Linux x86-64 require their accepted containment adapters.
Other hosts return a typed `unsupported_containment` error before archive, state,
or storage access; `disable`, `remove`, and content-free `status` remain available
for cleanup and diagnosis. A present project selection is admitted only after its
direct `.projectatlas` parent, immutable slot, manifests, digests, inventory, and
supervisor contract all verify. Lifecycle cleanup never follows a product-owned
symlink or junction leaf. As with other same-user local package managers, this
protects the stable lifecycle-owned tree and accidental or stale links; it is not
a defense against a concurrently malicious process running as the same user and
replacing filesystem entries during an operation.

`scan.language_overrides` maps one exact filename or dot-prefixed extension to an accepted canonical language ID
or compatibility alias. Exact filename selectors are case-sensitive; extension selectors are normalized to lower
case and the longest matching compound extension wins. Targets are validated against the versioned language
registry, so an unknown language or selector containing a path separator fails configuration loading instead of
silently changing parser behavior. Selection precedence is explicit override, built-in exact filename, longest
compound extension, ordinary extension, then a bounded content/dialect rule. The bounded content prefix is retained
from the scanner's existing exact hash read rather than opening every file a second time. Override changes are part
of the publication-contract fingerprint and therefore require normal derived refresh before indexed reads continue.

`project.purpose_filename` is intentionally omitted from new configs. ProjectAtlas still accepts the key as a
legacy migration override and otherwise uses `.purpose` internally only while importing old folder-purpose files
during `projectatlas scan`; new workflows should write purpose records to SQLite with `projectatlas purpose set`,
`projectatlas purpose review`, or the matching MCP tools.

ProjectAtlas inherits `.gitignore` dynamically on every scan/watch run through the Rust scanner. Do not copy
`.gitignore` entries into ProjectAtlas config just to keep them in sync; update `.gitignore` and ProjectAtlas will
honor the change the next time it scans. `scan.exclude_dir_names` and `scan.exclude_path_prefixes` are the stricter
ProjectAtlas-only ignore layer applied after the inherited `.gitignore` baseline by `projectatlas scan`,
`projectatlas map`, `projectatlas lint`, MCP `atlas_scan`, watcher refresh, and `strip-legacy-purpose`.
Use directory-name excludes for broad
generated/vendor/build folders such as `node_modules` or `target`; use path-prefix excludes for exact repository
subtrees such as `docs/api` or `app/public/generated`. Search then operates over the indexed file set and can use
literal, regex, or fuzzy matching. ProjectAtlas manual ignores add atlas-specific exclusions; they do not unignore
paths already excluded by `.gitignore`.

Keep private Memory Bank, cache, and harness state out of ProjectAtlas indexes and public Git history by listing
that local workspace state in `.gitignore`. ProjectAtlas will inherit those rules without copying tool-specific
folder names into source code or ProjectAtlas config.

Manage the manual ProjectAtlas layer with:

```powershell
projectatlas ignore list
projectatlas ignore init-gitignore
projectatlas ignore add --kind dir-name generated
projectatlas ignore add --kind path-prefix docs/api
projectatlas ignore remove --kind path-prefix docs/api
```

`projectatlas ignore init-gitignore` creates a missing project-root `.gitignore` with ProjectAtlas runtime-state
defaults. It is a no-op when the file already exists and does not require GitHub or a remote Git repository.
Project-local or personal workspace state should stay in `.gitignore`. ProjectAtlas honors those ignore rules and
does not need tool-specific folders copied into its own config. If a workflow note needs to be public, promote it
to `AGENTS.md`, `docs/`, or plugin skill documentation instead of committing private workspace memory.

During migration from legacy TOON maps, `projectatlas scan` imports purpose records only for paths still present in
the freshly indexed file set. Stale or newly excluded map rows are counted as skipped stale imports instead of
failing the first scan with a low-level SQLite no-row error.

`scan.text_index_max_bytes` caps the size of each UTF-8 file stored in SQLite for indexed text search. Oversized
files remain indexed as repository nodes, but their full text is skipped for search to keep large repositories fast
and memory bounded. Use a higher value only when the repository needs indexed search inside large generated or data
files.

Path-like entries in scan and untracked configuration are repository-relative. Absolute paths, drive-prefixed
paths, root paths, and `..` traversal are rejected before ProjectAtlas performs existence checks or lint probes.

### Purpose styles

- `purpose.default_style` controls the fallback header style (`javadoc`, `block-comment`, or `line-comment`).
- `purpose.styles_by_extension` maps specific extensions to a style.
- `purpose.line_comment_prefixes` controls which line-comment prefixes are recognized.

Example:

```toml
[purpose]
default_style = "javadoc"
line_comment_prefixes = ["//", "#", "--", ";"]

[purpose.styles_by_extension]
".go" = "line-comment"
".rs" = "line-comment"
".c" = "block-comment"
```

### Non-source file list

If you set `project.nonsource_files_path`, ProjectAtlas reads a TOON file with a `nonsource_files[]:` section. This
file is agent-maintained input for non-source summaries (configs, docs, assets) and is merged into the generated
atlas. The legacy `project.manual_files_path` key is still accepted for backward compatibility.

```
nonsource_files[]:
  path/to/file.txt,One line purpose summary
```

These entries are merged into the file list for non-source or config files that cannot carry headers.
