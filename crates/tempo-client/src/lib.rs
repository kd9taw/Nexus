//! Nexus driving a station from another computer, without the desktop shell around it: this
//! computer's end of the command link and of the two roads beside it, and what both ends of them read
//! and write. Client mode drives it from the screens (src-tauri's `client_mode`), as it always has;
//! the shack's listener (src-tauri's `remote_service::lan`) speaks the same wire with these same types.
//!
//! ## What is here
//!
//! - **The command link** ([`link`]), protocol 2, on the road to a paired station ([`road`]): TLS 1.3
//!   with both raw keys pinned ([`tls`]), the hello, the build check, calls whose ids rise across a run
//!   of Nexus, and the feeds. Its frames and the build's contract are [`wire`]'s, and one connection's
//!   writer, at either end, is [`outbox`].
//! - **Stop's second road** ([`stop_road`]) and **the audio road** ([`audio_road`]): each a UDP socket
//!   and a thread of its own, keyed from the link's TLS session. Their datagrams ([`datagram`]) and
//!   what each road carries ([`stop_road::wire`], [`audio_road::wire`]) are read and written the same
//!   way at both ends.
//! - **The internet roads' way across** ([`ice`]): each road's own socket, ICE to find a pair of
//!   addresses that reach the other end, and at this computer a relayed address on a TURN server
//!   ([`turn`]) for when none does.
//! - **The tables both ends route and admit by**: every command's class ([`classes`]); the refusals,
//!   the outcomes, the starts wired from afar and Stop's rules ([`dispatch`]); the alert's event
//!   ([`alert`]).
//! - **This computer's key for a station** ([`ComputerKey`]) and the station's record as this computer
//!   keeps it ([`PairedStation`]).
//! - **A test build's road switch** (`bench_road_switch`), only behind the feature of that name.
//!
//! ## What is not
//!
//! No Tauri, no webview and no engine: nothing here opens a window, runs a station's command or keys a
//! radio. The shack's side (its listener, its ends of the roads, the dispatcher's journal), the pairing
//! ceremony, the OS credential store, the window's loopback origin and client mode itself stay in
//! src-tauri. Nothing here needs Windows: the crate builds on its own wherever the workspace builds,
//! and its manifest names no Tauri, webview or Windows-only dependency, nor do its path dependencies
//! name a Tauri or webview one (the first test below holds both). This computer's own sound devices
//! (cpal, through tempo-audio) are behind the `device` feature; without it nothing the crate builds
//! on is a crate of the engine or builds the decoders' Fortran or SQLite, so a phone can build it
//! (the second test below holds that).
// A write to the link, and a piece read off it, fail only once the connection is over, which is
// all their `Err(())` says and all a caller acts on: the app's own contract since protocol 2,
// public now only so that the shack's side in src-tauri reads the same wire.
#![allow(clippy::result_unit_err)]
use ring::digest::{digest, SHA256};
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
use serde::{Deserialize, Serialize};
use tempo_stream::protocol;

pub mod alert {
    //! The alert's event: what the shack raises every alert as, on each of its windows and on the
    //! command link, and the one event a computer driving it takes in
    //! ([`crate::dispatch::FORWARDED_EVENTS`]). The alert's shape and its ids are the shack's
    //! (src-tauri's `alert`).

    /// The event every alert is raised as, on each window and on the command link.
    pub const EVENT: &str = "alert";
}
/// The account, as a station checks it: the built-in sign-in, proofs, the issuer's keys, devices.
pub mod account;
/// The audio road, beside the command link, and this computer's headset.
pub mod audio_road;
/// Test builds only (the `bench-road-switch` feature): the switch this computer's roads to the shack
/// are opened under, which blocks one of them at a time.
#[cfg(feature = "bench-road-switch")]
pub mod bench_road_switch;
/// Where each command runs when another computer drives a station.
pub mod classes;
/// The datagram layer of the roads beside the command link.
pub mod datagram;
/// What both ends route and admit a call by.
pub mod dispatch;
/// An internet road's way across: its socket, ICE, and the relay.
pub mod ice;
/// An ID token from the account's issuer, checked here: signature, issuer, audience, life, nonce.
pub mod id_token;
/// The command link, protocol 2, which client mode drives.
pub mod link;
/// The internet road's account side: signed requests and the first sign-in.
pub mod native;
/// One connection's writer, at either end of the command link.
pub mod outbox;
/// Phone's Lock from afar: its cap and "Still talking?".
pub mod phone_lock;
/// The road to a paired station, for the window's page and the command link.
pub mod road;
/// "Still there?" on the internet road: the operator's input, measured.
pub mod still_there;
/// Stop's second road, beside the command link.
pub mod stop_road;
/// The LAN road's TLS, this computer's half.
pub mod tls;
/// The pipe through a station's room, as a byte stream for TLS.
pub mod tunnel;
/// The TURN client the internet roads' relayed addresses come from.
pub mod turn;
/// The command link's wire, as both ends read and write it.
pub mod wire;

