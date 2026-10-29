### Bugs

- Automatic ExifTool preparation repeatedly restarted `tauri dev`.

  Status: fix implemented; awaiting developer verification (2026-10-29).
  Tauri watched the staging files created by the build itself and interrupted
  preparation. `.taurignore` now excludes ExifTool staging and runtime directories.
  Developer check: start `npm start` after `clean:deep` and confirm that ExifTool
  preparation completes without repeatedly restarting the Rust build.

- Editing any image setting returned the development app to the home screen.

  Status: fix implemented; awaiting developer verification (2026-10-29).
  Tailwind scanned photo sidecars under the project root. Its Vite integration
  sent a full page reload whenever autosave changed one, clearing editor state.
  Scoped CSS scanning to src/ and excluded metadata sidecars from Vite watching.
  An isolated regression test reproduced the reload before the fix and passes
  afterward. Developer check: start npm start, open the Ricoh and Nikon samples,
  change settings in several panels and confirm that the editor stays open.

- Lens correction headers moved when Chromatic Aberration wrapped onto two lines.

  Status: fix implemented; awaiting developer verification (2026-10-29).
  Switch tracks now retain their width; the three correction headers reserve
  equal minimum height. Developer check: narrow the Crop & Transform panel and
  compare the wrapped label, switch positions and amount sliders.

- Neutral built-in lens tables suppressed Lensfun; embedded correction controls appeared disabled.

  Status: fix implemented; awaiting developer verification (2026-10-29).
  Auto rejects neutral profiles and falls back to Lensfun. Controls inspect
  embedded coefficients as well as Lensfun values. Manual source selection is
  preserved through reload/export. Synthetic neutral-table and real RAW tests pass.
  Developer check: select each manual source, reopen the image, export, and
  verify that zero correction strength and Off preserve the uncorrected image.

- Camera ISO was missing or taken from ISOSpeed in parts of the interface.

  Status: fix implemented; awaiting developer verification (2026-10-29).
  Standard camera ISO is shared across readouts and sorting. The supplied
  Nikon Z6 III NEF reports ISO 4000, ahead of its internal MakerNote value.
  Developer check: compare the metadata panel, toolbar, library and ISO sorting.

- Export presets lost settings after restart

  Status: fix implemented; awaiting developer verification.

  The native ExportPreset schema omitted bit depth, output profile, timestamps and destination fields, so frontend values could disappear after reloading. New codec and author fields would have had the same problem.

  Fix: preserve additional preset fields through serde flattening. Frontend presets include per-format encoding settings and the author option.

  Developer check: save PNG16 and wide-profile presets, timestamp preservation, an original-folder destination/subfolder, and explicit WebP/JXL modes. Restart the app and verify each preset and the resulting exported file.
- Export discarded AI source-type assertions

  Status: fix implemented for standard IPTC source-type assertions; awaiting developer verification.

  The metadata-off path returned without transferring XMP. Other modes also did not transfer XMP source-type assertions.

  Fix: independently preserve Digital Source Type in all six export containers, with author/copyright copied only when requested. Optional camera metadata remains controlled separately.

  Developer check: export known AI-marked files to each format with metadata enabled and disabled. Inspect Digital Source Type with a metadata reader and check author/copyright and GPS behavior.
- Highlights changed abruptly near zero
- Camera calibration did not use the interpolated profile
- Batch analysis could retain too many RAW decodes
- Decoded cache could reuse externally changed files
- Importing does not work when importing 50000 images and more, and when rescanning, it says "database is locked"

  Status: fix implemented; awaiting developer verification (2026-10-29).
  Metadata reads now run outside short immediate write transactions; folder scans
  are serialized and overlapping roots deduplicated. Rescans preserve committed
  data. A separate 60,000-file fixture passes with a concurrent database writer.
  Developer check: repeat the real 60,000-photo import, verify its final row count,
  then rescan while browsing and updating ratings.
- When shoosing auto in lens profile, a ricoh gr ii gets the profile of a samyang 500mm f somthing

  Status: fix implemented; awaiting developer verification (2026-10-29).
  Fuzzy matching requires matching optical numbers, maker and an unambiguous
  high similarity score. Official ExifTool supplies lens identities;
  fixed-lens cameras match via their camera mount. The Ricoh GR II regression
  selects the ricohGR profile and rejects the Samyang candidate.
  Developer check: select Auto on a real GR II RAW and compare correction with
  the appropriate fixed-lens profile. The supplied R0019057.DNG selects the
  ricohGR profile in the real-file regression test.
- Olympus/OM System: distorsion och färgfel stöds, men inbyggd vinjetteringskorrigering saknas.
  
  Panasonic/Leica: den inbyggda modellen korrigerar bara distorsion.
  
  Fujifilm: vissa tabellformat stöds; andra avvisas trots att ExifTool kan läsa värdena.
  
  Canon: egna inbyggda profilmodeller saknar implementation och använder Lensfun som reserv.
  
  Sony: jag hittade ingen motsvarande saknad koefficient i de modeller vi stöder.

### Things to change

- Fix docs/DEVELOPMENT.md to show what it does, no need to write what is does not do.
  Implemented 2026-10-29: positive documentation ownership and contributor guidelines.
- Update exif reader to one that reads lens profiles
  Implemented 2026-10-29: official ExifTool 13.60, with its persistent process and Windows runtime packaging.
- Use the built in lens profile when avawable lensfun as a spare
  Implemented 2026-10-29 for supported DNG opcodes and Sony, Fujifilm,
  Olympus/OM System, Panasonic/Leica and Nikon models. Corrections reuse the
  shared ExifTool metadata cache; the lens panel displays the selected source.
  Lensfun supplies fallback for unsupported or absent embedded models. Coverage
  and model limitations are recorded in CODEMAP; real-camera verification remains.

### Things to consider (Do not implelent unless you need to or want to do i and its benfits the program)

- Job Queue with Cancellation Tokens to Stop CPU Spikes

  When users drag sliders quickly, you want to abort outdated background tasks immediately so your cpu does not get choked by a backlog of old preview renders. Using a cancellation token allows you to signal active worker threads to terminate their loops instantly.

  License: MIT

  Code implementation reference: https://github.com/tokio-rs/tokio/blob/master/tokio-util/src/sync/cancellation_token.rs

  How to use it: Instantiate a cancellation token, clone it, and pass it into your background worker. When a new slider event fires, call cancel on the old token and spin up a new task. Inside your heavy processing loops, periodically check if the token has been cancelled to exit early.
- Zero-Copy IPC via Tauri Custom Protocol

  Instead of converting your rendered image buffers into base64 strings or json arrays to pass across the tauri bridge, which hurts frontend performance, you can register a custom uri scheme. The frontend loads the image using a standard html tag, and the rust backend streams the raw bytes directly from memory.

  License: MIT / Apache 2.0

  Code implementation reference: https://github.com/tauri-apps/tauri/blob/dev/examples/streaming/main.rs
  
  How to use it: Check how the streaming example registers protocol handlers to intercept custom urls and return raw byte vectors directly from an in-memory cache, bypassing IPC serialization.
- Remove unnecessary validation tests
- Higher quality x-trans decoder, consider "Markesteijn 1-pass"?

### Need to be investigate

- Test SlowRAW on linux
- Test SlowRAW on mac
- Test more file formats for compatability

### Questions about the code
