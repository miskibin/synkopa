fn main() {
    // the frontend may not be built yet on a fresh clone; tauri-build needs the folder to exist
    let dist = std::path::Path::new("../ui/dist");
    if !dist.exists() {
        let _ = std::fs::create_dir_all(dist);
        let _ =
            std::fs::write(dist.join("index.html"), "<!doctype html><title>Synkopa</title><p>Run <code>pnpm --dir ui build</code>.</p>");
    }
    // macOS: ONNX Runtime's prebuilt static library (stems, pitch) calls `__isPlatformVersionAtLeast`
    // (what `@available` compiles to), which lives in clang's runtime; rustc links without it.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let out = std::process::Command::new("xcrun").args(["clang", "--print-runtime-dir"]).output();
        if let Some(o) = out.ok().filter(|o| o.status.success()) {
            let dir = String::from_utf8_lossy(&o.stdout).trim().to_string();
            println!("cargo:rustc-link-search=native={dir}");
            println!("cargo:rustc-link-lib=static=clang_rt.osx");
        }
    }
    tauri_build::build();
}
