# Development guide

## Documentation responsibilities

- README.md: project purpose, runtime requirements, basic development tools, compilation, packaging and updating common libraries and the RAW decoder.
- docs/CODEMAP.md: how the current implementation works, including data flow and module ownership. Avoid behavior already apparent from using the UI.
- docs/CHANGELOG.md: dated completed changes. Omit changes reverted within the
  same calendar day and describe that day's final result.
- docs/PLANNING.md: developer-owned bug tracking. AI may append newly discovered bugs and annotate fixes for developer verification. AI must not add feature plans, change priorities, delete entries or mark fixes developer-verified.
- This guide: contributor workflow, validation and documentation ownership. Link to build instructions and implementation details instead of duplicating them.

## Contributor workflow

Read CODEMAP before tracing a feature. Preserve unrelated work. When an IDE integration is available, use open IDE buffers as the source of truth. Components present controls and assemble requests; hooks/stores own frontend state; Rust owns native operations and processing. Reflect the capabilities of the linked encoder.

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
cargo test --manifest-path src-tauri/Cargo.toml --workspace --lib
cargo build --manifest-path src-tauri/Cargo.toml --bin SlowRAW
```

Windows monitor-profile diagnostics can be run against the current desktop:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib windows_display::tests -- --ignored --nocapture
```
