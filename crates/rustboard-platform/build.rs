fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let native = std::path::Path::new("../../native/macos");
    for name in ["Bridge.swift", "Security.swift"] {
        println!("cargo:rerun-if-changed={}", native.join(name).display());
    }
    let status = std::process::Command::new("xcrun")
        .args([
            "swiftc",
            "-swift-version",
            "5",
            "-O",
            "-emit-library",
            "-static",
            "-module-name",
            "RustboardBridge",
        ])
        .arg(native.join("Bridge.swift"))
        .arg(native.join("Security.swift"))
        .arg("-o")
        .arg(out.join("libRustboardBridge.a"))
        .status()
        .unwrap();
    assert!(status.success(), "Swift bridge compilation failed");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-search=native=/usr/lib/swift");
    let sdk = std::process::Command::new("xcrun")
        .args(["--show-sdk-path"])
        .output()
        .unwrap();
    println!(
        "cargo:rustc-link-search=native={}/usr/lib/swift",
        String::from_utf8(sdk.stdout).unwrap().trim()
    );
    println!("cargo:rustc-link-lib=static=RustboardBridge");
    for framework in ["AppKit", "Foundation", "Security"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
}
