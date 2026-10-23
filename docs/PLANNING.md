
### Bugs

- None that i am aware of

### Things to change

- For every export file format, separate the following settings that is supported: color space, bits per chanel, lossy/lossless, quality (slider, not up to lossless!), effort (or eqvivalent)
- Restrict prophoto rgb to lossless or max quality lossy only with appropiet bits per chanel (12-bit+)
- Move Watermark under Advanced export settings
- Add compiling help and common libaries update on README.md


### Things to maybe change

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
- More file formats

### Need to figure out

- Is the rendering path realy with zero noise reduction and sharpening?
