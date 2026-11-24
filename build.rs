use std::io;
use std::path::Path;
use std::process::Command;

fn npm(args: &[&str]) -> io::Result<()> {
    let mut command = if cfg!(windows) {
        let mut command = Command::new("cmd.exe");
        command.args(["/d", "/c", "npm.cmd"]);
        command
    } else {
        Command::new("npm")
    };
    let status = command.current_dir("ui").args(args).status().map_err(|error| io::Error::other(format!("Node.js and npm are required: {error}")))?;
    if !status.success() {
        return Err(io::Error::other(format!("npm {} failed ({status})", args.join(" "))));
    }
    Ok(())
}

fn web_assets() -> io::Result<()> {
    for path in [
        "ui/package.json",
        "ui/package-lock.json",
        "ui/index.html",
        "ui/vite.config.ts",
        "ui/tsconfig.json",
        "ui/src",
        "assets/ui",
        "assets/app-icon/farm-256.png",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    let lock = std::fs::read("ui/package-lock.json")?;
    let stamp = Path::new("ui/node_modules/.effectcraft-package-lock");
    if std::fs::read(stamp).ok().as_ref() != Some(&lock) {
        npm(&["ci", "--include=dev", "--no-audit", "--no-fund"])?;
        std::fs::write(stamp, lock)?;
    }
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("Missing Cargo OUT_DIR"))?);
    let bundle = out.join("web");
    let bundle_path = bundle.to_str().ok_or_else(|| io::Error::other("Cargo output path must be UTF-8"))?;
    // Vite accepts an absolute output path containing spaces as a single option.
    npm(&["run", "build", "--", "--outDir", bundle_path, "--emptyOutDir"])?;

    let mut assets = String::from("fn embedded(path: &str) -> Option<(&'static str, &'static [u8])> {\n    match path {\n");
    for dir in [&bundle, &bundle.join("assets")] {
        for entry in std::fs::read_dir(dir)? {
            let file = entry?.path();
            if !file.is_file() {
                continue;
            }
            let mime = match file.extension().and_then(|ext| ext.to_str()) {
                Some("html") => "text/html; charset=utf-8",
                Some("js") => "text/javascript; charset=utf-8",
                Some("css") => "text/css; charset=utf-8",
                Some("png") => "image/png",
                Some("svg") => "image/svg+xml",
                Some("ttf") => "font/ttf",
                _ => return Err(io::Error::other(format!("Unknown web asset type: {}", file.display()))),
            };
            let path = file.strip_prefix(&bundle).map_err(io::Error::other)?.to_string_lossy().replace('\\', "/");
            assets.push_str(&format!("        {:?} => Some(({mime:?}, include_bytes!({file:?}))),\n", format!("/{path}")));
        }
    }
    assets.push_str("        _ => None,\n    }\n}\n");
    std::fs::write(out.join("web_assets.rs"), assets)
}

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/app-icon/farm.ico");
    web_assets()?;
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }
    winresource::WindowsResource::new()
        .set_icon("assets/app-icon/farm.ico")
        .set("ProductName", "EffectCraft Render Farm")
        .set("FileDescription", "EffectCraft Render Farm")
        .set("OriginalFilename", "effectcraft-farm.exe")
        .set("InternalName", "effectcraft-farm")
        .set("LegalCopyright", "Copyright (c) 2026 ArtCraft Team and the EffectCraft contributors. MIT OR Apache-2.0.")
        .compile()
}
