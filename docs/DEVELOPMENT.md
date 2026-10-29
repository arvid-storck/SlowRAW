# Development guide

## Documentation responsibilities

- README.md: project purpose, runtime requirements, basic development tools, compilation, packaging and updating common libraries and the RAW decoder.
- docs/CODEMAP.md: current architecture, data flow, processing mechanisms and module ownership.
- docs/CHANGELOG.md: dated summaries of completed changes, describing each day's final result.
- docs/PLANNING.md: developer-owned bug tracking and priorities. AI records newly discovered bugs and adds fix notes for developer verification. The developer owns verification and existing entries.
- This guide: contributor workflow, validation and documentation ownership, with links to build instructions and implementation details.

## Contributor workflow

Read CODEMAP before tracing a feature. Preserve unrelated work. When an IDE integration is available, use open IDE buffers as the source of truth. Components present controls and assemble requests; hooks/stores own frontend state; Rust owns native operations and processing. Reflect the capabilities of the linked encoder.

Remove obsolete code, dependencies and files when replacing an implementation,
and check remaining references. Record the final implementation in CODEMAP and
the completed change in CHANGELOG. Keep build and dependency instructions in README.

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
