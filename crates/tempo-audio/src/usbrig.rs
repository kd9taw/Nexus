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

/// The USB identity split the SAME way a catalog name is — on whitespace and "/", each piece
/// normalized. Empty pieces dropped.
///
/// ⚠️ **THE SPLIT IS THE POINT.** This used to be one `normalize`d blob of
/// `manufacturer + product`, and a blob has no word boundaries left in it: `"Lab599"` and
/// `"TX-500"` fused into `LAB599TX500`, where the Ten-Tec Eagle's token `599` is a substring.
/// A Lab599 Discovery TX-500 therefore prefilled `(16013, "Ten-Tec Eagle (599)")`.
fn descriptor_tokens(manufacturer: &str, product: &str) -> Vec<String> {
    format!("{manufacturer} {product}")
        .split(|c: char| c.is_whitespace() || c == '/')
        .map(normalize)
        .filter(|t| !t.is_empty())
        .collect()
}

/// Does the catalog token `model_tok` claim the USB descriptor token `hay_tok`?
///
/// **Prefix, and never extended by a digit.** Two rules, one per false positive measured
/// against this catalog:
/// - *Prefix, not substring anywhere*: `599` does not claim `LAB599`. A model number is the
///   start of the word that names the radio, never buried inside someone else's.
/// - *Not extended by a digit*: `IC910` does not claim `IC9100`. Those are two different
///   Icoms, and the corpus contains only the first — so an IC-9100 that names itself
///   correctly used to be prefilled as an IC-910, and `port_prober` then dropped the
///   operator's own model for it.
///
/// A LETTER may still extend it, because that is how rig families are spelled and the catalog
/// name is often the shorter one: `TS590S` claims `TS-590SG`, `FT991` claims `FT-991A` (where
/// the same entry's longer token `FT991A` then wins on length).
fn token_claims(hay_tok: &str, model_tok: &str) -> bool {
    hay_tok
        .strip_prefix(model_tok)
        .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_digit()))
}

/// Best Hamlib model guess from a USB **product** (and manufacturer) string. Native-
/// USB rigs report their model there (e.g. `"IC-705"`); generic bridges report only
/// the chip (e.g. `"CP2102 USB to UART Bridge"`) → `None`. Picks the LONGEST model
/// token that CLAIMS one of the descriptor's own tokens ([`token_claims`]) so "K3S"
/// beats "K3" and "IC-7610" beats noise.
///
/// **The corpus is narrower than the catalog, and deliberately.** Two kinds of entry are
/// skipped because neither can ever BE the USB device in front of us, so any token of theirs
/// that matches is a false positive by construction: the Hamlib built-ins (Dummy/NET/FLRig,
/// model ≤ 4), and the software-served profiles ([`crate::rigmodels::is_software_cat_profile`]
/// — where the program is the rig, reached over TCP or a virtual COM pair).
pub fn match_rig_model(product: &str, manufacturer: &str) -> Option<(u32, &'static str)> {
    match_rig_model_in(product, manufacturer, &rig_models())
}

