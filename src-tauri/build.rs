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

    // Bundled-Hamlib presence gate — the same silent-lobotomy class as the model
    // above, and the wrapper script is not enough on its own: build-windows-cross.sh
    // stages these via scripts/fetch-hamlib.sh, but running `cargo tauri build`
    // directly (the normal way to iterate) skips that entirely, and a git WORKTREE
    // starts out in exactly that state. resources/hamlib/ holds the Windows CAT
    // runtime — rigctld.exe plus libhamlib-4.dll and the MinGW DLLs it loads — and
    // *.exe/*.dll are gitignored, so a fresh clone has only the tracked licence
    // .txt files. The bundle glob `resources/hamlib/*` still matches THOSE, so
    // nothing errors: the installer ships a hamlib directory with no Hamlib in it.
    // Every Hamlib-backed rig (Yaesu, Kenwood, Elecraft…) is then CAT-dead while a
    // native CI-V Icom works perfectly — indistinguishable from a code regression,
    // and it burns operator time before anyone suspects the build.
    //
    // WINDOWS TARGETS ONLY: Linux uses the system Hamlib, and build-linux.sh
    // deliberately DELETES these files before it builds, so gating any wider
    // would break every .deb / AppImage / Pi build.
    //
    // Keep this list in step with WANT in scripts/fetch-hamlib.sh. The smallest
    // real file in Hamlib 4.7.1 is rotctl.exe at ~114 KB, so 32 KB separates a
    // staged binary from an empty or truncated one.
    const HAMLIB: [&str; 8] = [
        "rigctld.exe",
        "rigctl.exe",
        "rotctld.exe",
        "rotctl.exe",
        "libhamlib-4.dll",
        "libwinpthread-1.dll",
        "libusb-1.0.dll",
        "libgcc_s_seh-1.dll",
    ];
    const HAMLIB_MIN_BYTES: u64 = 32 * 1024;
    println!("cargo:rerun-if-changed=resources/hamlib");
    println!("cargo:rerun-if-env-changed=NEXUS_ALLOW_MISSING_HAMLIB");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if radio && windows {
        let gaps: Vec<String> = HAMLIB
            .iter()
            .filter_map(|f| {
                let path = std::path::Path::new("resources/hamlib").join(f);
                match std::fs::metadata(&path) {
                    Err(_) => Some(format!("  {f} — MISSING")),
                    Ok(m) if m.len() < HAMLIB_MIN_BYTES => {
                        Some(format!("  {f} — only {} bytes (truncated?)", m.len()))
                    }
                    Ok(_) => None,
                }
            })
            .collect();
        if !gaps.is_empty() {
            let release = std::env::var("PROFILE").as_deref() == Ok("release");
            let allowed = std::env::var("NEXUS_ALLOW_MISSING_HAMLIB").is_ok();
            if release && !allowed {
                panic!(
                    "\nBundled Hamlib is INCOMPLETE — this release build would ship with NO CAT \
                     control for every non-Icom rig (Yaesu, Kenwood, Elecraft…), while a native \
                     CI-V Icom kept working, which looks exactly like a code regression:\n\n{}\n\n\
                     Stage it with:  ./scripts/fetch-hamlib.sh\n\
                     (The .exe/.dll are gitignored, so worktrees and fresh checkouts never have \
                     them. Set NEXUS_ALLOW_MISSING_HAMLIB=1 to build a knowingly CAT-less \
                     binary — CI's windows-cross link-check does, as it proves compile, not \
                     shipping.)\n",
                    gaps.join("\n")
                );
            }
            println!(
                "cargo:warning=bundled Hamlib incomplete — this build has no CAT for non-Icom rigs"
            );
        }
    }

    tauri_build::build();
}
