//! This station's club key for Field Day club sync, and the hash a club host pins keys by.
//!
//! A club position is named by an id that every board line carries, so a JOIN proves the
//! position with a key as well (`tempo_net::fdsync::PositionKey`): the host takes a position only
//! from the laptop that first joined the event as it (`tempo_app::fdevent::ClubLog::key_refusal`).
//!
//! - **Made once, here, with ring**: 32 bytes from the operating system's random source, as 64
//!   lower-case hex digits, the first time Nexus starts without one.
//! - **Kept in a file of its own beside settings.json** (`fd_position.key`), owner-only on unix,
//!   and never in Settings: Settings reach the screen, Remote and the settings file, and this key
//!   reaches only the JOIN line. A file rather than the OS credential store, so a position on a
//!   computer with no credential store (a Linux laptop or a Pi without a keyring) still joins.
//! - **Never logged, printed, shown or sent anywhere but a club JOIN.** Nothing here writes it
//!   to stderr or the diagnostic log, and `PositionKey` has no `Display` and a `Debug` that
//!   prints none of it.
//! - **Hashed in tempo-app before anything keeps it** (`tempo_app::fdevent::sha256_hex`, the
//!   hash a host pins by): the engine and the club journal hold the hash of each position's
//!   key, never the key.
//! - **Lost means a new position.** A file that cannot be read back is replaced by a new key,
//!   which a host that pinned the old one turns away by name; the refusal says how to give that
//!   laptop a position of its own.
use std::path::Path;

use ring::rand::{SecureRandom, SystemRandom};
use tempo_net::fdsync::PositionKey;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A new key: 32 bytes from the operating system's random source. Empty if that source fails,
/// which a host refuses by name (`fdevent::NO_POSITION_KEY`) rather than in silence.
fn new_key() -> PositionKey {
    let mut bytes = [0u8; 32];
    match SystemRandom::new().fill(&mut bytes) {
        Ok(()) => PositionKey::new(hex(&bytes)),
        Err(_) => PositionKey::default(),
    }
}

/// The club key kept at `path`, or a new one made and kept there when the file is missing or
/// holds no key a Nexus makes. The `Err` beside the key says the key could not be kept: it is
/// still this run's, but a host that pins it will not know the next run's.
pub(crate) fn load_or_make(path: &Path) -> (PositionKey, std::io::Result<()>) {
    if let Ok(text) = std::fs::read_to_string(path) {
        let kept = PositionKey::new(text.trim().to_string());
        if kept.is_club_key() {
            return (kept, Ok(()));
        }
    }
    let key = new_key();
    let kept = if key.is_club_key() {
        keep(path, &key)
    } else {
        Err(std::io::Error::other(
            "the operating system gave no random bytes",
        ))
    };
    (key, kept)
}

/// Write `key` to `path` owner-only, through a temporary file the rename publishes, so a crash
/// mid-write leaves the old file or the new one, never half of one.
fn keep(path: &Path, key: &PositionKey) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("key.tmp");
    #[cfg(unix)]
    let mut f = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?
    };
    #[cfg(not(unix))]
    let mut f = std::fs::File::create(&tmp)?;
    std::io::Write::write_all(&mut f, key.secret().as_bytes())?;
    f.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    drop(f);
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nexus-club-key-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// ⭐ **A key is made once and kept**: the first start makes one a host takes (64 hex
    /// digits) and writes it owner-only, every later start reads the very same one, and a file
    /// that holds no key is replaced by a new one. Two laptops never make the same key.
    #[test]
    fn a_club_key_is_made_once_kept_and_read_back() {
        let dir = scratch("kept");
        let path = dir.join("fd_position.key");
        let (made, kept) = load_or_make(&path);
        kept.unwrap();
        assert!(made.is_club_key());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), made.secret());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(
                mode, 0o600,
                "the key file is readable by others at {mode:o}"
            );
        }
        let (again, kept) = load_or_make(&path);
        kept.unwrap();
        assert_eq!(
            again.secret(),
            made.secret(),
            "the next start reads the same key"
        );
        let other = scratch("other").join("fd_position.key");
        assert_ne!(
            load_or_make(&other).0.secret(),
            made.secret(),
            "another laptop's key"
        );
        std::fs::write(&path, "not a key\n").unwrap();
        let (replaced, kept) = load_or_make(&path);
        kept.unwrap();
        assert!(replaced.is_club_key() && replaced.secret() != made.secret());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), replaced.secret());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(other.parent().unwrap());
    }
}
