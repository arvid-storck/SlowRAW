
### Bugs

- None that i am aware of

### Things to change

- For every export file format, separate the following settings that is supported: color space, bits per chanel, lossy/lossless, quality (slider, not up to lossless!), effort (or eqvivalent)
- Restrict prophoto rgb to (lossless or max quality lossy), 12+ bit/channel, high effort
- Move Watermark under Advanced export settings
- Add compiling help and common libaries update on README.md
- Update dev guide with better separation of responsability

  docs/CODEMAP.md : How the code currently works, but not things clear in the UI itself when using it
  
  docs/CHANGELOG.md : Changelog, but if a change is reverted within the same day it does not need to be written down
  
  docs/PLANNING.md : AI is only ever allowd to write new bugs, But please note if its fixed so the dev can later check if its fixed

  README.md : Write basic dev tools, compiling, uppdating raw fil handler package. And about the project itself
- When exporting without metadata option on, add an option to keep the author/maker or whatever its called in the same place "Remove GPS Data" is placed
- Make on/off switches clearer by making "on" with an vertical line inside the circle and "off" with an circular ring within the circle.
- When exporting without metadata, is the AI tag from ai images kept or removed? It should be kept regardless of settings. No need to note this in the UI.
  
### Things to consider

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

### Need to be investigate

- Test SlowRAW on linux
- Test SlowRAW on mac
- Test more file formats for compatability

### Need to figure out how the code works

- Is the rendering path realy with zero noise reduction and sharpening?
