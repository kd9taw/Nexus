// Tauri build script. Runs `tauri-build` codegen, which reads tauri.conf.json,
// embeds the frontend assets / dev config, and generates the context consumed
// by `tauri::generate_context!()` in src/lib.rs.
fn main() {
    // The official installer build can bake in the project's ClubLog API key via
    // the CLUBLOG_API_KEY env var (read by `option_env!` in src/lib.rs). Cargo does
    // NOT recompile on an env var changing unless told to — without this directive
    // an incremental build could ship a stale/empty key.
    println!("cargo:rerun-if-env-changed=CLUBLOG_API_KEY");

    // AI-CW model presence gate. The DeepCW model is gitignored (AGPL-3.0
    // © e04, 15 MB — see resources/deepcw/README.md), so a fresh checkout or
    // a git WORKTREE builds a tree where resources/deepcw/ has no model, the
    // bundle glob matches nothing, and the result is a fully-green build that
    // ships without the AI CW decoder. That exact artifact reached users on
    // 2026-07-20; release CI now stages+verifies the model, but LOCAL release
    // builds had no guard — this is it. Debug builds only warn (UI work needs
    // no model); release+radio builds fail hard unless explicitly allowed
    // (CI's windows-cross link-check sets NEXUS_ALLOW_MISSING_AICW=1).
    println!("cargo:rerun-if-changed=resources/deepcw");
    println!("cargo:rerun-if-env-changed=NEXUS_ALLOW_MISSING_AICW");
    let radio = std::env::var("CARGO_FEATURE_RADIO").is_ok();
    let model_present = std::path::Path::new("resources/deepcw/model.onnx").exists();
    if radio && !model_present {
        let release = std::env::var("PROFILE").as_deref() == Ok("release");
        let allowed = std::env::var("NEXUS_ALLOW_MISSING_AICW").is_ok();
        if release && !allowed {
            panic!(
                "\nresources/deepcw/model.onnx is MISSING — this release build would ship \
                 with NO AI CW decoder (the silent-lobotomy class: worktrees and fresh \
                 checkouts don't have the gitignored model).\n\
                 Stage it first (see src-tauri/resources/deepcw/README.md), or set \
                 NEXUS_ALLOW_MISSING_AICW=1 to build a knowingly model-less binary.\n"
            );
        }
        println!("cargo:warning=AI CW model missing — this build has no AI CW decoder");
    }

    tauri_build::build();
}
