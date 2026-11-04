fn main() {
    println!("cargo:rerun-if-changed=src/os_log.c");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/os_log.c")
            .warnings(true)
            .compile("aow_macos_log");
    }
}
