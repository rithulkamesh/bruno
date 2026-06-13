//! Links `libpiper` + `libonnxruntime` when the `piper` feature is enabled.
//!
//! Set `PIPER_DIR` to the libpiper cmake install prefix (the dir that contains
//! `libpiper.dylib`, `lib/libonnxruntime.dylib`, and `espeak-ng-data/`).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PIPER_DIR");

    if std::env::var_os("CARGO_FEATURE_PIPER").is_none() {
        return;
    }

    let dir = std::env::var("PIPER_DIR").unwrap_or_default();
    if dir.is_empty() {
        println!(
            "cargo:warning=`piper` feature is on but PIPER_DIR is not set; \
             libpiper will fail to link. Point PIPER_DIR at the libpiper install prefix."
        );
        return;
    }

    // libpiper lives at the install root; libonnxruntime under lib/.
    println!("cargo:rustc-link-search=native={dir}");
    println!("cargo:rustc-link-search=native={dir}/lib");
    println!("cargo:rustc-link-lib=dylib=piper");
    println!("cargo:rustc-link-lib=dylib=onnxruntime");

    // Embed runtime search paths so the dylibs resolve without DYLD_LIBRARY_PATH.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}/lib");
}