/// [`match_rig_model`] against an arbitrary catalog, so the corpus property can be checked
/// against a DELIBERATELY OFFENDING one. The list is hand-maintained and the exclusion list
/// beside it is too; nothing in the types couples them, so the only thing that can catch the
/// next tier move is a guard that runs the real matcher over a poisoned catalog and watches
/// it re-open the false positive. See `no_verified_tier_name_claims_a_device_that_is_not_that_radio`.
fn match_rig_model_in(
    product: &str,
    manufacturer: &str,
    catalog: &[(u32, &'static str)],
) -> Option<(u32, &'static str)> {
    let hay = descriptor_tokens(manufacturer, product);
    if hay.is_empty() {
        return None;
    }
    let mut best: Option<(usize, u32, &'static str)> = None;
    for &(model, name) in catalog {
        if model <= 4 || crate::rigmodels::is_software_cat_profile(model) {
            continue;
        }
        for tok in model_tokens(name) {
            let claims = hay.iter().any(|h| token_claims(h, &tok));
            if claims && best.is_none_or(|(len, ..)| tok.len() > len) {
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
    let mut rigs: Vec<DetectedRig> = ports
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
                // Product-named pairing only — the generic fallback moved to the
                // roster-wide assignment pass below, which is what stops two rigs
                // sharing one codec.
                suggested_audio: pair_audio_product(&p.product, audio_in),
                suggested_audio_out: pair_audio_product(&p.product, audio_out),
                interface: match_interface(p.vid, p.pid, &p.product, &p.manufacturer),
                civ_side: civ_port_side(&p.product),
            }
        })
        .collect();
    assign_generic_codecs(&mut rigs, audio_in, true);
    assign_generic_codecs(&mut rigs, audio_out, false);
    rigs
}

/// The roster-wide half of codec pairing — generic "USB Audio CODEC"-class devices handed
/// out **1:1 across physical rigs**, never shared (setup re-envisioning, 2026-08-09).
///
/// The per-row fallback broke ties by list order, so a dual-Yaesu station — two identical
/// codecs — paired BOTH rigs onto the first one, and radio 2 transmitted into radio 1's
/// sound card with nothing on screen saying so. Handing the pool out 1:1 turns the worst
/// case into a possible SWAP (visible; one binary question in the wizard fixes it) instead
/// of a silent share. Three rules:
///
/// - A product-named pick (already made in `detect_rigs`) is OWNED: its device leaves the
///   pool, and its rig consumes nothing further.
/// - A dual-UART pair (`civ_side` tagged, same VID/PID — two rows describing ONE radio)
///   consumes ONE device, assigned to both rows: the rows are alternatives, not two rigs.
/// - When the pool runs dry the surplus rig pairs NOTHING — honest, and the operator picks
///   by hand; sharing would recreate the bug this pass exists to kill.
fn assign_generic_codecs(rigs: &mut [DetectedRig], audio: &[AudioDevice], input: bool) {
    // The pool, best-tier first, original order within a tier (stable, like the old
    // fallback) — minus every product-owned device.
    let owned: Vec<String> = rigs
        .iter()
        .filter_map(|r| {
            if input {
                r.suggested_audio.clone()
            } else {
                r.suggested_audio_out.clone()
            }
        })
        .collect();
    let mut ranked: Vec<(u8, String)> = audio
        .iter()
        .filter_map(|a| generic_codec_tier(&a.label).map(|t| (t, a.name.clone())))
        .filter(|(_, n)| !owned.contains(n))
        .collect();
    ranked.sort_by_key(|(t, _)| *t);
    let mut pool = ranked.into_iter().map(|(_, n)| n);
    // Hand out per PHYSICAL rig, in roster order. A dual-UART pair shares one grant,
    // keyed by (vid, pid) — civ_port_side only tags the known dual-UART products, so
    // two SEPARATE identical Icoms are not conflated (they enumerate as two pairs on
    // distinct bus addresses but identical VID/PID; the tag + first-come grant keeps
    // the pair case right and two-full-Icoms degrades to a shared suggestion, which
    // the wizard's confirm step surfaces rather than hides).
    let mut pair_grant: Vec<((u16, u16), Option<String>)> = Vec::new();
    for i in 0..rigs.len() {
        let already = if input {
            rigs[i].suggested_audio.is_some()
        } else {
            rigs[i].suggested_audio_out.is_some()
        };
        if already {
            continue; // product-owned
        }
        let grant = if rigs[i].civ_side.is_some() {
            let key = (rigs[i].vid, rigs[i].pid);
            match pair_grant.iter().find(|(k, _)| *k == key) {
                Some((_, g)) => g.clone(),
                None => {
                    let g = pool.next();
                    pair_grant.push((key, g.clone()));
                    g
                }
            }
        } else {
            pool.next()
        };
        if input {
            rigs[i].suggested_audio = grant;
        } else {
            rigs[i].suggested_audio_out = grant;
        }
    }
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
    pair_audio_product(product, audio).or_else(|| {
        // Generic fallback: LABEL ONLY, and by TIER. This pass also fills audioOut, where a
        // false positive is the "TX out the PC speakers" class, so it stays narrow. For a
        // ROSTER, `assign_generic_codecs` supersedes this (1:1 across rigs); this single-rig
        // form remains for callers pairing one product in isolation.
        audio
            .iter()
            .filter_map(|a| generic_codec_tier(&a.label).map(|t| (t, a)))
            .min_by_key(|(t, _)| *t)
            .map(|(_, a)| a.name.clone())
    })
}

/// The product-named half of [`pair_audio`]: a device whose label/name references the
/// rig's product. This is the OWNED case in the roster-wide assignment.
fn pair_audio_product(product: &str, audio: &[AudioDevice]) -> Option<String> {
    let pn = normalize(product);
    if pn.is_empty() {
        return None;
    }
    // Product pass: label AND name. On Linux the ALSA card id is often derived from
    // the USB product string ("plughw:CARD=FTDX10,DEV=0"), so the name is a second
    // real signal rather than noise.
    audio
        .iter()
        .find(|a| normalize(&format!("{} {}", a.label, a.name)).contains(&pn))
        .map(|a| a.name.clone())
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
        // A LETTER may extend a model token. Catalog names are often the shorter spelling of
        // a family, so a descriptor that adds a trailing letter must still resolve — here on
        // an entry that has no longer sibling in the catalog to absorb it.
        assert_eq!(
            match_rig_model("IC-910H", "Icom Inc.").map(|(m, _)| m),
            Some(3044)
        );
        // A DIGIT may not. The IC-9100 is a different radio and is not in the corpus at all,
        // so the honest answer is "I don't know" — see
        // `no_verified_tier_name_claims_a_device_that_is_not_that_radio`.
        assert_eq!(match_rig_model("IC-9100", "Icom Inc."), None);
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

    /// ⚠️ A CATALOG ENTRY IS ALSO AUTO-DETECT CORPUS. Adding the SDR-console profiles put
    /// their names into `model_tokens`, which is matched against the tokens of
    /// `manufacturer + product` — so `LITE`, `SDR`, `HPSDR`, `ANAN`, `APACHE`, `HERMES` and
    /// `FLEX` became live claims about USB hardware. Measured on the shipped build: a
    /// **Digirig Lite** (a cable this crate already recognises by name, `match_interface`)
    /// came back as model 2054 "Thetis", and an SDRplay RSPdx as 2056 "SDR Console".
    ///
    /// That is not one wrong label. `detect_rigs` feeds `suggested_model` straight into the
    /// Rig Model field, and `port_prober::candidates_from` treats a name match as the
    /// "exact, trusted" candidate and DISCARDS the operator's configured model — so an
    /// IC-7300 behind a Digirig Lite would sweep every baud as a Thetis.
    ///
    /// None of these profiles is a USB device at all: the program IS the rig, reached over
    /// TCP or a virtual COM pair. They are excluded from the corpus by
    /// [`crate::rigmodels::is_software_cat_profile`] — the names stay as written, because the
    /// names are what an HL2 owner reads in the dropdown.
    #[test]
    fn a_software_cat_profile_is_never_matched_from_a_usb_descriptor() {
        // The three measured false positives.
        assert_eq!(
            match_rig_model("Digirig Lite", "Digirig"),
            None,
            "a Digirig Lite is a cable, not a Thetis"
        );
        assert_eq!(match_rig_model("SDRplay RSPdx", "SDRplay Ltd"), None);
        assert_eq!(match_rig_model("RS-HFIQ SDR", ""), None);
        // The tokens each new name contributed, one product string apiece.
        for (product, maker) in [
            ("ANAN-7000DLE", "Apache Labs"),
            ("Hermes Lite 2", "Hermes"),
            ("HPSDR interface", ""),
            ("FLEX control cable", ""),
        ] {
            assert_eq!(
                match_rig_model(product, maker),
                None,
                "{maker} {product} must not resolve to an SDR-program profile"
            );
        }
        // The corpus still does its job for the radios that DO name themselves on USB.
        assert_eq!(
            match_rig_model("IC-705", "Icom Inc.").map(|(m, _)| m),
            Some(3085)
        );
        assert_eq!(
            match_rig_model("FTDX10", "Yaesu").map(|(m, _)| m),
            Some(1042)
        );
    }

    /// USB identities that are **not** any radio in the verified corpus, so no catalog name
    /// may ever claim one. Three kinds, and all three have bitten:
    ///
    /// 1. **Cables and interfaces** this crate itself recognises as not-a-radio
    ///    ([`match_interface`]) — a Digirig Lite came back as a Thetis.
    /// 2. **SDR hardware and bare bridge chips** — an SDRplay RSPdx came back as SDR Console.
    /// 3. **Real transceivers that live OUTSIDE the corpus** (extended tier). Here a wrong
    ///    model is worse than none: `detect_rigs` writes `suggested_model` into the Rig Model
    ///    field, and `port_prober::candidates_from` takes a name match as its "exact, trusted"
    ///    candidate and DROPS the operator's configured model. An IC-9100 that names itself
    ///    correctly on USB resolved to `(3044, "Icom IC-910")` — a different radio — because
    ///    the token `IC910` was a substring of `IC9100`.
    const NOT_A_CORPUS_RADIO: &[(&str, &str)] = &[
        // (1) cables
        ("Digirig Lite", "Digirig"),
        ("Digirig Mobile", "Digirig"),
        ("RIGblaster Advantage", "West Mountain Radio"),
        // (2) SDR hardware, SDR programs' own names, bare bridge chips
        ("SDRplay RSPdx", "SDRplay Ltd"),
        ("RS-HFIQ SDR", ""),
        ("ANAN-7000DLE", "Apache Labs"),
        ("Hermes Lite 2", "Hermes"),
        ("HPSDR interface", ""),
        ("FLEX control cable", ""),
        ("SDR Console", ""),
        ("CP2102 USB to UART Bridge Controller", "Silicon Labs"),
        ("USB-Serial Controller", "Prolific"),
        // (3) real rigs, extended tier — the corpus must say "I don't know", never a neighbour
        ("IC-9100", "Icom Inc."),
        ("TX-500", "Lab599"),
        ("Discovery TX-500", "Lab599"),
    ];

    /// Every catalog name in `catalog` that claims one of [`NOT_A_CORPUS_RADIO`], as
    /// `"<maker> <product> → (model, name)"`. Empty is the property holding.
    fn corpus_false_positives(catalog: &[(u32, &'static str)]) -> Vec<String> {
        NOT_A_CORPUS_RADIO
            .iter()
            .filter_map(|(product, maker)| {
                match_rig_model_in(product, maker, catalog)
                    .map(|(m, n)| format!("{maker:?} {product:?} → ({m}, {n:?})"))
            })
            .collect()
    }

    /// ⭐ THE PROPERTY, not a list of product strings. D1 and D2 each asserted the specific
    /// names that had already gone wrong; neither could see the NEXT one. Nothing in the types
    /// couples [`crate::rigmodels::is_software_cat_profile`] to the catalog it filters, and the
    /// commit that introduced D2 says how the bug arrives: **a tier move**. Promote one entry
    /// out of `extended_rig_models`, or add a fifth SDR-program profile and forget the
    /// exclusion, and `LITE`/`SDR`/`CONSOLE` re-enter the corpus with the suite still green.
    ///
    /// So this runs the real matcher over the real catalog AND over a poisoned one, and
    /// requires the poisoned run to FAIL. What it pins is: no verified-tier name may claim a
    /// USB identity that is not that radio.
    #[test]
    fn no_verified_tier_name_claims_a_device_that_is_not_that_radio() {
        assert_eq!(
            corpus_false_positives(&rig_models()),
            Vec::<String>::new(),
            "the shipped catalog"
        );

        // TEETH — both shapes the D2 commit names, each as a synthetic catalog entry that
        // `is_software_cat_profile` has never heard of. Verified by hand too: pasting either
        // line into `rig_models()` reds the assertion above.
        for (offender, victim, expect) in [
            // (a) A TIER MOVE. 2040 was promoted out of `extended_rig_models` in the very
            //     commit that broke this; here 2049 makes the same move.
            (
                (2049, "Malachite DSP SDR"),
                ("SDRplay RSPdx", "SDRplay Ltd"),
                "the bare token SDR walks straight back into the corpus",
            ),
            // (b) A FIFTH SDR-PROGRAM PROFILE added without the exclusion.
            (
                (2058, "SparkSDR (Hermes Lite 2 / ANAN)"),
                ("Hermes Lite 2", "Hermes"),
                "a program's hardware list is not a claim about USB devices",
            ),
        ] {
            let mut poisoned = rig_models();
            poisoned.push(offender);
            assert!(
                !corpus_false_positives(&poisoned).is_empty(),
                "a guard that cannot red on {offender:?} is not a guard"
            );
            assert_eq!(
                match_rig_model_in(victim.0, victim.1, &poisoned).map(|(m, _)| m),
                Some(offender.0),
                "{expect}"
            );
        }
    }

    /// The other half of the same property, and the reason the fix could not simply be
    /// "match less": every corpus entry that contributes a token must still resolve its OWN
    /// catalog name. Read this beside the guard above — one says "claim nothing you are not",
    /// the other says "and still claim yourself".
    ///
    /// (Entries whose every token is shorter than 3 characters — Elecraft K3, K4 — contribute
    /// nothing to the corpus by design, so they are skipped rather than asserted.)
    #[test]
    fn every_corpus_entry_still_matches_its_own_name() {
        for (model, name) in rig_models() {
            if model <= 4 || crate::rigmodels::is_software_cat_profile(model) {
                continue;
            }
            if model_tokens(name).is_empty() {
                continue;
            }
            assert_eq!(
                match_rig_model(name, "").map(|(m, _)| m),
                Some(model),
                "{name} ({model}) must still match its own name"
            );
        }
    }

    /// ⭐ THE GUARD THAT NEEDS NO HAND-MAINTAINED LIST, and the one that would have caught the
    /// IC-9100 on the day the entry was added: **run every EXTENDED-tier name through the
    /// corpus.** Those radios are real and are not in the corpus, so the only honest answers
    /// are "that model" or "I don't know" — never a NEIGHBOUR. `port_prober::candidates_from`
    /// treats a name match as exact and trusted, so a neighbour here is a rig swept at another
    /// rig's bauds with the operator's own model discarded.
    ///
    /// It is derived entirely from the catalog, so it grows with the catalog: add an extended
    /// entry whose name a corpus token claims, and this reds without anyone remembering to
    /// list a product string.
    ///
    /// **One exception, and it is not a matching bug.** `Yaesu FT-1000MP Mark-V` (1004)
    /// CONTAINS the whole name of `Yaesu FT-1000MP` (1024) as its own leading words — no
    /// boundary rule can separate them, only whole-string coverage scoring, which is a
    /// redesign this does not need: the Mark-V's CAT is an RS-232 port, so that string does
    /// not arrive here from a USB descriptor. Listed, not hidden — a future coverage-scoring
    /// matcher retires the entry rather than inheriting a silent exception.
    #[test]
    fn no_extended_tier_rig_resolves_to_a_different_rig() {
        const CONTAINED_BY_NAME: &[u32] = &[1004]; // FT-1000MP Mark-V ⊃ FT-1000MP
        let verified: std::collections::HashSet<u32> =
            rig_models().iter().map(|(m, _)| *m).collect();
        let mut wrong: Vec<String> = Vec::new();
        for (model, name) in crate::rigmodels::all_rig_models() {
            if verified.contains(&model) || CONTAINED_BY_NAME.contains(&model) {
                continue;
            }
            if let Some((m, n)) = match_rig_model(name, "") {
                if m != model {
                    wrong.push(format!("{name:?} ({model}) → ({m}, {n:?})"));
                }
            }
        }
        assert_eq!(wrong, Vec::<String>::new());
    }

    /// The second consumer, and the more damaging one: a name match here REPLACES the
    /// operator's configured `fallback_model` with a "trusted" one. An IC-7300 wired through
    /// a Digirig Lite must still be probed as an IC-7300.
    #[test]
    fn a_digirig_lite_does_not_hijack_the_configured_model_in_the_port_sweep() {
        let ports = [port("COM7", 0x10C4, "Digirig Lite", "Digirig")];
        let cands = crate::port_prober::candidates_from(&ports, 3073, &[]);
        assert_eq!(cands.len(), 1, "one port, one candidate: {cands:?}");
        assert_eq!(cands[0].model, 3073, "the operator's own model: {cands:?}");
        assert!(
            !cands[0].seeded,
            "still the trusted-model branch: {cands:?}"
        );
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

    /// THE two-codec collision (setup re-envisioning, 2026-08-09): a dual-Yaesu station
    /// enumerates two identical "USB Audio CODEC" devices, and per-row pairing broke the
    /// tie by list order BOTH times — radio 2 transmitted into radio 1's sound card,
    /// silently. Generic codecs are handed out 1:1 across the roster: worst case is now a
    /// SWAP (a visible, binary question), never a share (an invisible collision).
    #[test]
    fn two_identical_rigs_get_two_different_generic_codecs() {
        let ports = vec![
            port("COM5", 0x0403, "FT-991A", "Yaesu"),
            port("COM7", 0x0403, "FT-991A", "Yaesu"),
        ];
        let audio = vec![wdev("USB AUDIO CODEC"), wdev("USB AUDIO CODEC (2)")];
        let rigs = detect_rigs(&ports, &audio, &audio, HostOs::Windows);
        assert_eq!(rigs.len(), 2);
        let (a, b) = (&rigs[0].suggested_audio, &rigs[1].suggested_audio);
        assert!(a.is_some() && b.is_some(), "both rigs pair SOMETHING");
        assert_ne!(a, b, "two rigs must never share one codec: {a:?} vs {b:?}");
        // The output direction is assigned independently but under the same 1:1 rule.
        assert_ne!(rigs[0].suggested_audio_out, rigs[1].suggested_audio_out);
    }

    /// A dual-UART Icom is TWO rows describing ONE radio (civ_side tags the pair) — the
    /// pair consumes ONE codec, leaving the second codec for the second physical rig.
    #[test]
    fn a_dual_uart_pair_consumes_one_codec_not_two() {
        let ports = vec![
            port("COM3", 0x10c4, "IC-9700 Serial Port A", "Icom"),
            port("COM4", 0x10c4, "IC-9700 Serial Port B", "Icom"),
            port("COM5", 0x0403, "FT-991A", "Yaesu"),
        ];
        let audio = vec![wdev("USB AUDIO CODEC"), wdev("USB AUDIO CODEC (2)")];
        let rigs = detect_rigs(&ports, &audio, &audio, HostOs::Windows);
        assert_eq!(rigs.len(), 3);
        assert!(
            rigs[0].civ_side.is_some() && rigs[1].civ_side.is_some(),
            "harness: the Icom rows are recognised as a dual-UART pair"
        );
        // Both Icom rows point at the SAME codec (they are one radio; the operator keeps
        // one row), and the Yaesu gets the OTHER one.
        assert_eq!(rigs[0].suggested_audio, rigs[1].suggested_audio);
        assert!(rigs[2].suggested_audio.is_some());
        assert_ne!(rigs[0].suggested_audio, rigs[2].suggested_audio);
    }

    /// More rigs than codecs: the surplus rig pairs NOTHING — pairing nothing is honest,
    /// pairing a device another rig owns is the bug this pass exists to kill.
    #[test]
    fn a_rig_beyond_the_codec_supply_pairs_nothing() {
        let ports = vec![
            port("COM5", 0x0403, "FT-991A", "Yaesu"),
            port("COM7", 0x0403, "FT-891", "Yaesu"),
        ];
        let audio = vec![wdev("USB AUDIO CODEC")];
        let rigs = detect_rigs(&ports, &audio, &audio, HostOs::Windows);
        assert_eq!(rigs[0].suggested_audio.as_deref(), Some("USB AUDIO CODEC"));
        assert_eq!(
            rigs[1].suggested_audio, None,
            "no second codec exists — say so instead of sharing"
        );
    }

    /// A product-named device is OWNED by its rig: the generic pool must not hand it out
    /// again, and the product-matched rig consumes nothing from the pool.
    #[test]
    fn a_product_matched_codec_is_removed_from_the_generic_pool() {
        let ports = vec![
            port("COM3", 0x10c4, "IC-705", "Icom"),
            port("COM5", 0x0403, "FT-991A", "Yaesu"),
        ];
        let audio = vec![wdev("IC-705 Audio"), wdev("USB AUDIO CODEC")];
        let rigs = detect_rigs(&ports, &audio, &audio, HostOs::Windows);
        assert_eq!(rigs[0].suggested_audio.as_deref(), Some("IC-705 Audio"));
        assert_eq!(rigs[1].suggested_audio.as_deref(), Some("USB AUDIO CODEC"));
    }
}
