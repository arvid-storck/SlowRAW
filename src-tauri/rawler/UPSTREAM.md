# Rawler

Cargo downloads the official dnglab commit in `upstream-revision` and applies
`nikon-he.patch` under its build directory. The patch adapts experimental Nikon
HE/HE* decoding from [dnglab PR #835](https://github.com/dnglab/dnglab/pull/835).
The first build after cleanup needs Git and network access. Later builds reuse
the source and profiles for the same revision and patch.

To update while the patch is needed, change the pinned commit, rebase the patch
onto it and update this crate's manifest for any upstream dependency changes.

When official Rawler supports both modes, switch the dependency in
`src-tauri/Cargo.toml` to the official repository:

```toml
rawler = { git = "https://github.com/dnglab/dnglab.git", rev = "<fixed revision>" }
```

Remove the local `rawler` directory and workspace exclusion, then update the
lockfile and run the sample test for each compression type.

```powershell
cargo update --manifest-path src-tauri/Cargo.toml -p rawler
$env:SLOWRAW_NIKON_HE_SAMPLE = "C:/path/to/sample.NEF"
cargo test --manifest-path src-tauri/Cargo.toml --lib nikon_high_efficiency_nef_develops -- --ignored
```

Public Nikon Z6 III samples are available for [HE](https://raw.pixls.us/getfile.php/7810/nice/nikon-he.NEF)
and [HE*](https://raw.pixls.us/getfile.php/7814/nice/nikon-he-star.NEF).
