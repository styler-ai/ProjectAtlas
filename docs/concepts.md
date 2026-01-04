# Concepts

ProjectAtlas is a lightweight way to keep structural intent close to the codebase and visible to agents.

## Folder purpose files

Each folder has a `.purpose` file with a single-line summary. This keeps intent discoverable from the tree view.
ProjectAtlas treats missing `.purpose` files as a lint error when `--strict-folders` is enabled.

## File Purpose headers

Each tracked source file carries a one-line `Purpose:` header near the top of the file. The comment
style is configurable per extension (Javadoc blocks, block comments, or line comments).

This allows:

- quick file selection before deep indexing
- early detection of duplicate responsibilities
- consistent, low-overhead documentation

## Non-source summaries

Some files cannot safely carry inline Purpose headers (JSON, lockfiles, images, generated outputs). Those live in
`.projectatlas/projectatlas-nonsource-files.toon`, which is merged into the atlas at map time. The generated
`projectatlas.toon` still shows a single `files[]` list, but the header distinguishes
`tracked_source_files`, `tracked_nonsource_files`, and the combined `tracked_files_total`.

## Health signals

ProjectAtlas surfaces:

- missing or invalid Purpose summaries
- duplicate summaries across files or folders
- untracked assets outside approved roots

These signals are meant to prompt cleanup before the structure drifts.
