# ProjectAtlas @RELEASE_VERSION@

This archive contains the ProjectAtlas @RELEASE_VERSION@ runtime for Windows.

## Verify and use this release

Run these ordinary commands after installation:

```powershell
projectatlas --require-version @PACKAGE_VERSION@ --version
atlas overview
projectatlas overview
```

`atlas` is the installed short command. `projectatlas` remains available when
you need to name the native runtime directly.

The installer saves its PATH entry for future processes. It cannot change the
environment inherited by an already-running host. Restart the environment-owning
launcher, Codex, or shell before relying on a newly installed bare command.

## Release channels

This archive is the @RELEASE_VERSION@ prerelease. For the stable channel, use
[v0.4.5 (stable)](https://github.com/styler-ai/ProjectAtlas/releases/tag/v0.4.5).
The matching source and release information are available at
https://github.com/styler-ai/ProjectAtlas/tree/@RELEASE_VERSION@.
