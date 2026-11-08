//! Compiles the Objective-C half for the Apple targets: user defaults, the
//! key-value store and CloudKit everywhere, and iCloud Drive where it exists,
//! which is not Apple TV.

fn main() {
    println!("cargo:rerun-if-changed=src/cloudkit.m");
    println!("cargo:rerun-if-changed=src/defaults.m");
    println!("cargo:rerun-if-changed=src/icloud.m");
    println!("cargo:rerun-if-changed=src/key_value.m");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let drive = matches!(os.as_str(), "ios" | "macos");
    if drive || os == "tvos" {
        let mut build = cc::Build::new();
        build
            .file("src/cloudkit.m")
            .file("src/defaults.m")
            .file("src/key_value.m");
        if drive {
            build.file("src/icloud.m");
        }
        build
            .flag("-fobjc-arc")
            .warnings_into_errors(true)
            .compile("aftermine_icloud");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=CloudKit");
    }
}
