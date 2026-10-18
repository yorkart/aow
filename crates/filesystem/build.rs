use std::{env, path::PathBuf, process::Command};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed=../../vendor/ripgrep");
    println!("cargo:rerun-if-changed=../../scripts/ripgrep.mjs");
    let status = Command::new("node")
        .arg(root.join("scripts/ripgrep.mjs"))
        .arg(env::var("TARGET").unwrap())
        .arg(&output)
        .status()
        .expect("Node.js is required to prepare the bundled ripgrep");
    assert!(status.success(), "could not prepare bundled ripgrep");
    println!(
        "cargo:rustc-env=AOW_BUILD_RG={}",
        output.join("bin/rg").display()
    );
}
