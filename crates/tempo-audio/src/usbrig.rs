//! Zero-config station setup: identify a connected radio from its USB descriptor.
//!
//! Two pure, testable pieces (the actual USB enumeration lives in [`crate::ports`]
//! behind the `serial` feature, and the command layer joins them):
//!
//! 1. **Driver resolver** — most ham rigs talk over a generic USB-serial bridge
//!    chip (Silicon Labs CP210x, FTDI, WCH CH340, Prolific). The chip is identified
//!    by USB **vendor id**; when its port won't bind, [`driver_hint`] points the
//!    operator at the correct *official* driver for their OS.
//! 2. **Rig matcher** — native-USB rigs report their model in the USB **product**
//!    string (e.g. `"IC-705"`). [`match_rig_model`] fuzzy-matches that against the
//!    curated [`crate::rigmodels::rig_models`] table to pre-select the Hamlib model.
//!    Rigs behind a generic bridge report only the chip name → no rig match (just a
//!    driver hint), which is the honest result.

use crate::audiodev::AudioDevice;
use crate::rigmodels::rig_models;

/// A known USB-serial bridge-chip family (by USB vendor id).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbSerialChip {
    /// Silicon Labs CP210x — Icom, Yaesu (dual), Kenwood, Elecraft, Xiegu, …
    Cp210x,
    /// FTDI FT232/FT2232 — Elecraft, many interface cables.
    Ftdi,
    /// WCH CH340/CH341 — budget rigs + clone cables.
    Ch340,
    /// Prolific PL2303 — older cables (driver is Windows-version-sensitive).
    Prolific,
    /// An unrecognized / native-CDC device (no extra driver needed).
    Other,
}

/// Identify the USB-serial bridge chip from the device's USB **vendor id**. (PID is
/// not needed — the vendor id is what selects the driver family.)
pub fn usb_serial_chip(vid: u16) -> UsbSerialChip {
    match vid {
        0x10C4 => UsbSerialChip::Cp210x,   // Silicon Laboratories
        0x0403 => UsbSerialChip::Ftdi,     // Future Technology Devices Intl
        0x1A86 => UsbSerialChip::Ch340,    // QinHeng Electronics (WCH)
        0x067B => UsbSerialChip::Prolific, // Prolific Technology
        _ => UsbSerialChip::Other,
    }
}

/// Host OS family, for OS-aware driver guidance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostOs {
    Windows,
    MacOs,
    Linux,
}

/// The host OS this binary is running on (for the live "what do I need" answer).
pub fn current_os() -> HostOs {
    #[cfg(target_os = "windows")]
    {
        HostOs::Windows
    }
    #[cfg(target_os = "macos")]
    {
        HostOs::MacOs
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        HostOs::Linux
    }
}

/// Driver guidance for a bridge chip on a given OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverHint {
    /// Human chip name, e.g. "Silicon Labs CP210x".
    pub chip: &'static str,
    /// True when the OS ships the driver in-kernel (no install needed).
    pub bundled: bool,
    /// One-line guidance.
    pub note: &'static str,
    /// Official driver download URL (empty when bundled / not applicable).
    pub url: &'static str,
}

