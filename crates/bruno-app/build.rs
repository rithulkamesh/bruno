fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").ok().as_deref() == Some("macos") {
        let plist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Info.plist");
        println!("cargo:rerun-if-changed={}", plist.display());
        println!("cargo:rustc-link-arg=-sectcreate");
        println!("cargo:rustc-link-arg=__TEXT");
        println!("cargo:rustc-link-arg=__info_plist");
        println!("cargo:rustc-link-arg={}", plist.display());
    }

    // Embed rpaths to the libpiper install on the FINAL binary. A dependency's
    // build script (bruno-voice) can't add link-args to this binary, so it has
    // to happen here. Only matters with the `piper` feature.
    println!("cargo:rerun-if-env-changed=PIPER_DIR");
    if std::env::var_os("CARGO_FEATURE_PIPER").is_some() {
        if let Ok(dir) = std::env::var("PIPER_DIR") {
            if !dir.is_empty() {
                println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
                println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}/lib");
            }
        }
    }
}
