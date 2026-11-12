use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
};

const UPSTREAM: &str = "https://github.com/dnglab/dnglab.git";
const REVISION: &str = include_str!("upstream-revision");
const PATCH: &[u8] = include_bytes!("nikon-he.patch");

fn git(directory: &Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("git")
        .current_dir(directory)
        .args(["-c", "core.autocrlf=false"])
        .args(args)
        .status()?;
    if !status.success() {
        return Err(format!("git {} exited with {status}", args.join(" ")).into());
    }
    Ok(())
}

fn prepare_source(out: &Path, manifest: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let revision = REVISION.trim();
    assert!(revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let patch_hash = blake3::hash(PATCH).to_hex();
    let cache = out.join(format!(
        "upstream-{}-{}",
        &revision[..12],
        &patch_hash[..12]
    ));
    if cache.join("src/lib.rs").is_file() {
        return Ok(cache);
    }

    println!(
        "cargo:warning=Fetching official Rawler at {}",
        &revision[..12]
    );
    let stage = tempfile::tempdir_in(out)?;
    let directory = stage.path();
    git(directory, &["init", "--quiet"])?;
    git(directory, &["remote", "add", "origin", UPSTREAM])?;
    git(
        directory,
        &[
            "sparse-checkout",
            "set",
            "rawler/src",
            "rawler/data/cameras",
            "rawler/data/lenses",
            "rawler/data/profiles",
        ],
    )?;
    git(
        directory,
        &["fetch", "--quiet", "--depth=1", "origin", revision],
    )?;
    git(directory, &["checkout", "--quiet", "--detach", revision])?;
    let mut apply = Command::new("git");
    apply
        .current_dir(directory)
        .args(["-c", "core.autocrlf=false", "apply"])
        .arg(manifest.join("nikon-he.patch"));
    if !apply.status()?.success() {
        return Err("Could not apply nikon-he.patch to the pinned Rawler revision".into());
    }
    fs::rename(directory.join("rawler"), &cache)?;
    Ok(cache)
}

fn join_definitions(out: &Path, kind: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut output = fs::File::create(out.join(format!("{kind}.toml")))?;
    for definition in glob::glob(&format!("./data/{kind}/*/**/*.toml"))? {
        let path = definition?;
        let text = fs::read_to_string(&path)?;
        text.parse::<toml::Value>()
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if kind == "cameras" {
            writeln!(output, "[[cameras]]")?;
        }
        writeln!(output, "{text}")?;
    }
    Ok(())
}

fn prepare() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let source = prepare_source(&out, &manifest)?;
    let entry = fs::read_to_string(source.join("src/lib.rs"))?;
    // The include runs at the crate root. The upstream header contains inner docs and lints.
    let body = entry
        .find("\nuse ")
        .ok_or_else(|| io::Error::other("Unrecognized Rawler crate header"))?;
    fs::write(source.join("src/slowraw-lib.rs"), &entry[body..])?;

    env::set_current_dir(&source)?;
    join_definitions(&out, "cameras")?;
    join_definitions(&out, "lenses")?;
    println!("cargo:rustc-env=SLOWRAW_RAWLER_SOURCE={}", source.display());
    Ok(())
}

fn main() {
    for file in ["build.rs", "upstream-revision", "nikon-he.patch"] {
        println!("cargo:rerun-if-changed={file}");
    }
    if let Err(error) = prepare() {
        panic!("Could not prepare Rawler: {error}. Check Git and network access.");
    }
}
