//! The installer an automatic update leaves in the Temp folder, removed at the next start.
//!
//! # What is left behind, and why
//!
//! On Windows, `tauri-plugin-updater` (2.10.1, `Update::make_temp_dir` / `write_to_temp`) writes
//! the downloaded installer to `%TEMP%\<productName>-<new version>-updater-<random>\` as
//! `<productName>-<new version>-installer.exe`. It calls `.keep()` on that folder — it has to,
//! because it then launches the installer and exits the process, and the installer is still
//! running from it. Nothing ever comes back for it, so every update left one installer behind.
//!
//! On macOS and Linux the plugin does not use this scheme: its `tauri_current_app`,
//! `tauri_updated_app` and `tauri_<deb|rpm>_update` folders are `TempDir`s it drops (and so
//! removes) itself. [`spawn`] is therefore a no-op off Windows.
//!
//! # What this removes — and only this
//!
//! A DIRECT child of the temp folder that:
//! - is a real directory (a symlink or junction is skipped, never followed);
//! - is named exactly `<productName>-<semver>-updater-<suffix>`, the suffix ASCII alphanumeric,
//!   as `tempfile` makes it;
//! - is for a version **no newer than the running build** — a newer one may belong to an update
//!   being downloaded or installed right now;
//! - was last modified at least [`MIN_AGE`] ago, so the installer that just started this build
//!   (same version, possibly still finishing) is left alone.
//!
//! Best-effort and quiet: it runs on a thread of its own, a folder it cannot remove is skipped,
//! and the diagnostic log gets one line with the count. It never fails a start.

use std::path::Path;
use std::time::{Duration, SystemTime};

/// How old a leftover folder must be before it is removed: well past the few seconds the
/// installer takes to finish and start the new build.
const MIN_AGE: Duration = Duration::from_secs(10 * 60);

/// Sweep the system temp folder for this app's leftover updater folders, on a thread of its own.
/// Windows only; elsewhere the plugin cleans up after itself (see the module header).
pub(crate) fn spawn(app: &tauri::AppHandle) {
    if !cfg!(windows) {
        return;
    }
    let info = app.package_info();
    let product = info.name.clone();
    let running = info.version.clone();
    let _ = std::thread::Builder::new()
        .name("nexus-updater-leftovers".into())
        .spawn(move || {
            let removed = sweep(
                &std::env::temp_dir(),
                &product,
                &running,
                MIN_AGE,
                SystemTime::now(),
            );
            if removed > 0 {
                tempo_core::applog::info(
                    "updater",
                    &format!("removed {removed} leftover update installer folder(s) from Temp"),
                );
            }
        });
}

/// Remove every leftover updater folder of `product` in `temp_root` (see the module header for
/// exactly which). Returns how many were removed.
fn sweep(
    temp_root: &Path,
    product: &str,
    running: &semver::Version,
    min_age: Duration,
    now: SystemTime,
) -> usize {
    let Ok(entries) = std::fs::read_dir(temp_root) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some((raw_version, version)) = name.to_str().and_then(|n| leftover_version(n, product))
        else {
            continue;
        };
        if version > *running {
            continue;
        }
        // `symlink_metadata`: a symlink (or Windows junction) reports as itself, not a directory.
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if !meta.file_type().is_dir() {
            continue;
        }
        let old_enough = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age >= min_age);
        if old_enough && remove_leftover(&entry.path(), product, raw_version) {
            removed += 1;
        }
    }
    removed
}

/// Remove `dir` only if it holds nothing but the installer the plugin writes into it
/// (`<product>-<version>-installer.exe` or `.msi`, a regular file) — that file, then the empty
/// folder. No recursive delete: a folder holding anything else is left whole.
fn remove_leftover(dir: &Path, product: &str, version: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut installer = None;
    for entry in entries {
        let Ok(entry) = entry else {
            return false;
        };
        let name = entry.file_name();
        let is_installer = [".exe", ".msi"]
            .iter()
            .any(|ext| name.to_str() == Some(&format!("{product}-{version}-installer{ext}")));
        let is_file = entry.file_type().is_ok_and(|t| t.is_file());
        if !is_installer || !is_file || installer.is_some() {
            return false;
        }
        installer = Some(entry.path());
    }
    if let Some(file) = installer {
        if std::fs::remove_file(file).is_err() {
            return false;
        }
    }
    std::fs::remove_dir(dir).is_ok()
}