/// What driver (if any) an operator needs for `chip` on `os` when the rig's serial
/// port doesn't appear/bind. Returns `None` for [`UsbSerialChip::Other`] (a native
/// CDC device needs no extra driver). The judgement of "bundled" is the common case
/// for modern OS versions — the note says so rather than over-promising.
pub fn driver_hint(chip: UsbSerialChip, os: HostOs) -> Option<DriverHint> {
    use HostOs::*;
    use UsbSerialChip::*;
    Some(match (chip, os) {
        (Cp210x, Windows) => DriverHint {
            chip: "Silicon Labs CP210x",
            bundled: false,
            // Modern Win10/11 install the CP210x VCP driver automatically via Windows Update
            // (this is the FT-710 / FTDX10 / FT-991A built-in USB bridge), so DON'T tell the
            // operator they must go install it — that + the old /developer-tools/ URL (now
            // dead; Silicon Labs moved it to /software-and-tools/) sent FT-710 users chasing a
            // driver that isn't on the page. Word it conditionally, like the CH340/macOS arm.
            note: "Windows usually installs the Silicon Labs CP210x driver automatically — install it manually only if the COM port never appears.",
            url: "https://www.silabs.com/software-and-tools/usb-to-uart-bridge-vcp-drivers",
        },
        (Cp210x, MacOs | Linux) => DriverHint {
            chip: "Silicon Labs CP210x",
            bundled: true,
            note: "Your OS ships the CP210x driver in-kernel — no install needed.",
            url: "",
        },
        (Ftdi, Windows) => DriverHint {
            chip: "FTDI",
            bundled: false,
            note: "Windows needs the FTDI VCP driver — install it, then Retry.",
            url: "https://ftdichip.com/drivers/vcp-drivers/",
        },
        (Ftdi, MacOs | Linux) => DriverHint {
            chip: "FTDI",
            bundled: true,
            note: "Your OS ships the FTDI driver in-kernel — no install needed.",
            url: "",
        },
        (Ch340, Windows) => DriverHint {
            chip: "WCH CH340",
            bundled: false,
            note: "Windows needs the WCH CH340 (CH34x) driver — install it, then Retry.",
            url: "https://www.wch-ic.com/downloads/CH341SER_EXE.html",
        },
        (Ch340, Linux) => DriverHint {
            chip: "WCH CH340",
            bundled: true,
            note: "Linux ships the CH340 driver in-kernel — no install needed.",
            url: "",
        },
        (Ch340, MacOs) => DriverHint {
            chip: "WCH CH340",
            bundled: false,
            note: "Older macOS needs the WCH CH34x driver; recent macOS bundles it — install only if the port is missing.",
            url: "https://www.wch-ic.com/downloads/CH34XSER_MAC_ZIP.html",
        },
        (Prolific, Windows) => DriverHint {
            chip: "Prolific PL2303",
            bundled: false,
            note: "Windows needs the Prolific PL2303 driver matched to your chip revision — install it, then Retry.",
            url: "https://www.profilictech.com/",
        },
        (Prolific, MacOs | Linux) => DriverHint {
            chip: "Prolific PL2303",
            bundled: true,
            note: "Your OS ships the PL2303 driver in-kernel — no install needed.",
            url: "",
        },
        (Other, _) => return None,
    })
}

/// Normalize a model/product token for matching: keep ASCII alphanumerics only,
/// uppercased (so "IC-705", "ic 705", "IC705" all collapse to "IC705").
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase()
}

/// Manufacturer / non-model words to ignore when extracting a rig's model tokens
/// from a friendly name (so "Icom" in "Icom IC-705" never matches a product string).
const MAKER_WORDS: &[&str] = &[
    "ICOM",
    "YAESU",
    "KENWOOD",
    "ELECRAFT",
    "FLEXRADIO",
    "TENTEC",
    "XIEGU",
    "QRP",
    "LABS",
    "ALINCO",
    "HAMLIB",
    "FLRIG",
    "RIGCTL",
    "RIGCTLD",
    "REMOTE",
    "SAT",
    "EMUL",
    "SLICE",
    "POWERSDR",
    "SMARTSDR",
    "DUMMY",
];

/// Model tokens worth matching in a friendly name. Split on whitespace and "/" only
/// (NOT internal dashes) so a model stays whole — "IC-705" → "IC705", not "IC"+"705"
/// (a bare "IC" would false-match "silICon"). Drop maker words and require length ≥ 3
/// so short line-prefixes (K3/K4/TS) can't substring-match generic descriptors.
fn model_tokens(name: &str) -> Vec<String> {
    name.split(|c: char| c.is_whitespace() || c == '/')
        .map(normalize)
        .filter(|t| t.len() >= 3 && !MAKER_WORDS.contains(&t.as_str()))
        .collect()
}

