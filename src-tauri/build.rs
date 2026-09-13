fn main() {
    tauri_build::build();
    // tauri-build embeds frontendDist at compile time but does not
    // itself watch the directory in this version -- without this line a
    // plain `cargo build` silently ships a stale UI.
    println!("cargo:rerun-if-changed=../dist");
}