/// The version in a folder name of the exact form `<product>-<semver>-updater-<suffix>`, as
/// written and as parsed.
fn leftover_version<'a>(name: &'a str, product: &str) -> Option<(&'a str, semver::Version)> {
    let rest = name.strip_prefix(product)?.strip_prefix('-')?;
    let (version, suffix) = rest.rsplit_once("-updater-")?;
    if suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    Some((version, semver::Version::parse(version).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    const HOUR: Duration = Duration::from_secs(3600);

    fn running() -> semver::Version {
        semver::Version::parse("1.24.0").unwrap()
    }

    /// A folder holding an installer, last modified `age` ago.
    fn folder(root: &Path, name: &str, age: Duration) -> PathBuf {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        // The installer as the plugin names it: `<product>-<version>-installer.exe`.
        let installer = name
            .split_once("-updater-")
            .map_or("x".to_string(), |(pv, _)| format!("{pv}-installer.exe"));
        fs::write(dir.join(&installer), b"MZ").unwrap();
        let when = SystemTime::now() - age;
        fs::File::open(dir.join(&installer))
            .unwrap()
            .set_modified(when)
            .unwrap();
        fs::File::open(&dir).unwrap().set_modified(when).unwrap();
        dir
    }

    #[test]
    fn removes_only_old_leftovers_no_newer_than_the_running_build() {
        let base = crate::test_dirs::root().join("updater-leftovers");
        let root = base.join("temp");
        fs::create_dir_all(&root).unwrap();

        let older = folder(&root, "Nexus-1.23.0-updater-Ab12Cd", HOUR);
        let same = folder(&root, "Nexus-1.24.0-updater-xYz789", HOUR);
        let older_beta = folder(&root, "Nexus-1.24.0-beta.2-updater-Q1w2E3", HOUR);
        let newer = folder(&root, "Nexus-1.25.0-updater-Ab12Cd", HOUR);
        let fresh = folder(
            &root,
            "Nexus-1.24.0-updater-FrEsH1",
            Duration::from_secs(60),
        );
        // Names that are not ours, or not exactly the plugin's shape.
        let keep: Vec<PathBuf> = [
            "Nexus-1.23.0-updater-",            // no suffix
            "Nexus-1.23.0-updater-ab.cd",       // suffix not alphanumeric
            "Nexus-notaversion-updater-Ab12Cd", // version does not parse
            "NexusX-1.23.0-updater-Ab12Cd",     // another product
            "Other-1.23.0-updater-Ab12Cd",      // another product
            "Nexus-1.23.0-installer",           // not an updater folder
            "nexus-1.23.0-updater-Ab12Cd",      // product name is case-exact
            "Nexus-1.23.0-updater-Ab12Cd-copy", // suffix not alphanumeric
        ]
        .iter()
        .map(|n| folder(&root, n, HOUR))
        .collect();
        // A FILE with a matching name is not a folder: left alone.
        let file = root.join("Nexus-1.22.0-updater-F1le00");
        fs::write(&file, b"not a dir").unwrap();
        // A matching folder one level down is not a direct child: left alone.
        let nested = folder(&root.join("sub"), "Nexus-1.22.0-updater-Nest00", HOUR);

        let removed = sweep(&root, "Nexus", &running(), MIN_AGE, SystemTime::now());

        assert_eq!(removed, 3);
        assert!(!older.exists());
        assert!(!same.exists());
        assert!(!older_beta.exists());
        assert!(
            newer.exists(),
            "a newer version may be installing right now"
        );
        assert!(
            fresh.exists(),
            "a fresh folder may belong to the install that just ran"
        );
        for k in &keep {
            assert!(k.exists(), "{} must not be touched", k.display());
        }
        assert!(file.exists());
        assert!(nested.exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_with_a_matching_name_is_not_followed() {
        let base = crate::test_dirs::root().join("updater-leftovers-symlink");
        let root = base.join("temp");
        fs::create_dir_all(&root).unwrap();
        let outside = folder(&base, "precious", HOUR);
        let link = root.join("Nexus-1.20.0-updater-L1nk00");
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        let removed = sweep(&root, "Nexus", &running(), MIN_AGE, SystemTime::now());

        assert_eq!(removed, 0);
        assert!(
            link.symlink_metadata().is_ok(),
            "the link itself is left alone"
        );
        assert!(outside.join("x").exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_matching_folder_holding_anything_but_the_installer_is_left_whole() {
        let base = crate::test_dirs::root().join("updater-leftovers-content");
        let root = base.join("temp");
        fs::create_dir_all(&root).unwrap();
        let extra = folder(&root, "Nexus-1.20.0-updater-Us3r01", HOUR);
        fs::write(extra.join("notes.txt"), b"mine").unwrap();
        let sub = folder(&root, "Nexus-1.20.0-updater-Us3r02", HOUR);
        fs::create_dir(sub.join("photos")).unwrap();
        let wrong = root.join("Nexus-1.20.0-updater-Us3r03");
        fs::create_dir(&wrong).unwrap();
        fs::write(wrong.join("Nexus-1.19.0-installer.exe"), b"MZ").unwrap();
        for d in [&extra, &sub, &wrong] {
            fs::File::open(d)
                .unwrap()
                .set_modified(SystemTime::now() - HOUR)
                .unwrap();
        }

        let removed = sweep(&root, "Nexus", &running(), MIN_AGE, SystemTime::now());

        assert_eq!(removed, 0);
        assert!(extra.join("notes.txt").exists());
        assert!(extra.join("Nexus-1.20.0-installer.exe").exists());
        assert!(sub.join("photos").exists());
        assert!(wrong.join("Nexus-1.19.0-installer.exe").exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_missing_temp_folder_is_not_an_error() {
        let root = crate::test_dirs::root().join("updater-leftovers-none");
        assert_eq!(
            sweep(&root, "Nexus", &running(), MIN_AGE, SystemTime::now()),
            0
        );
    }
}
