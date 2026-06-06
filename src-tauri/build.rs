// Tauri build script. Runs `tauri-build` codegen, which reads tauri.conf.json,
// embeds the frontend assets / dev config, and generates the context consumed
// by `tauri::generate_context!()` in src/lib.rs.
fn main() {
    tauri_build::build();
}