/// `bytes` as lowercase hexadecimal, two characters a byte.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// This computer's own key for one station: P-256, made with ring when this computer pairs with
/// that station, and kept with the station's record in the OS credential store (src-tauri's
/// `lan_client`), and nowhere else. It proves this computer twice over, in the TLS handshake and
/// on every stream offer. The private half is never logged, shown or sent, and nothing that
/// carries it has `Debug`, `Clone` or `Serialize`.
pub struct ComputerKey {
    pair: EcdsaKeyPair,
    /// The public half: SPKI DER, lowercase hex, the shape a device key has (182 characters).
    public_key: String,
}

impl ComputerKey {
    /// A new key, and its PKCS#8 document as lowercase hex for the station's record: the one copy
    /// of the private key that ever leaves this type, and it goes only there.
    pub fn generate() -> Option<(Self, String)> {
        let pkcs8 =
            EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
                .ok()?;
        let document = hex(pkcs8.as_ref());
        Some((Self::restore(&document)?, document))
    }

    /// The key a record holds, or `None` for anything that is not one.
    pub fn restore(pkcs8: &str) -> Option<Self> {
        let pair = EcdsaKeyPair::from_pkcs8(
            &ECDSA_P256_SHA256_FIXED_SIGNING,
            &protocol::hex_bytes(pkcs8)?,
            &SystemRandom::new(),
        )
        .ok()?;
        let public_key = format!(
            "{}{}",
            protocol::P256_SPKI_PREFIX_HEX,
            hex(pair.public_key().as_ref())
        );
        Some(Self { pair, public_key })
    }

    /// The public half, SPKI as lowercase hex: what the station pinned while pairing.
    pub fn public_key(&self) -> &str {
        &self.public_key
    }

    /// This computer's signature over an offer (A5): over SHA-256 of `sdp`'s DTLS fingerprint and
    /// the station's, device's and session's ids, as the station checks it (`stream::verify`).
    /// `None` for an offer with no single fingerprint, which no station would take.
    pub fn sign_offer(
        &self,
        sdp: &str,
        station: &str,
        device: &str,
        session: &str,
    ) -> Option<String> {
        let fingerprint: [u8; 32] = digest(&SHA256, &protocol::offer_fingerprint(sdp)?)
            .as_ref()
            .try_into()
            .ok()?;
        let signed = protocol::offer_binding(&fingerprint, station, device, session);
        let signature = self.pair.sign(&SystemRandom::new(), &signed).ok()?;
        Some(hex(signature.as_ref()))
    }
}

/// A paired station as the page shows it: never this computer's key, only the station's
/// fingerprint (SHA-256 of its SPKI, lowercase hex) and where it answered.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StationView {
    pub id: String,
    pub addresses: Vec<String>,
    pub key: String,
    /// The address or host name Operate tries first, as the operator typed it, when one is kept
    /// (beside the record, never in it: [`StationView::of`] gives none).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typed_address: Option<String>,
}

impl StationView {
    pub fn of(record: &PairedStation) -> Option<Self> {
        let spki = protocol::hex_bytes(&record.station_key)?;
        Some(Self {
            id: record.station_id.clone(),
            addresses: record.addresses.clone(),
            key: hex(digest(&SHA256, &spki).as_ref()),
            typed_address: None,
        })
    }
}

