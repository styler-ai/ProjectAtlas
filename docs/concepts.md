# Concepts

ProjectAtlas is a lightweight way to keep structural intent close to the codebase and visible to agents.

## Folder purpose files

Each folder has a `.purpose` file with a single-line summary. This keeps intent discoverable from the tree view.
ProjectAtlas treats missing `.purpose` files as a lint error when `--strict-folders` is enabled.

## File Purpose headers

Each tracked source file carries a one-line `Purpose:` header at the top of the file. This allows:

- quick file selection before deep indexing
- early detection of duplicate responsibilities
- consistent, low-overhead documentation

## Health signals

ProjectAtlas surfaces:

- missing or invalid Purpose summaries
- duplicate summaries across files or folders
- untracked assets outside approved roots

These signals are meant to prompt cleanup before the structure drifts.
