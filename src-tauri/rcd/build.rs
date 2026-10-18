use std::{env, path::PathBuf, process::Command};

fn run(command: &mut Command) {
    assert!(
        command
            .status()
            .expect("Git is required to fetch librtprocess")
            .success(),
        "Cannot prepare librtprocess; check Git/network access and the pinned revision"
    );
}

fn main() {
    let revision = include_str!("upstream-revision").trim();
    assert!(revision.len() == 40 && revision.bytes().all(|c| c.is_ascii_hexdigit()));
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let source = manifest.parent().unwrap().join("target/rcd-source");
    if !source.join(".git").exists() {
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        run(Command::new("git")
            .args([
                "clone",
                "--no-checkout",
                "https://github.com/CarVac/librtprocess.git",
            ])
            .arg(&source));
    }
    if !Command::new("git")
        .arg("-C")
        .arg(&source)
        .args(["cat-file", "-e", revision])
        .status()
        .unwrap()
        .success()
    {
        run(Command::new("git")
            .arg("-C")
            .arg(&source)
            .args(["fetch", "--depth", "1", "origin", revision]));
    }
    run(Command::new("git")
        .arg("-C")
        .arg(&source)
        .args(["checkout", "--detach", revision]));
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++14")
        .opt_level(3)
        .warnings(false)
        .define("LIBRTPROCESS_STATIC", None)
        .include(source.join("src/include"))
        .file(source.join("src/demosaic/rcd.cc"))
        .file(source.join("src/demosaic/border.cc"))
        .file(manifest.join("bridge.cpp"));
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        build.define("WIN32", None);
    }
    build.compile("slowraw_rcd");
    println!("cargo:rerun-if-changed=upstream-revision");
    println!("cargo:rerun-if-changed=bridge.cpp");
}
