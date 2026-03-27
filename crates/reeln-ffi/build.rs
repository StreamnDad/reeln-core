fn main() {
    // cbindgen does not yet support #[unsafe(no_mangle)] (Rust edition 2024).
    // The C header at include/reeln.h is maintained manually.
    // When cbindgen gains support, uncomment the block below.
    //
    // let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    // let config = cbindgen::Config::from_file("cbindgen.toml").unwrap_or_default();
    // cbindgen::Builder::new()
    //     .with_crate(crate_dir)
    //     .with_config(config)
    //     .generate()
    //     .map(|b| { b.write_to_file("include/reeln.h"); })
    //     .ok();
}
