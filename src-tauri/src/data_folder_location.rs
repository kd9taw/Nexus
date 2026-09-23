//! C8 — is the station's data folder somewhere a DATABASE must not live?
//!
//! Severity is the whole argument. Today the log is an ADIF append: the worst case on a share is
//! "lose the recent contacts and recover from the ring", because a truncated append loses its tail
//! and `parse_adif` resyncs at the next `<`. With the log in SQLite the worst case is **"the
//! database will not open"** — advisory locking across a network filesystem is the classic way to
//! corrupt one, and the operator's contacts are the one thing in this app that cannot be rebuilt.
//!
//! So the rule is asymmetric, and deliberately so:
//!
//!  * **Certain → refuse.** A Windows UNC share, or a Linux mount whose filesystem type IS a
//!    network protocol. These are facts, not guesses, and the refusal costs the operator nothing:
//!    it fires where they are CHOOSING a new folder, so the log they already have stays where it is.
//!  * **Suspected → warn, never assert.** A `Dropbox`/`OneDrive`/`Google Drive`/`iCloud`/`Nextcloud`
//!    path component. These are ordinary local filesystems and no API distinguishes them, so the
//!    check is a name match: it misses a renamed sync root and it would false-positive on a folder
//!    that merely happens to be called `Dropbox`. A false positive that asks a question is cheap; a
//!    false negative is a corrupt database — but a heuristic must not hard-refuse, so this one asks.
//!
//! Everything here is pure except [`sync_marker_beside`], and the mount table is handed in rather
//! than read, so the whole classifier is driven from a test on any platform.

use std::path::{Path, PathBuf};

/// What a data folder's path says about the storage underneath it.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct FolderLocation {
    /// CERTAIN network storage, named for the refusal. `None` means "not certainly remote" —
    /// never "certainly local", which is a claim this cannot make on every platform.
    pub network: Option<String>,
    /// SUSPECTED consumer file-sync folder, named for the warning. Heuristic; never a refusal.
    pub sync_suspected: Option<String>,
}

impl FolderLocation {
    /// Classify `target`. `mounts` is the contents of `/proc/mounts`, or `None` where there is no
    /// such table — which is the normal case off Linux, and means only that a mount cannot be
    /// ruled remote HERE, not that the folder is local.
    pub(crate) fn of(target: &Path, mounts: Option<&str>) -> Self {
        Self {
            network: unc_share(target).or_else(|| mounts.and_then(|m| network_mount(target, m))),
            sync_suspected: sync_component(target),
        }
    }

    /// The plain-English refusal, or `None` when the folder may be used. States the technical
    /// reason and names what to do instead — the operator is being told "no" about their own
    /// logbook, so the message has to leave them somewhere to go.
    pub(crate) fn refusal(&self, target: &Path) -> Option<String> {
        let what = self.network.as_deref()?;
        Some(format!(
            "{} is on {what}. Nexus keeps your logbook in a database, and a database on network \
             storage can be damaged by the way file locking works across a network — the worst \
             case is a logbook that will not open at all. Choose a folder on a drive inside this \
             computer.",
            target.display()
        ))
    }
}

/// The kernel's mount table, on the platform that publishes one.
///
/// ⚠️ **macOS returns `None`, and that is a GAP, not a decision.** The mount table there comes
/// from `statfs().f_fstypename`, which needs `libc` as a direct dependency — a new crate in this
/// workspace, so a NOTICE/licence change and an operator gate. Until that is approved a macOS
/// operator on an SMB share is caught only by the UNC spelling or the sync heuristic, not by the
/// certain check. The seam is here: give this function a macOS arm and the rest follows.
pub(crate) fn mount_table() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/mounts").ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

// ───────────────────────────────────────────────────────────────────────────────────────────
// CERTAIN — Windows UNC

