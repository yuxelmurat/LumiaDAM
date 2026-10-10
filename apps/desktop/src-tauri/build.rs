fn main() {
    println!("cargo:rerun-if-changed=tauri.conf.json");
    // Simge ve arayüz değişince EXE içindeki kaynaklar da yeniden derlenmeli.
    println!("cargo:rerun-if-changed=icons");
    println!("cargo:rerun-if-changed=../dist");
    tauri_build::build()
}
