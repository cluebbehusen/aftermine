//! Compiles the Objective-C half for the Apple targets: user defaults, the
//! key-value store and CloudKit everywhere, iCloud Drive where it exists,
//! which is not Apple TV, and the idle timer and background tasks where
//! `UIKit` exists, which is not the Mac.

fn main() {
    println!("cargo:rerun-if-changed=src/application.m");
    println!("cargo:rerun-if-changed=src/cloudkit.m");
    println!("cargo:rerun-if-changed=src/defaults.m");
    println!("cargo:rerun-if-changed=src/icloud.m");
    println!("cargo:rerun-if-changed=src/key_value.m");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let drive = matches!(os.as_str(), "ios" | "macos");
    let uikit = matches!(os.as_str(), "ios" | "tvos");
    if drive || os == "tvos" {
        let mut build = cc::Build::new();
        build
            .file("src/cloudkit.m")
            .file("src/defaults.m")
            .file("src/key_value.m");
        if drive {
            build.file("src/icloud.m");
        }
        if uikit {
            build.file("src/application.m");
        }
        build
            .flag("-fobjc-arc")
            .warnings_into_errors(true)
            .compile("aftermine_apple_bridge");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=CloudKit");
        if uikit {
            println!("cargo:rustc-link-lib=framework=UIKit");
        }
    }
}