/// Best Hamlib model guess from a USB **product** (and manufacturer) string. Native-
/// USB rigs report their model there (e.g. `"IC-705"`); generic bridges report only
/// the chip (e.g. `"CP2102 USB to UART Bridge"`) → `None`. Picks the LONGEST model
/// token that appears in the haystack so "K3S" beats "K3" and "IC-7610" beats noise.
/// Skips the Hamlib built-ins (Dummy/NET/FLRig, model ≤ 4) — never a physical USB rig.
pub fn match_rig_model(product: &str, manufacturer: &str) -> Option<(u32, &'static str)> {
    let hay = normalize(&format!("{manufacturer} {product}"));
    if hay.is_empty() {
        return None;
    }
    let mut best: Option<(usize, u32, &'static str)> = None;
    for (model, name) in rig_models() {
        if model <= 4 {
            continue;
        }
        for tok in model_tokens(name) {
            if hay.contains(&tok) && best.is_none_or(|(len, ..)| tok.len() > len) {
                best = Some((tok.len(), model, name));
            }
        }
    }
    best.map(|(_, m, n)| (m, n))
}

/// Which side of an Icom-style dual-UART USB pair a port is, judged from its product /
/// driver friendly name. The IC-7610/9700 built-in USB is a CP2105 dual UART: TWO COM
/// ports, and only ONE speaks CI-V. Icom's Windows driver labels them "… Serial Port A
/// (CI-V)" / "… Serial Port B"; the stock Silicon Labs driver labels them "Enhanced" /
/// "Standard" COM port (Enhanced carries CI-V). Picking the wrong one opens cleanly and
/// returns zero bytes forever — the exact signature of a dead rig.
///
/// `Some(true)` = the CI-V side; `Some(false)` = the second port that never answers
/// CI-V; `None` = no signal either way (single-port rigs, generic bridges). Matches are
/// deliberately conservative — a wrong `Some` here would steer the operator AWAY from
/// the working port.
pub fn civ_port_side(product: &str) -> Option<bool> {
    let p = product.to_ascii_uppercase();
    if p.contains("CI-V") || p.contains("ENHANCED") || p.contains("SERIAL PORT A") {
        return Some(true);
    }
    if p.contains("STANDARD COM") || p.contains("SERIAL PORT B") {
        return Some(false);
    }
    None
}

/// A recognised sound-card INTERFACE (Digirig, RigBlaster…) — a cable between the PC and a
/// radio, NOT a radio. Deliberately carries no rig model: an interface can be wired to any
/// radio, and claiming one would be a guess that silently mis-configures CAT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownInterface {
    /// Display name for the Detect list, e.g. "Digirig Mobile".
    pub name: &'static str,
    /// The `ptt_method` to pre-fill — "rts" for every interface here (they all key a serial
    /// control line). Never "cat": the interface cannot key the rig's CAT channel for it.
    pub ptt_method: &'static str,
    /// Does keying share the CAT serial port? `Some(true)` = leave "PTT Serial Port" BLANK
    /// (the single-cable wiring; see `service::keys_on_the_cat_port`). `Some(false)` = it has
    /// its own keying port. `None` = varies by model, so ASK rather than pre-fill — an
    /// interface guessed wrong here keys the wrong thing, which is a TX-path error.
    pub shares_cat_port: Option<bool>,
    /// One plain sentence for the operator, shown beside the detected port.
    pub note: &'static str,
}

/// Recognise a known interface cable from its USB identity.
///
/// ⚠️ **MATCHES ON THE PRODUCT/MANUFACTURER STRING ONLY — never on VID/PID.** A stock Digirig
/// is a Silicon Labs CP2102, `10C4:EA60`, which is the SAME VID/PID as an FTDX10, an FT-710 and
/// several Xiegu radios. Keying off the numbers would confidently label a working FTDX10 as an
/// interface cable and pre-fill the wrong PTT method — worse than not recognising it at all.
/// So: an interface that does not NAME itself is correctly returned as `None`, and the operator
/// configures it by hand exactly as before. `vid`/`pid` are accepted only to confirm a name
/// match, never to make one.
pub fn match_interface(
    vid: u16,
    _pid: u16,
    product: &str,
    manufacturer: &str,
) -> Option<KnownInterface> {
    let hay = format!("{manufacturer} {product}").to_ascii_uppercase();
    // Digirig Mobile: ONE USB port carrying CAT and RTS keying, plus its own codec. This is
    // the wiring `service::keys_on_the_cat_port` exists for.
    if hay.contains("DIGIRIG") {
        let lite = hay.contains("LITE");
        return Some(KnownInterface {
            name: if lite {
                "Digirig Lite"
            } else {
                "Digirig Mobile"
            },
            ptt_method: "rts",
            // Lite keys via CM108 HID, which Nexus does not implement — say so rather than
            // pre-filling a method that would never key.
            shares_cat_port: if lite { None } else { Some(true) },
            note: if lite {
                "Digirig Lite keys over CM108 HID, which Nexus does not support yet — use VOX, \
                 or a rig that keys over CAT."
            } else {
                "One cable for CAT and keying: leave PTT Serial Port blank and Nexus keys RTS \
                 on the CAT port."
            },
        });
    }
    // West Mountain RIGblaster. The family spans PTT-only boxes (Plug & Play) and CAT+PTT ones
    // (Advantage), so the port question genuinely varies — `None` means ask, don't guess.
    if hay.contains("RIGBLASTER") || hay.contains("WEST MOUNTAIN") {
        return Some(KnownInterface {
            name: "West Mountain RIGblaster",
            ptt_method: "rts",
            shares_cat_port: None,
            note: "Keys RTS on a serial port. Which port depends on your model — if CAT and \
                   keying are one cable, leave PTT Serial Port blank.",
        });
    }
    // A bare bridge chip cannot be identified further, and MUST NOT be guessed at: see the
    // shared-VID/PID warning above. `vid` is referenced so the intent is explicit.
    let _ = vid;
    None
}

