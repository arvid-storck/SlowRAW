# Development guide

## Documentation responsibilities

- README.md: project purpose, runtime requirements, basic development tools, compilation, packaging and updating common libraries and the RAW decoder.
- docs/CODEMAP.md: current architecture, data flow, processing mechanisms, module ownership and all development tools and compilation.
- docs/CHANGELOG.md: dated summaries of completed changes, describing each day's final result.
- docs/PLANNING.md: developer-owned bug tracking and priorities. AI records newly discovered bugs and adds fix notes for developer verification. The developer owns verification and existing entries.
- docs/DEVELOPMENT.md: contributor workflow, validation and documentation ownership.

## Contributor workflow

### Documents

Respect the current document responsabilites, There should be no need for any other documents. Do not add any loose documents randomly in the code.

Consider reading CODEMAP.md to learn the structure and file resonability.

Keep the documents updated and relevant to the project. Especially CODEMAP if any of the files is changed/added/removed to keep it up to date.

Keep compiling simple and understandable for the users, no need for details. This is for both README and CODEMAP.

### Code style goals

Remove obsolete code, dependencies and files when replacing an implementation, and check remaining references.

Whenever possible, reduce the amount of code assuming its redundant.

Only implent a feature that works for all of the supported OS, if a feature does not work for all OS, DO NOT IMPLEMENT IT.

Try to reuse existing functions and dont write several functions that do almost the same thing.

Dont add comments unless the function is completly unredable, but if the funktion is unredable, consider writing it in a more clearer way.

If a problem can be solved with a well made external libary, concider using it, unless a custom solution is way esier and more efficent. Only use well regarded external libary that is proven to work and tested by time and/or other software and users.

Make the implementation work for all supported OS with one path only if possible, try to not make a special version for one OS.

## Verification

Use focused checks for changed behavior.

```powershell
npm run typecheck
npm run lint
npm run i18n:check
npm run i18n:lint
npm run build
node scripts/test-color-workflow.mjs
node scripts/test-export-settings.mjs
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --workspace --lib -- --test-threads=1
cargo build --manifest-path src-tauri/Cargo.toml --bin SlowRAW
```

Windows monitor-profile diagnostics can be run against the current desktop:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib windows_display::tests -- --ignored --nocapture
```
