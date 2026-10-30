### Bugs
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
- Olympus/OM System: distorsion och färgfel stöds, men inbyggd vinjetteringskorrigering saknas.
  
  Panasonic/Leica: den inbyggda modellen korrigerar bara distorsion.
  
  Fujifilm: vissa tabellformat stöds; andra avvisas trots att ExifTool kan läsa värdena.
  
  Canon: egna inbyggda profilmodeller saknar implementation och använder Lensfun som reserv.
  
  Sony: jag hittade ingen motsvarande saknad koefficient i de modeller vi stöder.
- Use in nikon raw file for vigetting "Vignette Coefficient1", "Vignette Coefficient2", "Vignette Coefficient3"
- Har Sony och olympus verkligen Distortion, Chromatic Aberration, och vignetting inbyggt i metadatan? Verkar inte troligt
- Vissa bilder laddas upp och ned vid import?

### Things to change

- Lägg tillbacka titeln "Dates", Gör att titlarna inte lyser
- Increase priority or threads to build cache when importing images, (thumbnails and/or EXIF loading is slow but that is all i can see)

### Things to consider (Do not implelent unless you need to or want to do and its benfits the program)

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
- If built in lens correction data exist that is not used currently

### Questions about the code
