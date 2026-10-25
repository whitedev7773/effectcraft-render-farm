fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/app-icon/farm.ico");
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
