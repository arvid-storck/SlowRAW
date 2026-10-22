# Catalog-focused build

This build opens directly in the catalog. The disk-browser tree, pinned-folder
navigation, folder preloading, splash/community landing screen, and duplicate
quick-filter controls are no longer part of the library workflow. Editor panels
are hidden in the catalog; export opens only when requested. Existing
editing, manual masks, noise reduction, albums, import, and export remain.

## Filtering

- Cameras, lenses, ratings (including 0), tags, colors, and file types use
  multi-select checkbox lists. No Ctrl/Shift modifier is needed.
- Selections within one list are ORed; different lists are ANDed.
- An empty selection means all values. Clear filters also clears the date.
- Smart groups store all selections. Right-click → Edit filters updates an existing
  group without duplicating it; ordinary groups render recursively and expand on
  creation. Older single-value smart groups are normalized when opened.
- Dates are chosen in the year/month/day tree. Tags come from indexed user tags.

## Bounded catalog loading

SQLite performs filtering and sorting over the catalog before paging. Each
page holds at most 500 images. A filtered SQL count, read in the same transaction,
provides page X of Y and navigation boundaries. The header also shows catalog total.
Selection and bulk operations apply to the loaded page, not every search result.
Previous/Next controls are at the start of the library header; there is no large Catalog button. Ordinary manually populated
albums retain their existing loading behavior; smart groups use catalog paging.

Catalog initialization and query loading live in `useCatalog.ts`, mounted by App,
so hiding the filter panel does not stop loading. Search is debounced. Late replies are ignored after filter/page/album changes.
Thumbnail maps are pruned on page changes, preserving the currently edited image.
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

The older standalone catalog/cache test and smoke-fixture scripts are no longer
in this tree. Manually verify a full page and a final partial page, combined
filters, rating changes that remove images from a filter, and thumbnail loading
after returning to an evicted page. Use a separate test catalog and copied photos.

Passing the production build is not equivalent to passing TypeScript checks.
Large-collection end-to-end performance still needs measurement on representative
photo collections; bounded paging is not a measured speed claim.