/// A station this computer is paired with over its network, as src-tauri's `lan_client` keeps it in
/// the OS credential store. No `Debug` and no `Clone`, as the vault's `StationKey`: it holds this
/// computer's own key for the station.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairedStation {
    pub station_id: String,
    /// The device id the station gave this computer: what it stamps on everything it sends.
    pub device_id: String,
    /// The station's LAN key, SPKI as lowercase hex, exactly as it presented it while pairing:
    /// pinned from then on.
    pub station_key: String,
    /// This computer's own key for this station: PKCS#8, lowercase hex.
    pub pkcs8: String,
    /// Where the station answered, `address:port`, the last that worked first.
    pub addresses: Vec<String>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    /// Whether `name` is a crate of the desktop shell or a webview it hosts.
    fn shell(name: &str) -> bool {
        [
            "tauri",
            "wry",
            "tao",
            "webview2",
            "webkit2gtk",
            "javascriptcore",
        ]
        .iter()
        .any(|kind| name == *kind || name.starts_with(&format!("{kind}-")))
    }

    /// Whether `name` is Windows' own bindings, which build nowhere else.
    fn windows_only(name: &str) -> bool {
        ["windows", "winapi", "winreg"]
            .iter()
            .any(|kind| name == *kind || name.starts_with(&format!("{kind}-")))
    }

    /// Whether a table header is a table of dependencies, and whether it is one for Windows alone.
    fn dependency_table(header: &str) -> Option<bool> {
        let header = header.trim_matches(|c| c == '[' || c == ']');
        let kind = header.rsplit('.').next().unwrap_or(header);
        let dependencies = kind.ends_with("dependencies")
            || header.split('.').any(|part| part.ends_with("dependencies"));
        let windows = header.contains("cfg(windows)") || header.contains("\"windows\"");
        dependencies.then_some(windows)
    }

    /// What the manifest at `path`, read through `read`, names that a client core may not depend
    /// on, each with its manifest and line; then the manifests of the path dependencies it builds
    /// on, in turn, held to the shell and its webviews (their Windows-only tables are their own
    /// platform code, which builds wherever the workspace does). Each manifest is read once, and
    /// `seen` keeps which.
    fn refusals(
        path: &Path,
        read: &dyn Fn(&Path) -> Option<String>,
        seen: &mut BTreeSet<PathBuf>,
    ) -> Vec<String> {
        let own = seen.is_empty();
        if !seen.insert(std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())) {
            return Vec::new();
        }
        let Some(text) = read(path) else {
            return vec![format!("{}: not there", path.display())];
        };
        let mut found = Vec::new();
        let mut table: Option<bool> = None;
        let mut paths = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let at = format!("{}:{}", path.display(), n + 1);
            let code = line.split('#').next().unwrap_or_default().trim_end();
            if code.starts_with('[') {
                // A path dependency's own tests' dependencies are never built for this crate.
                table =
                    dependency_table(code).filter(|_| own || !code.contains("dev-dependencies"));
                if own && table == Some(true) {
                    found.push(format!("{at}: a dependency table for Windows alone"));
                }
                // `[dependencies.tauri]`, a dependency's own table.
                if let Some(name) = code
                    .trim_matches(|c| c == '[' || c == ']')
                    .rsplit_once("dependencies.")
                    .map(|(_, name)| name)
                {
                    if shell(name) || (own && windows_only(name)) {
                        found.push(format!("{at}: names {name}"));
                    }
                }
                continue;
            }
            if table.is_none()
                || code.starts_with(|c: char| c.is_whitespace() || c == '"' || c == ']' || c == '}')
            {
                continue;
            }
            let Some((name, value)) = code.split_once('=') else {
                continue;
            };
            let name = name.trim().split('.').next().unwrap_or_default();
            if shell(name) || (own && windows_only(name)) {
                found.push(format!("{at}: names {name}"));
            }
            if let Some(dependency) = value
                .split("path = \"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
            {
                paths.push(
                    path.parent()
                        .unwrap_or(Path::new(""))
                        .join(dependency)
                        .join("Cargo.toml"),
                );
            }
        }
        for dependency in paths {
            found.extend(refusals(&dependency, read, seen));
        }
        found
    }

    fn manifest() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
    }

    /// ★ The client core needs no Tauri, no webview and nothing Windows-only: its manifest names no
    /// crate of the desktop shell or its webviews, no Windows binding and no table for Windows
    /// alone, and no path dependency it builds on names a crate of the shell or its webviews. The
    /// guard read the manifests it is about. CONTROL: one planted line of each kind is named, in the
    /// crate's own manifest and in a path dependency's, and nothing else is.
    #[test]
    fn the_client_core_needs_no_tauri_no_webview_and_nothing_windows_only() {
        let from_disk = |path: &Path| std::fs::read_to_string(path).ok();
        let mut seen = BTreeSet::new();
        assert_eq!(
            refusals(&manifest(), &from_disk, &mut seen),
            Vec::<String>::new()
        );
        let read: Vec<String> = seen
            .iter()
            .filter_map(|path| path.parent()?.file_name()?.to_str().map(str::to_string))
            .collect();
        for crate_dir in [
            "tempo-client",
            "tempo-app",
            "tempo-audio",
            "tempo-core",
            "tempo-stream",
            "tempo-base",
            "tempo-codec",
        ] {
            assert!(
                read.iter().any(|dir| dir == crate_dir),
                "never read {crate_dir}: {read:?}"
            );
        }

        let core = manifest();
        let own = std::fs::read_to_string(&core).unwrap();
        let audio = core.parent().unwrap().join("../tempo-audio/Cargo.toml");
        let planted: [(&Path, &str, &str); 6] = [
            (&core, "[dependencies]\n", "[dependencies]\ntauri = \"2\"\n"),
            (
                &core,
                "[dependencies]\n",
                "[dependencies]\nwry = { version = \"0.53\" }\n",
            ),
            (
                &core,
                "[features]\n",
                "[target.'cfg(windows)'.dependencies]\nring = \"0.17\"\n\n[features]\n",
            ),
            (
                &core,
                "[dependencies]\n",
                "[dependencies]\nwindows-sys = \"0.59\"\n",
            ),
            (
                &core,
                "[features]\n",
                "[dependencies.webview2-com]\nversion = \"0.38\"\n\n[features]\n",
            ),
            (
                &audio,
                "[dependencies]\n",
                "[dependencies]\ntauri-runtime = \"2\"\n",
            ),
        ];
        let audio_text = std::fs::read_to_string(&audio).unwrap();
        for (file, from, to) in planted {
            let text = if file == core {
                own.clone()
            } else {
                audio_text.clone()
            };
            assert!(
                text.contains(from),
                "{} has no {from:?} to plant in",
                file.display()
            );
            let changed = text.replacen(from, to, 1);
            let file = file.to_path_buf();
            let read = |path: &Path| {
                if path == file
                    || std::fs::canonicalize(path).ok() == std::fs::canonicalize(&file).ok()
                {
                    Some(changed.clone())
                } else {
                    std::fs::read_to_string(path).ok()
                }
            };
            let found = refusals(&manifest(), &read, &mut BTreeSet::new());
            assert_eq!(found.len(), 1, "planted {to:?}: {found:?}");
        }
    }

    /// Whether `name` is a crate of the engine, or one that builds the decoders' Fortran or SQLite:
    /// what a build of the client core alone, a phone's, has no use for.
    fn engine(name: &str) -> bool {
        [
            "tempo-app",
            "tempo-core",
            "tempo-audio",
            "tempo-fast-sys",
            "rusqlite",
            "libsqlite3-sys",
        ]
        .contains(&name)
    }

    /// What the manifest at `path`, read through `read`, builds on of the engine with no feature
    /// asked for, each with its manifest and line; then the same of each path dependency it builds
    /// on that way, in turn. An optional dependency is left out unless its manifest turns a feature
    /// on by default; a table of one dependency (`[dependencies.name]`) is judged by its name; a
    /// crate of the engine is named and not read further. Each manifest is read once, and `seen`
    /// keeps which.
    fn engine_reached(
        path: &Path,
        read: &dyn Fn(&Path) -> Option<String>,
        seen: &mut BTreeSet<PathBuf>,
    ) -> Vec<String> {
        if !seen.insert(std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())) {
            return Vec::new();
        }
        let Some(text) = read(path) else {
            return vec![format!("{}: not there", path.display())];
        };
        let by_default = text.lines().any(|line| {
            let code: String = line
                .split('#')
                .next()
                .unwrap_or_default()
                .split_whitespace()
                .collect();
            code.starts_with("default=[") && code != "default=[]"
        });
        let mut found = Vec::new();
        let mut built = false;
        let mut paths = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let at = format!("{}:{}", path.display(), n + 1);
            let code = line.split('#').next().unwrap_or_default().trim_end();
            if code.starts_with('[') {
                built = dependency_table(code).is_some() && !code.contains("dev-dependencies");
                if let Some((_, name)) = code
                    .trim_matches(|c| c == '[' || c == ']')
                    .rsplit_once("dependencies.")
                {
                    if built && engine(name) {
                        found.push(format!("{at}: builds on {name}"));
                    }
                }
                continue;
            }
            if !built
                || code.starts_with(|c: char| c.is_whitespace() || c == '"' || c == ']' || c == '}')
            {
                continue;
            }
            let Some((name, value)) = code.split_once('=') else {
                continue;
            };
            let name = name.trim().split('.').next().unwrap_or_default();
            if value.contains("optional = true") && !by_default {
                continue;
            }
            if engine(name) {
                found.push(format!("{at}: builds on {name}"));
            } else if let Some(dependency) = value
                .split("path = \"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
            {
                paths.push(
                    path.parent()
                        .unwrap_or(Path::new(""))
                        .join(dependency)
                        .join("Cargo.toml"),
                );
            }
        }
        for dependency in paths {
            found.extend(engine_reached(&dependency, read, seen));
        }
        found
    }

    /// ★ The client core builds without the engine: with no feature asked for, nothing it builds on
    /// is a crate of the engine (tempo-app, tempo-core, tempo-audio) or one that builds the decoders'
    /// Fortran or SQLite, so a phone can build it. The guard read exactly what such a build reads:
    /// the core, tempo-base, tempo-codec and tempo-stream, and not tempo-audio, whose sound devices
    /// are the `device` feature's. CONTROL: one planted line of each kind is named, alone: the
    /// engine in the core's manifest, the sound devices built always or turned on by default, SQLite
    /// in tempo-codec's, the decoders' Fortran in tempo-base's, and a decoder that brings it in
    /// from further down.
    #[test]
    fn the_client_core_builds_without_the_engine() {
        let from_disk = |path: &Path| std::fs::read_to_string(path).ok();
        let mut seen = BTreeSet::new();
        assert_eq!(
            engine_reached(&manifest(), &from_disk, &mut seen),
            Vec::<String>::new()
        );
        let read: BTreeSet<String> = seen
            .iter()
            .filter_map(|path| path.parent()?.file_name()?.to_str().map(str::to_string))
            .collect();
        assert_eq!(
            read,
            BTreeSet::from(
                ["tempo-base", "tempo-client", "tempo-codec", "tempo-stream"].map(String::from)
            )
        );

        let core = manifest();
        let crates = core.parent().unwrap().parent().unwrap();
        let base = crates.join("tempo-base/Cargo.toml");
        let codec = crates.join("tempo-codec/Cargo.toml");
        let planted: [(&Path, &str, &str, &str); 6] = [
            (
                &core,
                "[dependencies]\n",
                "[dependencies]\ntempo-core = { path = \"../tempo-core\" }\n",
                "tempo-core",
            ),
            (
                &core,
                "tempo-audio = { path = \"../tempo-audio\", optional = true }",
                "tempo-audio = { path = \"../tempo-audio\" }",
                "tempo-audio",
            ),
            (
                &core,
                "[features]\n",
                "[features]\ndefault = [\"device\"]\n",
                "tempo-audio",
            ),
            (
                &codec,
                "[dependencies]\n",
                "[dependencies]\nrusqlite = { version = \"0.37.0\", features = [\"bundled\"] }\n",
                "rusqlite",
            ),
            (
                &base,
                "[lints]\n",
                "[dependencies]\ntempo-fast-sys = { path = \"../tempo-fast-sys\" }\n\n[lints]\n",
                "tempo-fast-sys",
            ),
            (
                &core,
                "[dependencies]\n",
                "[dependencies]\nft8 = { path = \"../ft8\" }\n",
                "tempo-fast-sys",
            ),
        ];
        for (file, from, to, named) in planted {
            let text = std::fs::read_to_string(file).unwrap();
            assert!(
                text.contains(from),
                "{} has no {from:?} to plant in",
                file.display()
            );
            let changed = text.replacen(from, to, 1);
            let read = |path: &Path| {
                if std::fs::canonicalize(path).ok() == std::fs::canonicalize(file).ok() {
                    Some(changed.clone())
                } else {
                    std::fs::read_to_string(path).ok()
                }
            };
            let found = engine_reached(&manifest(), &read, &mut BTreeSet::new());
            assert_eq!(found.len(), 1, "planted {to:?}: {found:?}");
            assert!(
                found[0].ends_with(&format!(": builds on {named}")),
                "planted {to:?}: {found:?}"
            );
        }
    }
}
