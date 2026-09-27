use std::{env, fs, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by Cargo"),
    );
    let bundle = manifest_dir.join("../../vt-worker/dist/vt-worker.mjs");
    println!("cargo:rerun-if-changed={}", bundle.display());

    let metadata = fs::metadata(&bundle).unwrap_or_else(|error| {
        panic!(
            "VT worker bundle is unavailable at {}: {error}. Run `npm --prefix vt-worker run build` before building terminald",
            bundle.display()
        )
    });
    assert!(
        metadata.is_file() && metadata.len() != 0,
        "VT worker bundle must be a non-empty regular file: {}",
        bundle.display()
    );

    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"))
        .join("vt-worker.mjs");
    fs::copy(&bundle, &output).unwrap_or_else(|error| {
        panic!(
            "failed to stage VT worker bundle from {} to {}: {error}",
            bundle.display(),
            output.display()
        )
    });
}