/// A fully-resolved detection result for one connected USB radio — everything the
/// setup wizard needs to one-click configure it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedRig {
    pub port_name: String,
    pub vid: u16,
    pub pid: u16,
    pub product: String,
    pub manufacturer: String,
    /// Hamlib model guessed from the product string (None = couldn't identify the
    /// rig, only the bridge chip — the operator picks the model, `driver` still helps).
    pub suggested_model: Option<u32>,
    pub suggested_model_name: Option<&'static str>,
    pub chip: UsbSerialChip,
    /// Driver guidance when the chip needs one on this OS (None = native/bundled).
    pub driver: Option<DriverHint>,
    /// Best-guess paired CAPTURE device (the rig's USB-Audio CODEC input), by name.
    pub suggested_audio: Option<String>,
    /// Best-guess paired PLAYBACK device, matched against the OUTPUT list — on Windows the rig's
    /// CODEC input and output enumerate under DIFFERENT names, so reusing the input name for TX
    /// sent modem audio to the PC speakers instead of the rig ("TX out the speakers" bug).
    pub suggested_audio_out: Option<String>,
    /// Set when this port is a recognised INTERFACE CABLE rather than a radio (see
    /// [`match_interface`]). Mutually exclusive with `suggested_model` in practice: an interface
    /// names itself and no rig token matches, while a native-USB radio names its model. When
    /// present the operator still picks the RIG — the cable does not imply one.
    pub interface: Option<KnownInterface>,
    /// For dual-UART rigs (IC-7610/9700): which of the pair this is — `Some(true)` = the
    /// CI-V/CAT side, `Some(false)` = the second port that never answers CI-V (see
    /// [`civ_port_side`]). Lets the UI break the tie between two rows that otherwise both
    /// say "Icom IC-7610".
    pub civ_side: Option<bool>,
}

/// Join enumerated USB ports + audio device names into per-rig suggestions. Pure, so
/// the matching/pairing is testable; the command layer supplies the live enumeration.
pub fn detect_rigs(
    ports: &[crate::ports::UsbPort],
    audio_in: &[AudioDevice],
    audio_out: &[AudioDevice],
    os: HostOs,
) -> Vec<DetectedRig> {
    ports
        .iter()
        .map(|p| {
            let (suggested_model, suggested_model_name) =
                match match_rig_model(&p.product, &p.manufacturer) {
                    Some((m, n)) => (Some(m), Some(n)),
                    None => (None, None),
                };
            let chip = usb_serial_chip(p.vid);
            DetectedRig {
                port_name: p.port_name.clone(),
                vid: p.vid,
                pid: p.pid,
                product: p.product.clone(),
                manufacturer: p.manufacturer.clone(),
                suggested_model,
                suggested_model_name,
                chip,
                driver: driver_hint(chip, os),
                suggested_audio: pair_audio(&p.product, audio_in),
                suggested_audio_out: pair_audio(&p.product, audio_out),
                interface: match_interface(p.vid, p.pid, &p.product, &p.manufacturer),
                civ_side: civ_port_side(&p.product),
            }
        })
        .collect()
}

