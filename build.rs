// Mette l'icona (e il nome che Esplora risorse mostra in Proprietà) nell'eseguibile Windows.
// Serve `rc.exe` dell'SDK di Windows, presente sulle macchine della CI; senza, il programma
// si compila lo stesso, solo senza icona.
fn main() {
    println!("cargo:rerun-if-changed=assets/renamebot.ico");
    println!("cargo:rerun-if-changed=build.rs");
    // Lo script gira sulla macchina che compila, quindi il sistema di destinazione si legge dall'ambiente.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/renamebot.ico");
    res.set("ProductName", "RenameBot")
        .set("FileDescription", "RenameBot")
        .set("OriginalFilename", "renamebot.exe");
    if let Err(e) = res.compile() {
        println!("cargo:warning=icona non inserita nell'eseguibile: {e}");
    }
}