/// The `\\server\share` a path names, if it names one.
///
/// This runs on every platform on purpose: it is a fact about the SPELLING of the path, and the
/// operator types this path by hand (the Settings field exists precisely because an OS folder
/// picker will not reach a share). Matching it here means the Linux CI job proves the Windows rule.
fn unc_share(target: &Path) -> Option<String> {
    let raw = target.to_str()?;
    // Windows accepts forward slashes in a UNC too (`//nas/ham`). POSIX does NOT: `//srv/x` is an
    // ordinary absolute path there, and refusing it would lock a Linux operator out of a LOCAL
    // folder — the strict direction is not the safe direction when the log is already in it.
    #[cfg(windows)]
    let raw = &raw.replace('/', "\\");
    let rest = raw.strip_prefix(r"\\")?;
    // `\\?\` and `\\.\` open the Win32 device namespace; only `\\?\UNC\` is a share. `\\?\C:\data`
    // is an ordinary local drive and must NOT be refused.
    let rest = match rest.strip_prefix('?').or_else(|| rest.strip_prefix('.')) {
        Some(device) => {
            let after = device.strip_prefix('\\')?;
            let (head, tail) = after.split_at(after.find('\\').unwrap_or(after.len()));
            if !head.eq_ignore_ascii_case("UNC") {
                return None;
            }
            tail.strip_prefix('\\')?
        }
        None => rest,
    };
    let mut parts = rest.split('\\').filter(|s| !s.is_empty());
    let server = parts.next()?;
    Some(match parts.next() {
        Some(share) => format!(r"a Windows network share (\\{server}\{share})"),
        None => format!(r"a Windows network share (\\{server})"),
    })
}

// ───────────────────────────────────────────────────────────────────────────────────────────
// CERTAIN — Linux network mounts

/// Filesystem types that ARE a network protocol. Named one by one rather than pattern-matched:
/// this list refuses the operator their own folder, so every entry has to be defensible.
///
/// ⚠️ `9p` is deliberately ABSENT. It is how WSL2 surfaces the machine's own C: drive
/// (`/mnt/c` is `9p` on this very box) and how several hypervisors pass a LOCAL folder through;
/// refusing it would lock a developer or a VM operator out of ordinary local storage. The spec
/// names NFS/CIFS/SMB, and that is what this list is.
const NETWORK_FSTYPES: &[&str] = &[
    "nfs",
    "nfs4",
    "cifs",
    "smb3",
    "smbfs",
    "afs",
    "afpfs",
    "ncpfs",
    "ceph",
    "glusterfs",
    "lustre",
    "davfs",
    "fuse.davfs",
    "fuse.sshfs",
    "sshfs",
    "fuse.smbnetfs",
];

/// The network filesystem `target` sits on, per `/proc/mounts`.
///
/// LONGEST MOUNT POINT WINS, and it has to: `/mnt` ext4 with `/mnt/nas` NFS under it is the shape
/// of a real shack, and the shallow line would otherwise clear a path that is genuinely on the NAS.
/// The comparison is by PATH COMPONENT, so the mount `/mnt/nas` does not swallow `/mnt/nasty`.
fn network_mount(target: &Path, mounts: &str) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for line in mounts.lines() {
        let mut fields = line.split_whitespace();
        let (Some(_device), Some(point), Some(fstype)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let point = unescape_mount_field(point);
        if !target.starts_with(&point) {
            continue;
        }
        let depth = point.components().count();
        if best.as_ref().is_some_and(|(seen, _)| *seen >= depth) {
            continue;
        }
        let fstype = unescape_mount_field(fstype).to_string_lossy().into_owned();
        best = Some((depth, fstype));
    }
    let (_, fstype) = best?;
    NETWORK_FSTYPES
        .iter()
        .any(|known| known.eq_ignore_ascii_case(&fstype))
        .then(|| format!("a network drive ({fstype})"))
}

/// `/proc/mounts` escapes space, tab, newline and backslash as three-digit octal. A shack really
/// does have a `/mnt/Ham Shack NAS`, and reading the field raw would simply fail to match it.
/// Decoded as BYTES, not chars, so a non-UTF-8 mount point survives to the comparison intact.
fn unescape_mount_field(field: &str) -> PathBuf {
    let raw = field.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let octal = (raw[i] == b'\\' && i + 3 < raw.len())
            .then(|| std::str::from_utf8(&raw[i + 1..i + 4]).ok())
            .flatten()
            .and_then(|digits| u8::from_str_radix(digits, 8).ok());
        match octal {
            Some(byte) => {
                out.push(byte);
                i += 4;
            }
            None => {
                out.push(raw[i]);
                i += 1;
            }
        }
    }
    bytes_to_path(out)
}

#[cfg(unix)]
fn bytes_to_path(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
}