/// Pick the sound device most likely to be this rig's USB-Audio CODEC: prefer a
/// device whose name references the rig's product/model, else the most specific generic
/// "USB Audio CODEC" match (the near-universal FT8 rig-audio device name). `None` if
/// neither. Returns the device's `name` — the STORABLE string, never the label.
///
/// The matching runs against the operator-facing `label`, which is why this works on Linux
/// at all: cpal names an ALSA device `plughw:CARD=CODEC,DEV=0`, which no pattern here has
/// ever matched, so zero-config rig-audio pairing silently did nothing on every Linux box
/// until the picker started carrying descriptions alongside names.
fn pair_audio(product: &str, audio: &[AudioDevice]) -> Option<String> {
    let pn = normalize(product);
    if !pn.is_empty() {
        // Product pass: label AND name. On Linux the ALSA card id is often derived from
        // the USB product string ("plughw:CARD=FTDX10,DEV=0"), so the name is a second
        // real signal rather than noise.
        if let Some(a) = audio
            .iter()
            .find(|a| normalize(&format!("{} {}", a.label, a.name)).contains(&pn))
        {
            return Some(a.name.clone());
        }
    }
    // Generic fallback: LABEL ONLY, and by TIER. This pass also fills audioOut, where a
    // false positive is the "TX out the PC speakers" class, so it stays narrow.
    audio
        .iter()
        .filter_map(|a| generic_codec_tier(&a.label).map(|t| (t, a)))
        .min_by_key(|(t, _)| *t)
        .map(|(_, a)| a.name.clone())
}

