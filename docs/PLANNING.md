### Bugs

- Filter baserat på nyckelord borde vara AND
- Smart grupper borde kunna ha mer komplexa filter (filter 1 och filter 2 och ...)
- Gör det möljligt att flytta smarta grupper som andra grupper in i mappar(grupp)
- Vissa filer visar brännvidd som "700/10" och öppning som "30/10"
- Jag saknar vissa bilder vid import
- Vid import ska den lista hur många bilder som kommer importeras och sen visa när den importerar "Importing in progress\n x images of y imported"
- Ta bort standad taggar
- Vid misslycket import av bild, skriv i en fil: namnet, position, varför den misslyckades 

#### Nikon vignetting

- The current Nikon gain polynomial produces negative values for some
  Z6 III frames with the NIKKOR Z 24-120mm f/4 S. SlowRAW skips the
  affected vignette curve and can still use the built-in distortion
  correction. Compare the gain against a Nikon reference rendering to
  determine the correct interpretation.

#### Code review findings (2026-11-12)

- **File renames can separate photos from their sidecars.** `rename_files`
  renames all photos first and then their sidecars. If any rename fails, it
  logs the error, continues with the remaining operations and returns success.
  A locked photo or XMP can leave the files under different names. The
  frontend also uses the shortened list of successes by original position,
  so it can select the wrong photo after a partial failure.
  See `src-tauri/src/file_management.rs` and `src/hooks/useFileOperations.ts`.
- **A failed import can leave a destination file that blocks retry.**
  Copy and move import write the photo before copying its sidecars. If
  copying an XMP file fails after the photo was written, the photo remains
  in the destination. The next attempt sees the photo and counts it as
  already present, skipping the missing sidecar.
  See `import_files` in `src-tauri/src/file_management.rs`.
- **Flatpak offline build inputs are stale.** The npm manifest prefetches Vite
  6.3.5 and lacks the locked 8.2.4 archive. A clean native build also needs
  Rawler and librtprocess from Git, and the Flatpak manifest only stages
  ExifTool and the npm and Cargo manifests. Refresh the offline sources and
  verify a clean sandboxed Flatpak build on Linux.
  See `packaging/io.github.arvid-storck.SlowRAW.yml`,
  `src-tauri/rawler/build.rs` and `src-tauri/rcd/build.rs`.

#### Performance findings (2026-11-12)

- **Deep scrolling repeats the filtered count and skips earlier rows.**
  Every window calls `COUNT(*)` and a separate query with `OFFSET`. Measure
  filtered scrolling near the end of a large catalog. Preserve scrollbar
  jumps to arbitrary positions when considering changes.
  See `catalog_query_page` in `src-tauri/src/catalog.rs`.
- **Thumbnail workers reread settings for every file.** Each thumbnail job
  reads and parses `settings.json` and runs the migration checks. Profile
  cold and warm thumbnail batches before caching settings across jobs.
  See `start_thumbnail_workers` in `src-tauri/src/file_management.rs`.
- **Export can scan the same source again for provenance.** After decoding,
  the required-metadata pass reopens the source and can scan the whole
  memory-mapped file for XML and provenance markers. Profile large RAW
  batches before consolidating this work with the metadata cache. Preserve
  AI provenance and the existing compressed-XMP fallbacks.
  See `required_xmp` in `src-tauri/src/export_metadata.rs`.

#### Fixed



### Things to change

Easy


Hard and/or risky

- Rename variables, fuctions and classes to to camel case, and upper camel case. But only to make it uniform, not when a langage has a recomended style already.

### Remaining code reduction

- Share thumbnail placeholder and layer-transition logic between the library
  grid and list in `src/components/panel/library/LibraryItems.tsx` and the
  filmstrip in `src/components/panel/Filmstrip.tsx`. Compare loading and
  fade behavior when a cell is reused or its thumbnail is evicted.
- Share the repeated per-pixel blend code in
  `src-tauri/src/panorama_utils/stitching.rs`. Verify both stitching
  directions and preserve overlap and seam ownership.
- Review the repeated parametric-mask coordinate and sampling loops in
  `src-tauri/src/mask_generation.rs`. Compare mask output with rotated
  images and at different preview scales before consolidating them.
- Compare the drag-and-drop handling for the two panel regions in
  `src/components/panel/SidePanelArea.tsx`. Check drop targets and
  panel ordering before sharing the common handlers.

### Things to consider (Do not implelent unless you need to or want to do and its benfits the program)

- Remove unnecessary validation tests
- Higher quality x-trans decoder, consider "Markesteijn 1-pass"?

### Need to be investigate

- Test SlowRAW on linux
- Test SlowRAW on mac
- Test more file formats for compatability
- If built in lens correction data exist that is not used currently

### Questions about the code

- How does the queue part work?
- Vad är det som tar tid vid inläsningen av metadatan när jag först öppnas en bild?