#[cfg(not(unix))]
fn bytes_to_path(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

// ───────────────────────────────────────────────────────────────────────────────────────────
// SUSPECTED — consumer file sync

/// Sync roots matched on the WHOLE path component, case-insensitively.
///
/// `com~apple~CloudDocs` is the name iCloud Drive actually has on disk; "iCloud Drive" is the
/// Finder's display name and is what an operator types, so both are here.
const SYNC_EXACT: &[&str] = &[
    "Dropbox",
    "Google Drive",
    "GoogleDrive",
    "iCloud Drive",
    "com~apple~CloudDocs",
    "Nextcloud",
];

/// Matched on a component PREFIX: a work OneDrive is `OneDrive - Contoso`, and the operator's
/// personal one is plain `OneDrive`.
const SYNC_PREFIXES: &[&str] = &["OneDrive"];

/// Marker files a sync client leaves in its own root. These catch a RENAMED sync root, which the
/// name match above cannot — and a renamed root is the common case for anyone with two accounts.
const SYNC_MARKERS: &[&str] = &[".dropbox", ".dropbox.device"];

fn sync_component(target: &Path) -> Option<String> {
    // Split on BOTH separators rather than using `Path::components`, which knows only the host's.
    // The operator types this path by hand, and a Windows path read on Linux is one long component
    // — `C:\Users\op\Google Drive\nexus` would scan as a single name and match nothing. A Linux
    // folder with a literal backslash in it is mis-split by this, and that is the right trade: the
    // only consequence here is a WARNING, never a refusal.
    for name in target
        .to_str()
        .unwrap_or_default()
        .split(['/', '\\'])
        .filter(|s| !s.is_empty())
    {
        if SYNC_EXACT.iter().any(|s| s.eq_ignore_ascii_case(name)) {
            return Some(name.to_string());
        }
        // `get`: the component is the operator's own folder or account name, and a prefix's
        // length in bytes can end inside one of its characters (`田中太郎`).
        if let Some(root) = SYNC_PREFIXES.iter().find(|s| {
            name.get(..s.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(s))
        }) {
            let _ = root;
            return Some(name.to_string());
        }
    }
    None
}

/// A sync client's marker file in `target` or any folder above it. The only part of this module
/// that touches the disk, so it is called where I/O is already happening and never on a hot path.
pub(crate) fn sync_marker_beside(target: &Path) -> Option<String> {
    target.ancestors().find_map(|dir| {
        SYNC_MARKERS
            .iter()
            .find(|marker| dir.join(marker).exists())
            .map(|_| dir.display().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of a real `/proc/mounts`: a shallow local mount with a DEEPER network one under
    /// it, a sibling that is local, and a mount point whose name contains an escaped space.
    const MOUNTS: &str = "\
/dev/sda2 / ext4 rw,relatime 0 0
/dev/sdb1 /mnt ext4 rw,relatime 0 0
nas:/export/ham /mnt/nas nfs4 rw,relatime 0 0
/dev/sdc1 /mnt/nasty ext4 rw,relatime 0 0
//truenas/ham /mnt/Ham\\040Shack cifs rw,relatime 0 0
none /mnt/c 9p rw,relatime 0 0
";

    #[test]
    fn a_unc_path_is_certainly_remote_however_it_is_spelled() {
        for spelling in [
            r"\\nas\ham\nexus",
            r"\\?\UNC\nas\ham\nexus",
            r"\\?\unc\nas\ham",
            r"\\NAS\ham",
        ] {
            let found = FolderLocation::of(Path::new(spelling), None).network;
            assert!(
                found.is_some_and(|w| w.contains("Windows network share")),
                "{spelling} names a share"
            );
        }
        // POSITIVE CONTROLS — the Win32 namespace prefixes that are LOCAL drives. If these came
        // back remote the rule above would be "any path starting with two backslashes", which
        // would refuse an operator's own C: drive.
        for local in [r"\\?\C:\Users\op\nexus", r"\\.\C:\nexus", r"C:\Users\op"] {
            assert_eq!(
                FolderLocation::of(Path::new(local), None).network,
                None,
                "{local} is a local drive"
            );
        }
    }

    #[test]
    fn the_longest_mount_point_decides_and_a_sibling_is_not_swallowed() {
        let remote = FolderLocation::of(Path::new("/mnt/nas/nexus"), Some(MOUNTS)).network;
        assert_eq!(
            remote.as_deref(),
            Some("a network drive (nfs4)"),
            "the DEEPER nfs4 line wins over the ext4 /mnt above it"
        );
        // POSITIVE CONTROL: same parent, same table, one letter different in the mount point.
        // A byte-prefix comparison would call this NFS and refuse a local disk.
        assert_eq!(
            FolderLocation::of(Path::new("/mnt/nasty/nexus"), Some(MOUNTS)).network,
            None,
            "/mnt/nasty is its own ext4 mount, not a child of /mnt/nas"
        );
        // POSITIVE CONTROL: a plain local path under the shallow ext4 mount.
        assert_eq!(
            FolderLocation::of(Path::new("/mnt/local/nexus"), Some(MOUNTS)).network,
            None
        );
    }

    #[test]
    fn an_escaped_space_in_a_mount_point_still_matches() {
        // `/proc/mounts` writes `/mnt/Ham Shack` as `/mnt/Ham\040Shack`. Reading the field raw
        // makes a shack NAS with a space in its name silently pass as local storage.
        assert_eq!(
            FolderLocation::of(Path::new("/mnt/Ham Shack/nexus"), Some(MOUNTS))
                .network
                .as_deref(),
            Some("a network drive (cifs)"),
        );
    }

    #[test]
    fn a_9p_passthrough_is_local_storage() {
        // WSL2 surfaces the machine's own C: drive as 9p (`/mnt/c` on the box this was written
        // on). Refusing it locks an operator out of a drive that is physically inside the computer.
        assert_eq!(
            FolderLocation::of(Path::new("/mnt/c/Users/op/nexus"), Some(MOUNTS)).network,
            None
        );
    }

    #[test]
    fn no_mount_table_never_asserts_a_folder_is_local() {
        // Off Linux there is no /proc/mounts. The absence of a table must read as "cannot tell",
        // never as "local" — and the UNC rule, which needs no table, must still fire.
        assert_eq!(
            FolderLocation::of(Path::new("/mnt/nas/nexus"), None).network,
            None
        );
        assert!(FolderLocation::of(Path::new(r"\\nas\ham"), None)
            .network
            .is_some());
    }

    #[test]
    fn a_sync_folder_is_suspected_but_never_certain() {
        for path in [
            "/home/op/Dropbox/nexus",
            "/home/op/OneDrive - Contoso/nexus",
            "/home/op/OneDrive/nexus",
            "/Users/op/Library/Mobile Documents/com~apple~CloudDocs/nexus",
            "/home/op/Nextcloud/nexus",
            r"C:\Users\op\Google Drive\nexus",
        ] {
            let found = FolderLocation::of(Path::new(path), Some(MOUNTS));
            assert!(found.sync_suspected.is_some(), "{path} looks synced");
            assert_eq!(
                found.network, None,
                "{path} is a guess about a LOCAL filesystem — it must never refuse"
            );
            assert_eq!(
                found.refusal(Path::new(path)),
                None,
                "{path} warns, it does not refuse"
            );
        }
        // POSITIVE CONTROL: an ordinary folder is neither.
        let plain = FolderLocation::of(Path::new("/home/op/Documents/nexus"), Some(MOUNTS));
        assert_eq!(plain, FolderLocation::default());
    }

    /// ⚠️ This runs at EVERY START over the data folder's path, and on Windows that path sits
    /// under the account name. Each component was compared against `OneDrive` by the prefix's
    /// length in BYTES, so a name with a multi-byte character across byte 8 — a kanji or Hangul
    /// account name, or `Jean-Frédéric` — split the character and panicked before the logbook
    /// opened.
    #[test]
    fn a_non_ascii_account_name_does_not_stop_the_check() {
        for user in ["田中太郎", "김철수입니다", "Jean-Frédéric"] {
            let home = format!(r"C:\Users\{user}\AppData\Roaming\tempo");
            assert_eq!(
                FolderLocation::of(Path::new(&home), None),
                FolderLocation::default(),
                "{user}"
            );
            // CONTROL: a real OneDrive under the same account is still named.
            let synced = format!(r"C:\Users\{user}\OneDrive\nexus");
            assert_eq!(
                FolderLocation::of(Path::new(&synced), None)
                    .sync_suspected
                    .as_deref(),
                Some("OneDrive"),
                "{user}"
            );
        }
    }

    #[test]
    fn the_refusal_names_the_reason_and_what_to_do_instead() {
        let target = Path::new("/mnt/nas/nexus");
        let why = FolderLocation::of(target, Some(MOUNTS))
            .refusal(target)
            .expect("refused");
        assert!(why.contains("/mnt/nas/nexus"), "names the folder: {why}");
        assert!(why.contains("nfs4"), "states the technical reason: {why}");
        assert!(
            why.contains("drive inside this computer"),
            "says what to do instead: {why}"
        );
        // Operator ruling: the refusal does not advertise anything.
        for forbidden in ["Station Link", "paid", "subscription", "Remote"] {
            assert!(
                !why.contains(forbidden),
                "must not mention {forbidden}: {why}"
            );
        }
    }
}
