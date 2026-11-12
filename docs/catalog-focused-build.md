# Catalog-focused build

This build opens directly in the catalog. The disk-browser tree, pinned-folder
navigation, folder preloading, splash/community landing screen, and duplicate
quick-filter controls are no longer part of the library workflow. Editor panels
are hidden in the catalog; export opens only when requested. Existing
editing, manual masks, noise reduction, albums, import, and export remain.

## Filtering

- Cameras, lenses, ratings (including 0), tags, colors, and file types use
  multi-select checkbox lists. No Ctrl/Shift modifier is needed.
- Filter lists fill the available panel width and long values wrap within it.
- Selections within one list are ORed; different lists are ANDed.
- An empty selection means all values. Clear filters also clears the date.
- Smart groups store all selections. Right-click → Edit filters updates an existing
  group without duplicating it; ordinary groups render recursively and expand on
  creation. Older single-value smart groups are normalized when opened.
- Dates are chosen in the year/month/day tree. Tags come from indexed user tags.

## Bounded catalog loading

SQLite filters and sorts the catalog, with newest capture date first by default.
A filtered count sets the size of the virtual grid. Scrolling loads up to three
batches of 166 images around the viewport and evicts earlier batches. Selection
and bulk operations work on loaded images. Smart groups use the same scrolling
window. Ordinary albums load their own members.

Catalog initialization and query loading live in `useCatalog.ts`, mounted by App,
so hiding the filter panel does not stop loading. Search is debounced. Late
replies are ignored after a window or query change. Thumbnail maps are pruned
when the window moves, preserving the currently edited image. The Build Cache
job in Settings warms thumbnail files for the full indexed catalog.
Database queries and facet aggregation run on blocking workers rather than the
UI thread. Integrity checking is performed once per database per process, not
for each filter request. Tag deduplication happens inside SQLite.

Add folder explicitly adds a folder to the catalog. Rescan folders is incremental
and skips unchanged image/sidecar timestamps. Removing a catalog folder leaves
the actual files untouched. Import offers its own destination selection because
there is no longer an active filesystem folder in the library.

## Validation

```powershell
npm run typecheck
npm run lint
npm run i18n:check
npm run i18n:lint
node scripts/test-catalog.mjs
cargo test --manifest-path src-tauri/Cargo.toml --lib catalog::tests
npm run build
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-portable.ps1 -OutputDirectory artifacts-catalog-final
```

The packaging script rejects failed builds and refuses to replace an existing
portable output directory. Choose a fresh output directory for each build.
Keep the old portable folder and its database until migration is verified.

Use a separate test catalog and copied photos to verify scrolling across
multiple windows and back, a final partial row, combined filters and rating
changes that remove images from a filter.
