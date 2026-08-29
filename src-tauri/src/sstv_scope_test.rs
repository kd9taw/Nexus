//! The webview may only load images from paths inside `assetProtocol.scope`. So the directory
//! SSTV images are WRITTEN to and the scope the webview is GRANTED are one fact expressed in two
//! files, and nothing connected them.
//!
//! They drifted, and the operator found it: the gallery moved from `<local data>/Nexus/
//! sstv-gallery` to `<Pictures>/Nexus SSTV` (so received pictures land somewhere a person can
//! actually find them), `tauri.conf.json` was not updated, and every preview became a blank box.
//! Nothing errored — a blocked asset URL just yields no image. The pictures were on disk and
//! correct the whole time.
//!
//! This pins the pair together. It is deliberately a string check against the shipped config
//! rather than a runtime one: the scope is enforced by the webview, which no unit test has.

/// `$PICTURE` is tauri 2.x's own spelling (`BaseDirectory::Picture`, tauri-2.11.2
/// `src/path/mod.rs:190`) — NOT `$PICTURES`. A wrong variable name parses fine and grants
/// nothing, which fails exactly like the bug this test exists for.
const PICTURE_SCOPE: &str = "$PICTURE/Nexus SSTV/**";
/// Still needed: `sstv_gallery_dir()` falls back here when `picture_dir()` cannot be resolved,
/// and `migrate_sstv_gallery()` reads it to bring an existing gallery forward.
const LEGACY_SCOPE: &str = "$LOCALDATA/Nexus/sstv-gallery/**";

fn conf() -> String {
    // CARGO_MANIFEST_DIR is src-tauri/, where tauri.conf.json lives.
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json"))
        .expect("tauri.conf.json must be readable")
}

#[test]
fn the_asset_scope_covers_where_sstv_images_are_actually_written() {
    let c = conf();
    let v: serde_json::Value =
        serde_json::from_str(&c).expect("tauri.conf.json must be valid JSON");
    let scope = v["app"]["security"]["assetProtocol"]["scope"]
        .as_array()
        .expect("assetProtocol.scope must be an array");
    let entries: Vec<&str> = scope.iter().filter_map(|s| s.as_str()).collect();

    assert!(
        entries.contains(&PICTURE_SCOPE),
        "the gallery writes to <Pictures>/Nexus SSTV (lib.rs `sstv_gallery_dir`), but the webview \
         is not granted it — every preview renders as a blank box with no error. scope = {entries:?}"
    );
    assert!(
        entries.contains(&LEGACY_SCOPE),
        "the legacy location is still the fallback when picture_dir() fails, and is what \
         migrate_sstv_gallery reads — dropping it blanks previews for anyone it falls back for. \
         scope = {entries:?}"
    );
}

#[test]
fn the_scope_still_names_the_directory_the_code_builds() {
    // Ties the two files by their SHARED STRING. If someone renames the folder in lib.rs, this
    // fails here rather than silently blanking the gallery in a shipped build.
    let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("lib.rs must be readable");
    assert!(
        lib.contains(r#"join("Nexus SSTV")"#),
        "sstv_gallery_dir() no longer joins \"Nexus SSTV\" — update PICTURE_SCOPE in this test \
         and the scope in tauri.conf.json together, or previews go blank"
    );
    assert!(
        lib.contains(r#".join("sstv-gallery")"#),
        "legacy_sstv_gallery_dir() no longer joins \"sstv-gallery\" — same pairing applies"
    );

    // POSITIVE CONTROL: the read is really seeing lib.rs, so the two assertions above are
    // findings rather than an empty-string vacuously containing nothing.
    assert!(
        lib.contains("fn sstv_gallery_dir()"),
        "control: this test is not reading the file it thinks it is"
    );
}
