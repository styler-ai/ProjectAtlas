# ProjectAtlas @RELEASE_VERSION@

This archive contains the ProjectAtlas @RELEASE_VERSION@ runtime.

## Verify and use this release

Run these ordinary commands after installation:

```text
projectatlas --require-version @PACKAGE_VERSION@ --version
projectatlas init
atlas overview
projectatlas overview
```

`atlas` is the installed short command. `projectatlas` remains available when
you need to name the native runtime directly.

The installer makes the commands available in its own process, but cannot change
the environment inherited by an already-running host. On Windows, it saves its
PATH entry for future processes; restart the environment-owning launcher, Codex,
or shell before relying on a newly installed bare command. On Linux and macOS,
ensure `~/.local/bin` is on your shell PATH, then start a new shell before
relying on a newly installed bare command.

## Release channels

@RELEASE_CHANNEL_GUIDANCE@
The matching source and release information are available at
https://github.com/styler-ai/ProjectAtlas/tree/@RELEASE_VERSION@.
