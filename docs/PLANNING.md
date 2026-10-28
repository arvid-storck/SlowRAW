### Bugs

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
- When shoosing auto in lens profile, a ricoh gr ii gets the profile of a samyang 500mm f somthing

### Things to change

- Fix docs/DEVELOPMENT.md to show what it does, no need to write what is does not do.
- Use the built in lens profile when avawable lensfun as a spare

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
