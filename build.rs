fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/app.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon_with_id("assets/icon.ico", "1");
        res.set_manifest_file("assets/app.manifest");
        res.set("ProductName", "jhs");
        res.set("FileDescription", "jhs: modo privacidad y apagado programado");
        if let Err(e) = res.compile() {
            panic!("no se pudo compilar el recurso de Windows: {e}");
        }
    }
}