/// How specifically does this sound-device name look like a rig-audio USB codec? Lower is
/// more specific; `None` = not a rig codec at all. Used only as the FALLBACK, after a
/// product-name match fails.
///
/// "USB AUDIO CODEC" is the near-universal name for a radio's built-in codec and for most
/// RigBlaster models. It is NOT what an outboard interface cable presents: a Digirig Mobile
/// enumerates on Windows as **"USB PnP Sound Device"** and on Linux/macOS often as a bare
/// "USB Audio Device", so neither existing pattern matched and Detect paired nothing — the
/// operator had to find the device by hand with no hint which of several it was.
///
/// ⚠️ **Tiered, not a flat OR** — the flat version took the FIRST device matching ANY
/// pattern, which is list order. On the reported 8-card Linux box the input list runs
/// "…USB Audio Device… USB AUDIO CODEC", so once labels became real the broad
/// `USB AUDIO DEVICE` pattern would have paired card 3 instead of the FTDX10: pairing
/// nothing would have become pairing the WRONG thing, which is worse. Ranking fixes that
/// on Windows too, though only where 2+ generic USB audio devices exist — and there
/// today's answer is arbitrary list order anyway.
///
/// Deliberately conservative. Every pattern here still contains "USB", so a built-in laptop
/// mic/speaker (Realtek, "Microphone Array", HDMI) can never win the fallback; and because this
/// only runs after the product-name pass, a rig that DOES name itself is unaffected.
fn generic_codec_tier(name: &str) -> Option<u8> {
    let n = name.to_ascii_uppercase();
    if n.contains("USB AUDIO CODEC") || n.contains("USB CODEC") {
        Some(0)
    } else if n.contains("USB PNP SOUND DEVICE") {
        // Digirig Mobile / Digirig Lite / CM108-class dongles.
        Some(1)
    } else if n.contains("USB AUDIO DEVICE") {
        // Generic enumeration used by several interface cables and by ALSA for the same devices.
        Some(2)
    } else if n.contains("USB AUDIO") {
        // Kept last and broadest: the historical pattern, which also covers names like
        // "USB Audio CODEC #2" that the duplicate-name disambiguator produces.
        Some(3)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::UsbPort;

    #[test]
    fn chip_id_from_vendor() {
        assert_eq!(usb_serial_chip(0x10C4), UsbSerialChip::Cp210x);
        assert_eq!(usb_serial_chip(0x0403), UsbSerialChip::Ftdi);
        assert_eq!(usb_serial_chip(0x1A86), UsbSerialChip::Ch340);
        assert_eq!(usb_serial_chip(0x067B), UsbSerialChip::Prolific);
        assert_eq!(usb_serial_chip(0x1234), UsbSerialChip::Other);
    }

    #[test]
    fn driver_hint_os_aware() {
        // Windows needs a download; *nix/mac bundle CP210x.
        let w = driver_hint(UsbSerialChip::Cp210x, HostOs::Windows).unwrap();
        assert!(!w.bundled && w.url.contains("silabs"));
        assert!(
            driver_hint(UsbSerialChip::Cp210x, HostOs::Linux)
                .unwrap()
                .bundled
        );
        assert!(
            driver_hint(UsbSerialChip::Ftdi, HostOs::MacOs)
                .unwrap()
                .bundled
        );
        // A native CDC device needs nothing.
        assert_eq!(driver_hint(UsbSerialChip::Other, HostOs::Windows), None);
    }

    #[test]
    fn matches_native_usb_rig_from_product_string() {
        assert_eq!(
            match_rig_model("IC-705", "Icom Inc."),
            Some((3085, "Icom IC-705"))
        );
        assert_eq!(match_rig_model("IC-7300", ""), Some((3073, "Icom IC-7300")));
        // Case / spacing / dash insensitive.
        assert_eq!(
            match_rig_model("ft991a", "Yaesu").map(|(m, _)| m),
            Some(1035)
        );
        assert_eq!(
            match_rig_model("TS-590SG", "Kenwood").map(|(m, _)| m),
            Some(2037)
        );
    }

    #[test]
    fn longest_token_wins_for_overlapping_models() {
        // "K3S" must beat "K3" (Elecraft) — the longer, more specific token.
        assert_eq!(
            match_rig_model("Elecraft K3S", "").map(|(m, _)| m),
            Some(2043)
        );
    }

    #[test]
    fn generic_bridge_product_is_no_rig_match() {
        // A generic CP210x descriptor identifies the chip, not the rig.
        assert_eq!(
            match_rig_model("CP2102 USB to UART Bridge Controller", "Silicon Labs"),
            None
        );
        assert_eq!(match_rig_model("USB-Serial Controller", "Prolific"), None);
        assert_eq!(match_rig_model("", ""), None);
    }

    fn port(name: &str, vid: u16, product: &str, maker: &str) -> UsbPort {
        UsbPort {
            port_name: name.into(),
            vid,
            pid: 0xEA60,
            product: product.into(),
            manufacturer: maker.into(),
        }
    }

    /// ⚠️ THE TRAP THIS TABLE EXISTS TO AVOID. A stock Digirig is a Silicon Labs CP2102,
    /// `10C4:EA60` — the SAME VID/PID as an FTDX10, an FT-710 and several Xiegu radios. If
    /// `match_interface` ever keys off the numbers, a working FTDX10 gets labelled an interface
    /// cable and pre-filled with the wrong PTT method. Not recognising a device is a mild
    /// inconvenience; mislabelling a radio is a TX-path misconfiguration.
    #[test]
    fn a_bare_bridge_chip_is_never_claimed_as_an_interface() {
        // Same VID/PID as a Digirig, but the product string names a chip, not a cable.
        assert_eq!(
            match_interface(
                0x10C4,
                0xEA60,
                "CP2102 USB to UART Bridge Controller",
                "Silicon Labs"
            ),
            None
        );
        // Same VID/PID again, but this time it IS a radio.
        assert_eq!(
            match_interface(0x10C4, 0xEA60, "FTDX10", "Yaesu"),
            None,
            "a radio on the shared CP2102 id must never be called an interface"
        );
        assert_eq!(
            match_interface(0x0403, 0x6001, "FT232R USB UART", "FTDI"),
            None
        );
    }

    #[test]
    fn known_interfaces_are_matched_by_name_and_carry_no_rig_model() {
        let d = match_interface(0x10C4, 0xEA60, "Digirig Mobile", "Digirig")
            .expect("a device that names itself Digirig is recognisable");
        assert_eq!(d.name, "Digirig Mobile");
        assert_eq!(d.ptt_method, "rts");
        assert_eq!(
            d.shares_cat_port,
            Some(true),
            "the defining Digirig Mobile wiring: one cable for CAT and keying"
        );

        // Lite keys via CM108 HID, which Nexus does not implement. It must NOT claim a serial
        // wiring that would never key — `None` means "ask", and the note says why.
        let lite = match_interface(0x10C4, 0xEA60, "Digirig Lite", "Digirig").unwrap();
        assert_eq!(lite.shares_cat_port, None);
        assert!(lite.note.contains("CM108"));

        let rb = match_interface(
            0x0403,
            0x6001,
            "RIGblaster Advantage",
            "West Mountain Radio",
        )
        .expect("RIGblaster is recognisable by name");
        assert_eq!(rb.ptt_method, "rts");
        assert_eq!(
            rb.shares_cat_port, None,
            "the RIGblaster family spans PTT-only and CAT+PTT boxes — ask, do not guess"
        );
    }

    /// A Windows/macOS device, where cpal's name IS the friendly string (label == name).
    fn wdev(name: &str) -> AudioDevice {
        AudioDevice {
            name: name.to_string(),
            label: name.to_string(),
        }
    }
    /// A Linux device: the ALSA PCM name addresses it, the card description names it.
    fn ldev(name: &str, label: &str) -> AudioDevice {
        AudioDevice {
            name: name.to_string(),
            label: label.to_string(),
        }
    }

    /// Digirig's codec matched NEITHER prior pattern, so Detect paired no audio at all and the
    /// operator had to guess which device was the radio.
    #[test]
    fn interface_codecs_pair_as_rig_audio() {
        for name in [
            "USB PnP Sound Device", // Digirig on Windows
            "USB Audio Device",     // several cables, and ALSA for the same hardware
            "USB Audio CODEC",      // built-in rig codecs, most RigBlasters
            "USB Audio CODEC #2",   // the duplicate-name disambiguator
        ] {
            assert!(
                generic_codec_tier(name).is_some(),
                "{name} should be recognised as rig audio"
            );
        }
        // Must NOT capture the PC's own hardware — this only runs as a fallback, but a false
        // positive here sends modem audio to the laptop speakers (the "TX out the speakers" bug).
        for name in [
            "Realtek High Definition Audio",
            "Microphone Array (Intel Smart Sound)",
            "Speakers (Realtek)",
            "HDMI Output",
        ] {
            assert!(
                generic_codec_tier(name).is_none(),
                "{name} is not rig audio"
            );
        }
    }

    /// THE FIELD REPORT'S THIRD CASUALTY: zero-config rig-audio pairing on Linux, which has
    /// never once worked — every pattern is written for a description ("USB AUDIO CODEC")
    /// and cpal only ever offered the PCM name ("plughw:CARD=CODEC,DEV=0"), so Detect Rigs
    /// paired the CAT port and left audio blank on every Linux box.
    ///
    /// The FTDX10 hides behind a generic CP2102 bridge, so its product string names no rig
    /// and this must resolve through the generic fallback — and it must reach the CODEC and
    /// not the plain "USB Audio Device" that sorts ahead of it on this operator's machine.
    #[test]
    fn linux_pairs_the_rig_codec_by_its_description_not_its_pcm_name() {
        let ports = vec![port(
            "/dev/ttyUSB0",
            0x10C4,
            "CP2102 USB to UART Bridge Controller",
            "Silicon Labs",
        )];
        // His real pruned list order: card 3 first, the FTDX10 (card 7) last.
        let audio = vec![
            ldev("plughw:CARD=Generic,DEV=0", "HD-Audio Generic"),
            ldev("plughw:CARD=Device,DEV=0", "USB Audio Device"),
            ldev("plughw:CARD=CODEC,DEV=0", "USB AUDIO CODEC"),
            ldev("pipewire", "PipeWire Sound Server"),
        ];
        let got = detect_rigs(&ports, &audio, &audio, HostOs::Linux);
        // The STORABLE ALSA name comes back, never the label — a label cannot address a device.
        assert_eq!(
            got[0].suggested_audio.as_deref(),
            Some("plughw:CARD=CODEC,DEV=0")
        );
        assert_eq!(
            got[0].suggested_audio_out.as_deref(),
            Some("plughw:CARD=CODEC,DEV=0")
        );
    }

    /// The tiering, on its own: with two generic USB audio devices present, the more
    /// specific "CODEC" wins wherever it sits in the list. A flat first-match OR would take
    /// list order and pair the wrong radio.
    #[test]
    fn the_more_specific_generic_codec_wins_regardless_of_list_order() {
        let ports = vec![port(
            "COM9",
            0x10C4,
            "CP2102 USB to UART Bridge",
            "Silicon Labs",
        )];
        let audio = vec![wdev("USB Audio Device"), wdev("USB Audio CODEC")];
        assert_eq!(
            detect_rigs(&ports, &audio, &audio, HostOs::Windows)[0]
                .suggested_audio
                .as_deref(),
            Some("USB Audio CODEC")
        );
        // ...and the Digirig dongle still wins over a bare "USB Audio Device".
        let audio = vec![wdev("USB Audio Device"), wdev("USB PnP Sound Device")];
        assert_eq!(
            detect_rigs(&ports, &audio, &audio, HostOs::Windows)[0]
                .suggested_audio
                .as_deref(),
            Some("USB PnP Sound Device")
        );
    }

    #[test]
    fn detect_native_usb_rig_full_resolution() {
        // An IC-705 (native USB, Silicon Labs bridge) + its USB-Audio CODEC.
        let ports = vec![port("COM5", 0x10C4, "IC-705", "Icom Inc.")];
        // Windows enumerates the rig's CODEC under DIFFERENT input vs output names.
        let audio_in = vec![wdev("Microphone (USB Audio CODEC)"), wdev("Realtek HD")];
        let audio_out = vec![wdev("Speakers (USB Audio CODEC)"), wdev("Realtek HD")];
        let got = detect_rigs(&ports, &audio_in, &audio_out, HostOs::Windows);
        assert_eq!(got.len(), 1);
        let r = &got[0];
        assert_eq!(r.suggested_model, Some(3085));
        assert_eq!(r.suggested_model_name, Some("Icom IC-705"));
        assert_eq!(r.chip, UsbSerialChip::Cp210x);
        // Windows → CP210x driver hint present.
        assert!(r.driver.as_ref().is_some_and(|d| !d.bundled));
        assert_eq!(
            r.suggested_audio.as_deref(),
            Some("Microphone (USB Audio CODEC)")
        );
        // The fix: TX device is matched against the OUTPUT list (the speaker-side CODEC), not the
        // mic — otherwise applying the input name to audioOut sent TX audio to the PC speakers.
        assert_eq!(
            r.suggested_audio_out.as_deref(),
            Some("Speakers (USB Audio CODEC)")
        );
    }

    #[test]
    fn detect_generic_bridge_gives_driver_only() {
        // A CH340-cabled rig that reports only the chip → no model, but a driver hint
        // and (on Linux) bundled. No audio match → None.
        let ports = vec![port("/dev/ttyUSB0", 0x1A86, "USB Serial", "wch.cn")];
        let audio = vec![wdev("Built-in Audio")];
        let got = detect_rigs(&ports, &audio, &audio, HostOs::Linux);
        assert_eq!(got[0].suggested_model, None);
        assert_eq!(got[0].chip, UsbSerialChip::Ch340);
        assert!(got[0].driver.as_ref().is_some_and(|d| d.bundled)); // Linux ships CH340
        assert_eq!(got[0].suggested_audio, None);
        assert_eq!(got[0].suggested_audio_out, None);
    }

    /// The IC-7610 pair: both rows resolve to the same model, so the A/B discriminator in
    /// the friendly name is the ONLY thing that breaks the operator's coin flip. It must
    /// survive both driver namings — and stay `None` for everything else, because a wrong
    /// `Some` steers the operator away from the working port.
    #[test]
    fn civ_side_is_read_from_the_dual_uart_friendly_names() {
        // Icom's own driver naming.
        assert_eq!(civ_port_side("IC-7610 Serial Port A (CI-V)"), Some(true));
        assert_eq!(civ_port_side("IC-7610 Serial Port B"), Some(false));
        // Stock Silicon Labs CP2105 naming.
        assert_eq!(
            civ_port_side("CP2105 Dual USB to UART Bridge: Enhanced COM Port"),
            Some(true)
        );
        assert_eq!(
            civ_port_side("CP2105 Dual USB to UART Bridge: Standard COM Port"),
            Some(false)
        );
        // Single-port rigs and generic bridges carry no signal — no guess.
        assert_eq!(civ_port_side("IC-705"), None);
        assert_eq!(civ_port_side("CP2102 USB to UART Bridge Controller"), None);
        assert_eq!(civ_port_side(""), None);
    }

    #[test]
    fn detect_carries_the_civ_side_for_a_dual_port_icom() {
        let ports = vec![
            port("COM4", 0x10C4, "IC-7610 Serial Port B", "Icom Inc."),
            port("COM3", 0x10C4, "IC-7610 Serial Port A (CI-V)", "Icom Inc."),
        ];
        let got = detect_rigs(&ports, &[], &[], HostOs::Windows);
        assert_eq!(got.len(), 2);
        // Both identify as the same rig — WITHOUT civ_side these rows are identical.
        assert_eq!(got[0].suggested_model, Some(3078));
        assert_eq!(got[1].suggested_model, Some(3078));
        assert_eq!(got[0].civ_side, Some(false));
        assert_eq!(got[1].civ_side, Some(true));
    }

    #[test]
    fn pair_audio_prefers_model_named_device_over_generic() {
        let audio = vec![wdev("Generic USB Audio"), wdev("IC-705 Audio")];
        assert_eq!(
            pair_audio("IC-705", &audio).as_deref(),
            Some("IC-705 Audio")
        );
        // No model-named device → falls back to the generic USB-audio device.
        assert_eq!(
            pair_audio("FT-991A", &[wdev("Generic USB Audio")]).as_deref(),
            Some("Generic USB Audio"),
        );
    }
}
