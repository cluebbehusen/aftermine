//! Compiles the Objective-C half for the Apple targets with iCloud Drive.

fn main() {
    println!("cargo:rerun-if-changed=src/icloud.m");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if matches!(os.as_str(), "ios" | "macos") {
        cc::Build::new()
            .file("src/icloud.m")
            .flag("-fobjc-arc")
            .warnings_into_errors(true)
            .compile("aftermine_icloud");
        println!("cargo:rustc-link-lib=framework=Foundation");
    }
}
