### Bugs

När automatic lens correction är på, när den är av så fixar det sig
- Lens correctopn automatiskt satt på går aldrog från "Waiting for Auto-Detect" till "Lens profile not found" då ingen lins profil finns
- Texten från "Lens profile not found" uppdateras inte korrekt, måste manuellt uppdatera den
- Den visar "No native gpu preview" fast det går men bara tar lite tid
- Ibland så uppdaterar den inte preview (stannar på "Native GPU preview unavailable"), funkar dock så fort jag ändrar en enda inställning eller byter bild fram ock tillbacka

Annat
- Varför används inte databasens info för att läsa in camera, lins, brännvidd, f värde?
- Låt lins och kamera namnet ta flera rader om det behövs
- På kameras nikon z6iii och 24-120mm f4 s så hittar inte den inbyggda lins profilen ibland men ibland hittas den. (Jag misstänker avrundings fel med nära 0 fel)

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




### Things to change

Easy

- Fix compiling documantation

Hard and/or risky

- Rename variables, fuctions and classes to to camel case, and upper camel case. But only to make it uniform, not when a langage has a recomended style already.

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
- Is there still android code left?
  - 2026-11-12 review. No Android-specific application code, target, or
    packaging configuration was found. Tauri's generated schemas and
    dependency lockfiles still list Android as an upstream platform.
    Shared desktop dependencies include `wgpu-core-deps-windows-linux-android`.
  - `src-tauri/src/file_management.rs` still has a generic fallback for
    platforms outside Windows, macOS and Linux in move-on-import. It is
    unreachable on the supported targets and can be reviewed for removal.
- Vad är det som tar tid vin inläsningen av metadatan när jag först öppnas en bild?
