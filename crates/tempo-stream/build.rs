//! On a Windows target, compile the VP8 shim (`src/video/vp8_shim.c`) against the pinned libvpx
//! and link both statically. Nothing to do anywhere else: the encoder is Windows-only for now.
//!
//! libvpx comes from `scripts/build-windows-cross.sh`, which builds the pinned, hash-checked
//! release into `target/vpx-mingw` (`VPX_MINGW_PREFIX` overrides where to look, as
//! `FFTW_MINGW_PREFIX` does for FFTW). A missing build is a hard error here, not a stream that
//! silently has no picture.
use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/video/vp8_shim.c");
    println!("cargo:rerun-if-env-changed=VPX_MINGW_PREFIX");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let prefix = env::var_os("VPX_MINGW_PREFIX")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"))
                .join("../../target/vpx-mingw")
        });
    let library = prefix.join("lib/libvpx.a");
    assert!(
        library.exists() && prefix.join("include/vpx/vp8cx.h").exists(),
        "the Windows build needs the pinned static libvpx at {} (lib/libvpx.a and include/vpx/) — \
         run scripts/build-windows-cross.sh, which builds it, or point VPX_MINGW_PREFIX at a build",
        prefix.display()
    );
    println!("cargo:rerun-if-changed={}", library.display());
    cc::Build::new()
        .file("src/video/vp8_shim.c")
        .include(prefix.join("include"))
        .compile("nexus_vp8");
    println!(
        "cargo:rustc-link-search=native={}",
        prefix.join("lib").display()
    );
    println!("cargo:rustc-link-lib=static=vpx");
}
