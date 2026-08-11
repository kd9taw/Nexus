//! Hamlib rig model numbers for the rig-control UI, split into two tiers.
//!
//! [`rig_models`] is the **verified** tier: `(model, name)` pairs we've
//! sanity-checked and that cover the radios most likely to be used for Field
//! Day / digital work. It's what the default Settings dropdown shows.
//!
//! [`all_rig_models`] is the **full** catalog: the verified tier plus a much
//! broader set of common amateur transceivers so an operator whose exact rig
//! isn't in the short list can still find it (surface this behind a "show all
//! models" toggle). Rigctld actually speaks 250+ models; this covers the
//! common transmitting amateur rigs across the major makers. For anything
//! still not listed, the operator can type the raw Hamlib model number — the
//! definitive list for their installed Hamlib is `rigctl -l`.
//!
//! Every number here is anchored on Hamlib's `include/hamlib/riglist.h`
//! (`model = 1000 * backend + index`, e.g. Dummy=1, NET=2, FLRig=4,
//! IC-7300=3073). Model indices are append-only and never renumbered across
//! Hamlib releases, so a number verified in one 4.x release holds in the next.

/// `(hamlib_model_number, friendly_name)` for the curated **verified** set of
/// common rigs. This is the default UI list.
///
/// Ordered roughly Dummy/NET first, then by manufacturer. Not exhaustive —
/// for the long tail use [`all_rig_models`] or type the model number directly;
/// `rigctl -l` is the definitive list for the operator's Hamlib version.
pub fn rig_models() -> Vec<(u32, &'static str)> {
    // Numbers verified against Hamlib 4.7.1 `include/hamlib/riglist.h`:
    // model = 1000 * backend + index (RIG_MAKE_MODEL), anchored on
    // Dummy=1, NET=2, FLRig=4, IC-7300=3073. Curated to common amateur rigs;
    // `rigctl -l` is the definitive list for the operator's Hamlib version.
    vec![
        // Hamlib built-ins
        (1, "Hamlib Dummy"),
        (2, "NET rigctl (remote rigctld)"),
        (4, "FLRig (flrig)"),
        // Icom
        (3073, "Icom IC-7300"),
        (3085, "Icom IC-705"),
        (3078, "Icom IC-7610"),
        (3081, "Icom IC-9700"),
        // IC-7760 (split control-head/RF-deck flagship). Verified index 92 in the bundled
        // Hamlib 4.7.1 riglist.h → model 3092. Driven via Hamlib rigctld, NOT Nexus's native
        // CI-V/scope path (that stays limited to the hardware-verified 7300-family): the 7760
        // is typically LAN-connected, which uses Hamlib anyway, and its scope stream is
        // unverified. Adding it here is what makes CAT work — before, the 7760 was absent from
        // every table, so it fell through to a wrong/zero model and CAT was dead.
        (3092, "Icom IC-7760"),
        // IC-7300MKII — `RIG_MODEL_IC7300MK2 = RIG_MAKE_MODEL(RIG_ICOM, 94)` in the bundled
        // 4.7.0 riglist.h → 3094 (`rigctl -l` names it `IC-7300MK2`, Beta). Driven via rigctld,
        // NOT the native CI-V/scope path: `icom_scope_model` stays limited to the radios whose
        // `0x27` stream we have actually seen, and this one has not been on a bench here.
        (3094, "Icom IC-7300MKII"),
        (3070, "Icom IC-7100"),
        (3013, "Icom IC-718"),
        // The IC-725/726/728/729 family. Added 2026-08-06 from a field report: an operator
        // with an IC-728 could not find it in either tier, picked "Icom IC-705 (3085)" as the
        // nearest-looking Icom, and Nexus drove his 1990s rig with the IC-705's CI-V address
        // and command set — every read timed out, forever, while WSJT-X (which has the model)
        // worked on the same cable. Selecting 3085 also exposed and armed Native Icom CI-V,
        // a 115200-baud scope path, on a radio running at 1200. A missing catalog row is not a
        // cosmetic gap: it makes an operator choose a wrong one.
        (3014, "Icom IC-725"),
        (3015, "Icom IC-726"),
        (3016, "Icom IC-728"),
        (3017, "Icom IC-729"),
        (3060, "Icom IC-7000"),
        (3023, "Icom IC-746"),
        (3046, "Icom IC-746PRO"),
        (3057, "Icom IC-756PROIII"),
        (3044, "Icom IC-910"),
        (3090, "Icom IC-905"),
        // Yaesu
        (1035, "Yaesu FT-991 / FT-991A"),
        (1049, "Yaesu FT-710"),
        // FTX-1 — `RIG_MODEL_FTX1 = RIG_MAKE_MODEL(RIG_YAESU, 51)` → 1051 (`rigctl -l`: `FTX-1`,
        // Beta; its own backend, dated 20251224.0, not the shared newcat one). Yaesu's current
        // portable: shipping now, and absent from every table here until this entry.
        (1051, "Yaesu FTX-1"),
        (1042, "Yaesu FTDX10"),
        (1040, "Yaesu FTDX101D"),
        (1044, "Yaesu FTDX101MP"),
        (1036, "Yaesu FT-891"),
        (1022, "Yaesu FT-857 / FT-857D"),
        (1043, "Yaesu FT-897D"),
        (1020, "Yaesu FT-817 / FT-817ND"),
        (1041, "Yaesu FT-818 / FT-818ND"),
        (1046, "Yaesu FT-450D"),
        (1028, "Yaesu FT-950"),
        (1029, "Yaesu FT-2000"),
        (1034, "Yaesu FTDX1200"),
        (1037, "Yaesu FTDX3000"),
        (1032, "Yaesu FTDX5000"),
        (1024, "Yaesu FT-1000MP"),
        // Kenwood
        (2031, "Kenwood TS-590S"),
        (2037, "Kenwood TS-590SG"),
        (2041, "Kenwood TS-890S"),
        (2039, "Kenwood TS-990S"),
        (2028, "Kenwood TS-480 (SAT/HX)"),
        (2014, "Kenwood TS-2000"),
        (2010, "Kenwood TS-870S"),
        (2009, "Kenwood TS-850"),
        // Elecraft (Kenwood-family backend)
        (2029, "Elecraft K3"),
        (2043, "Elecraft K3S"),
        (2047, "Elecraft K4"),
        (2044, "Elecraft KX2"),
        (2045, "Elecraft KX3"),
        // FlexRadio. 2036 is the WSJT-X-proven path: it speaks the Flex dialect
        // of Kenwood CAT served by the SmartSDR CAT app's TCP/serial ports on
        // the PC. SmartSDR CAT's DEFAULT TCP port 5002 is directed at slice A;
        // its per-slice extras are B=60001, C=60002, D=60003 (60000 base).
        // 23005 talks the radio's native API directly and is alpha-grade in
        // Hamlib (failed on a real 6400M with WSAEADDRNOTAVAIL) — keep it
        // selectable, but nothing auto-picks it anymore.
        (2036, "FlexRadio FLEX-6xxx (SmartSDR CAT)"),
        (23005, "FlexRadio SmartSDR native (experimental)"),
        // SDR console SOFTWARE — here the PROGRAM is the rig. A Hermes Lite 2, an ANAN, a
        // legacy Flex has no CAT port of its own: CAT is served by the program running on
        // the PC, over TCP or a virtual COM pair. So these are named for what the operator
        // LAUNCHED, not for the board on the desk. Field report 2026-08: an HL2 owner on
        // Thetis searched this list for his hardware, found nothing, and settled on the
        // FLEX-6xxx profile — which connects, then drives Thetis with the FLEX-6000 command
        // set (2036 has no RIG_LEVEL_STRENGTH at all, so the S-meter goes dead).
        // Numbers verified in the bundled Hamlib 4.7.1 riglist.h:
        //   RIG_MODEL_THETIS     = RIG_MAKE_MODEL(RIG_KENWOOD, 54) → 2054
        //   RIG_MODEL_POWERSDR   = RIG_MAKE_MODEL(RIG_KENWOOD, 48) → 2048
        //   RIG_MODEL_HPSDR      = RIG_MAKE_MODEL(RIG_KENWOOD, 40) → 2040 (OpenHPSDR, PiHPSDR)
        //   RIG_MODEL_SDRCONSOLE = RIG_MAKE_MODEL(RIG_KENWOOD, 56) → 2056
        // 2054/2048 share `rigs/kenwood/flex6xxx.c`'s powersdr caps: STRENGTH, SWR and
        // RFPOWER_METER present, and `flex6k_set_ptt` keys with `ZZTX1;ZZTX` (key AND read
        // back). NOT a TS-2000 emulation — the old 2048 label said so and was wrong; the
        // dialect is PowerSDR's own ZZ-prefixed set (`ZZMD%02d` for mode).
        (2054, "Thetis (Hermes Lite 2 / ANAN / HPSDR)"),
        (2048, "PowerSDR / mRX PS (Apache ANAN / legacy FLEX)"),
        (2040, "piHPSDR / OpenHPSDR (Hermes Lite 2 / ANAN)"),
        (2056, "SDR Console (SDR-Radio.com)"),
        // Ten-Tec
        (16013, "Ten-Tec Eagle (599)"),
        (16008, "Ten-Tec Orion (565)"),
        (16011, "Ten-Tec Omni VII (588)"),
        // Xiegu (Icom-family backend)
        (3088, "Xiegu G90"),
        (3087, "Xiegu X6100"),
        (3091, "Xiegu X6200"),
        (3089, "Xiegu X5105"),
        // QRP / kit rigs (Kenwood-family backend — they all speak Kenwood-style CAT).
        // Numbers off the bundled 4.7.0 riglist.h: QRPLABS=RIG_MAKE_MODEL(RIG_KENWOOD,52),
        // FX4=…53, TRUSDX=…55, QRPLABS_QMX=…57.
        (2057, "QRP Labs QMX"),
        (2052, "QRP Labs QCX / QDX"),
        // BG2FX's FX-4 family — one Hamlib backend for all four variants (`rigctl -l` prints
        // `FX4/C/CR/L`), so the variants are spelled here rather than given numbers they do
        // not have.
        (2053, "BG2FX FX-4 (C/CR/L)"),
        // The DL2MAN/PE1NNZ kit. Named exactly as the project spells it, because that is what
        // its owner types into the box.
        (2055, "(tr)uSDX"),
        // Alinco
        (17002, "Alinco DX-SR8"),
    ]
}

/// The additional (beyond-verified) common amateur transceivers that fill out
/// the full catalog. Kept private; callers want [`all_rig_models`] (verified +
/// these) or [`rig_models`] (verified only).
///
/// Every number is anchored on Hamlib's `riglist.h` (`model = 1000 * backend +
/// index`). These are drawn from the authoritative header — no guessed numbers.
fn extended_rig_models() -> Vec<(u32, &'static str)> {
    vec![
        // Hamlib built-in bridges (backend 0)
        (5, "TRX-Manager (rig control)"),
        // ⚠️ NO model 7 ("TCI (SunSDR / ExpertSDR)"). The number is real in `riglist.h`, but the
        // Hamlib we SHIP cannot load it — measured on the delivery path, 2026-08-06:
        //   ./rigctld.exe -m 7 -r 127.0.0.1:50001 -t 4599
        //     → "Unknown rig num 7, or initialization error."   (model 1 starts fine)
        //   ./rigctl.exe -l | grep -i "tci\|sunsdr\|expert"     → nothing
        // Offering it meant an operator who ticked "Show all models" could pick a rig whose
        // daemon then refuses to start. **A `strings libhamlib-4.dll | grep tci1x` hit (29 of
        // them) does NOT establish the backend ships** — that check passed twice while this was
        // broken. Source strings can be in the DLL with the backend unregistered in the build;
        // the only check that settles it is starting rigctld on the model. TCI itself is a
        // different protocol from Hamlib CAT — SunSDR/ExpertSDR owners use the CAT server port
        // (13013), which is where the network-port hint now points them.
        // Icom (backend 3)
        (3011, "Icom IC-706MKIIG"),
        (3010, "Icom IC-706MKII"),
        (3009, "Icom IC-706"),
        (3055, "Icom IC-703"),
        (3061, "Icom IC-7200"),
        (3067, "Icom IC-7410"),
        (3062, "Icom IC-7700"),
        (3063, "Icom IC-7600"),
        (3056, "Icom IC-7800"),
        (3068, "Icom IC-9100"),
        (3026, "Icom IC-756"),
        (3027, "Icom IC-756PRO"),
        (3047, "Icom IC-756PROII"),
        (3019, "Icom IC-735"),
        // Yaesu (backend 1)
        (1001, "Yaesu FT-847"),
        (1010, "Yaesu FT-736R"),
        (1021, "Yaesu FT-100 / FT-100D"),
        (1023, "Yaesu FT-897"),
        (1027, "Yaesu FT-450"),
        (1014, "Yaesu FT-920"),
        // Verified against the BUNDLED rigctl.exe -l (Stable backend): the vintage-rig
        // field ask — an FT-890 owner could already type 1015, now the picker names it.
        (1015, "Yaesu FT-890"),
        (1030, "Yaesu FTDX9000"),
        (1004, "Yaesu FT-1000MP Mark-V"),
        (1016, "Yaesu FT-990"),
        // Kenwood (backend 2)
        (2001, "Kenwood TS-50S"),
        (2002, "Kenwood TS-440S"),
        (2003, "Kenwood TS-450S"),
        (2004, "Kenwood TS-570D"),
        (2016, "Kenwood TS-570S"),
        (2005, "Kenwood TS-690S"),
        (2007, "Kenwood TS-790"),
        (2011, "Kenwood TS-940S"),
        (2013, "Kenwood TS-950SDX"),
        (2025, "Kenwood TS-140S"),
        (2034, "Kenwood TM-D710"),
        (2035, "Kenwood TM-V71"),
        // Kenwood-family backend: Elecraft K2 + SDRs that speak Kenwood CAT
        (2021, "Elecraft K2"),
        // 2040 (OpenHPSDR / PiHPSDR) was here; it moved UP into the verified tier with the
        // other SDR-program profiles — behind the Show-all toggle it was unfindable by the
        // Hermes Lite 2 owner who needs it. The tiers must stay disjoint.
        (2049, "Malachite DSP SDR"),
        (2050, "Lab599 Discovery TX-500"),
        (2051, "SDRuno (SDRplay)"),
        // Ten-Tec (backend 16)
        (16001, "Ten-Tec TT-550 Pegasus"),
        (16002, "Ten-Tec TT-538 Jupiter"),
        (16007, "Ten-Tec TT-516 Argonaut V"),
        (16009, "Ten-Tec TT-585 Paragon"),
        // Xiegu (Icom-family backend)
        (3076, "Xiegu X108G"),
        // Alinco (backend 17)
        (17001, "Alinco DX-77"),
    ]
}

/// The full rig catalog: the [`rig_models`] verified set first (in curated
/// order), then [`extended_rig_models`] grouped by manufacturer. Use this for
/// the "show all models" view; the two tiers never share a model number, so a
/// caller can badge verified entries by membership in [`rig_models`].
pub fn all_rig_models() -> Vec<(u32, &'static str)> {
    rig_models()
        .into_iter()
        .chain(extended_rig_models())
        .collect()
}

/// Friendly name for a Hamlib model number, if it's anywhere in the full
/// catalog (verified or extended). Returns `None` for an unknown number, so a
/// typed-in model that Hamlib supports but we don't name still shows as blank
/// rather than mislabeled.
pub fn rig_model_name(model: u32) -> Option<&'static str> {
    all_rig_models()
        .into_iter()
        .find(|(m, _)| *m == model)
        .map(|(_, name)| name)
}

/// Catalog entries where **the program is the rig**: CAT is served by an application on a PC
/// (or by the radio's own Ethernet API), over TCP or a virtual COM pair. None of them is ever
/// a USB device that enumerates with a descriptor.
///
/// ⚠️ **THIS IS WHAT KEEPS A NAME OUT OF THE USB AUTO-DETECT CORPUS.** A catalog name is not
/// only dropdown text: [`crate::usbrig::match_rig_model`] tokenises every VERIFIED-tier name
/// and substring-matches the tokens against `manufacturer + product`. Adding the SDR-program
/// profiles therefore made `LITE`, `SDR`, `HPSDR`, `ANAN`, `APACHE`, `HERMES` and `FLEX` live
/// claims about USB hardware, and a **Digirig Lite** — a cable [`crate::usbrig::match_interface`]
/// recognises by that exact string — started resolving to 2054 "Thetis", an SDRplay RSPdx to
/// 2056 "SDR Console". Both consumers act on it: `detect_rigs` writes `suggested_model` into
/// the Rig Model field, and `port_prober::candidates_from` takes a name match as its "exact,
/// trusted" candidate and drops the operator's configured model.
///
/// The fix is membership, not naming — the names are the whole point of the profiles, and an
/// HL2 owner has to be able to read his hardware in one.
///
/// - `2036` / `23005` — FlexRadio 6000: Ethernet radio, CAT served by the SmartSDR CAT app.
/// - `2054` / `2048` / `2040` / `2056` — Thetis / PowerSDR / piHPSDR / SDR Console.
/// - `5` / `2051` — TRX-Manager, SDRuno. These sit in `extended_rig_models`, which
///   `match_rig_model` does not read today. They are listed anyway because a TIER MOVE is
///   exactly how this bug arrived: 2040 was promoted out of the extended tier in the same
///   commit, and `5`'s tokens include the bare words `RIG` and `CONTROL`.
/// - `7` (TCI) is kept in the set below although it is no longer OFFERED anywhere — the
///   bundled Hamlib cannot load it, so `extended_rig_models` dropped it (see there). Membership
///   here only ever withholds a name match, so keeping it costs nothing and covers a settings
///   file written before it was delisted.
///
/// (Hamlib's Dummy/NET/FLRig — models ≤ 4 — are excluded by `match_rig_model` separately;
/// they are not radios at all rather than software-served ones.)
pub(crate) fn is_software_cat_profile(model: u32) -> bool {
    matches!(
        model,
        5 | 7 | 2036 | 2040 | 2048 | 2051 | 2054 | 2056 | 23005
    )
}

/// Recognise a CAT server that **names itself** in the greeting it sends on connect, and
/// return `(program name, the catalog model written for it)`.
///
/// Thetis's TCP/IP CAT server greets every client with
/// `#Thetis TCP/IP Cat - <window title>#;` — that greeting is what
/// [`crate::rigctld_server::probe_cat_port`] hands us, and it is the only evidence here.
///
/// **What this may and may not conclude.** The program naming itself is a fact we can quote;
/// everything after it is not. The tail of a Thetis banner is a compile-time build label
/// (`TitleBar.BUILD_NAME`) — the field report's `HL2 Beta 2 (MI0BOT)` is a *fork's* label,
/// true for that operator and a lie on any other build of the same fork. So the match is on
/// the program token alone, and **no hardware is ever inferred from a banner**. The greeting
/// is also operator-switchable in Thetis ("Send version on client connect"), so `None` here
/// means "did not name itself", never "not Thetis".
pub fn program_from_banner(banner: &str) -> Option<(&'static str, u32)> {
    let b = banner.to_ascii_lowercase();
    // Thetis first: a Thetis fork's banner can carry PowerSDR lineage words, and Thetis is
    // the more specific claim. Both map to `flex6xxx.c` caps with a real S-meter.
    if b.contains("thetis") {
        Some(("Thetis", 2054))
    } else if b.contains("powersdr") {
        Some(("PowerSDR", 2048))
    } else {
        None
    }
}

/// The kind of NATIVE spectrum stream a radio can provide — the shared capability gate for
/// the per-radio panadapter (Wave 7 Flex + Wave 8 Icom converge here). `None` (from
/// [`native_spectrum_kind`]) means the universal audio-FFT scope is the only option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpectrumKind {
    /// Icom CI-V spectrum scope (command `0x27`) — requires Nexus to own the serial CI-V
    /// port natively. Carries the rig's default CI-V bus address.
    IcomCiv { civ_addr: u8 },
    /// FlexRadio SmartSDR panadapter over VITA-49 UDP (the `sub pan` / `display pan` stream).
    FlexVita,
}

/// Map a curated Hamlib model number to the Icom rig whose native CI-V scope Nexus supports.
/// Only the 7300-family radios that actually expose the `0x27` scope stream are listed.
pub(crate) fn icom_scope_model(model: u32) -> Option<crate::civ::commands::IcomModel> {
    use crate::civ::commands::IcomModel::*;
    Some(match model {
        3073 => Ic7300,
        3078 => Ic7610,
        3081 => Ic9700,
        3085 => Ic705,
        3090 => Ic905,
        _ => return None,
    })
}

/// Hamlib **serial** rigs whose CAT backend answers slowly enough that the tight 700 ms
/// serial read deadline can fire before rigctld/Hamlib finishes its own
/// `post_write_delay + timeout × retry` (plus the internal retry-on-timeout) — producing a
/// spurious "rig reply incomplete after 700 ms (got \"\")" even though the rig would have
/// answered. These get the longer (2.5 s) `slow_transport` window that network chains and
/// the native CI-V daemon already use — matching how WSJT-X lets Hamlib's own timeout+retry
/// finish rather than racing an external stopwatch.
///
/// ⚠️ **This set stands on its own and mirrors nothing.** It used to be documented as mirroring
/// `BAUD_BY_MODEL` in SettingsPanel.tsx — a name two renames dead (`RIG_CAT_RATES`, then
/// `RIG_FIXED_BAUD`) — and after the 2026-08-06 deletions the two do not correspond at all: the
/// Xiegus, the vintage Kenwoods and the TS-870S/570D/570S below every one lost their baud row,
/// because those rows were transcribed from manuals and that is what made them wrong. **The
/// function is unaffected: it is about the 700 ms READ DEADLINE, not about baud.** A rig belongs
/// here because its backend answers slowly, which is why it survived a change that was entirely
/// about rates. Do not re-couple the two — that shared premise is what made one wrong table
/// propagate into a second place.
///
/// The slow set, verified against Hamlib riglist.h:
/// - **Xiegu** CI-V family (G90/X6100/X6200/X5105/X108G) — fixed 19200, slow CI-V backend.
/// - **Vintage Kenwood** (IF-232C era, fixed 4800 8N2: TS-50S/140S/440S/450S/690S/790/850/
///   940S/950SDX) and the 9600 TS-870S / TS-570D/S.
///
/// Every modern/fast rig is UNAFFECTED and keeps the 700 ms deadline: Yaesu, Icom via
/// rigctld, modern Kenwood (TS-590/890/990), Flex, Elecraft.
#[cfg_attr(not(feature = "device"), allow(dead_code))] // caller lives in service.rs (device)
pub(crate) fn is_slow_serial_rig(model: u32) -> bool {
    matches!(
        model,
        // Xiegu CI-V (fixed 19200, slow backend)
        3088 | 3087 | 3091 | 3089 | 3076
        // Vintage Kenwood — fixed 4800
        | 2001 | 2002 | 2003 | 2005 | 2007 | 2009 | 2011 | 2013 | 2025
        // 1990s Kenwood — 9600 factory default
        | 2004 | 2010 | 2016
        // PowerSDR (2048) / Thetis (2054) over a VIRTUAL COM pair. Not a slow wire — a slow
        // BACKEND: Hamlib's caps for both declare `.timeout = 800, .retry = 10`
        // (`rigs/kenwood/flex6xxx.c`), so one transaction can legitimately run ~8 s inside
        // Hamlib while the 700 ms deadline out here fires at 0.7 s and reports "reply
        // incomplete" for a program that was going to answer. THE COM0COM ROUTE IS THE ONLY
        // ONE THAT NEEDS THIS — and it is [`is_slow_serial_link`], which is handed the app's
        // one network answer, that holds that line. (It did not, and could not, when this
        // entry was added:
        // the classification was connection-blind, and `service.rs`'s FAST S-METER POLL is
        // gated on it with no `is_network()` in front — so an ANAN on TCP silently dropped to
        // half the S-meter rate.)
        | 2048 | 2054
    )
}

/// The raw CAT string that reads a rig's TRUE current mode straight off the wire, for the
/// models whose CAT is ASCII text and whose query we can name. `None` = **send nothing raw**,
/// which is the answer for every other radio on earth and the default this function exists to
/// restore.
///
/// **Why it is gated at all.** `service::mode_set_note` pushed `MD0;` — a *Yaesu* CAT string —
/// through rigctld's `w` (send_cmd) on **every mode change, to every rig, ungated by make**,
/// and it cost two different radios two different things:
/// - **Icom.** CI-V is a binary bus; `MD0;` is five bytes of ASCII junk on it, and nothing
///   answers. `w` then blocks until our read deadline expires, and `rig::command_with_deadline`
///   drops the rigctld connection on any failure — which is the daemon's disconnect fail-safe
///   unkey, i.e. the trigger the PTT-flicker work was chasing. It also burned the one
///   diagnostic that answers "modes won't switch" for every Icom operator.
/// - **Old Yaesu.** The FT-847/817/857/897/1000MP family does NOT speak this ASCII set; their
///   CAT is fixed 5-byte binary packets. Four stray bytes there do not merely fail — they
///   desynchronise the framing, so the NEXT genuine command is re-read starting from the wrong
///   byte. That is a made-up command sent to a transmitter, and it is why "Yaesu" is not the
///   gate: **the make is not the protocol.**
///
/// **What the gate actually is**, and it is checkable rather than remembered: Hamlib's shared
/// `newcat.c` backend is the modern Yaesu ASCII CAT set, and every rig built on it reports the
/// same backend date. From the bundled 4.7.0 (`rigctl -m <model> --dump-caps`), the whole
/// `20241118.x` family is listed below and everything else is out. Two Yaesus are deliberately
/// excluded despite being modern: nothing is lost by excluding one (the note falls back to
/// `read_mode`), and a wrong include writes bytes to a radio.
/// - **FTX-1 (1051)** — its own backend, `20251224.0`, not newcat's date. Very likely the same
///   command set; "very likely" is not the standard for bytes on a wire.
/// - Anything not in the list at all, including every future model — absent means silent, which
///   is the safe direction.
#[cfg_attr(not(feature = "device"), allow(dead_code))] // caller lives in service.rs (device)
pub(crate) fn raw_mode_query(model: u32) -> Option<&'static str> {
    // Yaesu `newcat.c` (backend version 20241118.x in the bundled Hamlib 4.7.0): FT-450,
    // FT-950, FT-2000, FTDX-9000, FTDX-5000, FTDX-1200, FT-991/991A, FT-891, FTDX-3000,
    // FTDX-101D, FTDX-10, FTDX-101MP, FT-450D, FT-710.
    matches!(
        model,
        1027 | 1028
            | 1029
            | 1030
            | 1032
            | 1034
            | 1035
            | 1036
            | 1037
            | 1040
            | 1042
            | 1044
            | 1046
            | 1049
    )
    // `MD0;` = read the mode of VFO-A. The reply is the rig's own code (`MD02;` = USB,
    // `MD0C;` = DATA-U), which is the ground truth `m` cannot give — Hamlib's `m` can return
    // the mode it BELIEVES it set, from cache, on a rig that never moved.
    .then_some("MD0;")
}

/// A **serial** CAT link that needs the LONG (2.5 s) command deadline: a known slow-backend
/// rig ([`is_slow_serial_rig`]) — or ANY rig on a slow configured baud (≤ 19200). At
/// 19200 a multi-frame Hamlib transaction (the Icom DATA-mode set: mode + data-mode
/// frames, each echoed on the CI-V bus, plus per-frame rig processing and Hamlib's own
/// read-back/retries) can legitimately outlast the 700 ms fast deadline — which then
/// reports "rig reply incomplete", and the bounded mode retry misread that as a rig
/// without the mode (the IC-7610 @ 19200 "rig has no PKTUSB mode" report; 19200 is that
/// rig's CI-V default that the "Auto" USB baud tracks). Baud 0 = "backend default" is
/// NOT slow — every affected slow-default model is already in the model list.
///
/// `is_network` is load-bearing, not decoration: **a network link is never a slow serial
/// link**, and neither input the rest of this function reads means anything over TCP — the
/// model may be an SDR program that is only slow through a com0com pair, and `baud` is a
/// stale serial field the network path never uses. Answering that question connection-blind
/// is how the fast S-meter poll (`service.rs`) halved its rate for a TCP Thetis/ANAN: three
/// of the four callers wrote `t.is_network() || is_slow_serial_link(…)` and the fourth had
/// no `t` to ask.
///
/// ⚠️ **IT IS A BOOLEAN, AND IT COMES FROM THE ONE PLACE THAT ANSWERS THAT QUESTION.** This
/// briefly took a `rig_conn: &str` and tested it here
/// (`eq_ignore_ascii_case("network")`) — a second implementation of
/// [`tempo_app::settings::rig_conn_is_network`], whose own doc is headed "THE SINGLE SOURCE
/// OF TRUTH for that question, for the whole app" and lists the three ways a previous copy
/// parted company from it. The string test re-created two: it called a "network" pick with
/// **no address yet** a network link (the SoT requires an address, because rigctld would
/// have nowhere to go — so a Xiegu G90 whose owner had flipped Connection to Network without
/// typing one lost its long deadline while rigctld was still dialling its serial port at
/// 19200), and it accepted mixed case the SoT does not. The only caller is
/// [`crate::service::Transport::is_slow_serial_link`], which feeds this `self.is_network()`
/// — i.e. the SoT — so there is no string left to parse and nothing left to remember.
#[cfg_attr(not(feature = "device"), allow(dead_code))] // caller lives in service.rs (device)
pub(crate) fn is_slow_serial_link(model: u32, baud: u32, is_network: bool) -> bool {
    if is_network {
        return false;
    }
    is_slow_serial_rig(model) || (1..=19_200).contains(&baud)
}

/// Classify what native spectrum stream a radio offers, given its Hamlib model number and
/// its connection kind (`"serial"` / `"network"`). This is the single gate both native
/// panadapter workers consult:
/// - **Flex** (SmartSDR CAT 2036 / native 23005) over a **network** connection → `FlexVita`.
/// - **Icom 7300/7610/9700/705/905** over a **serial** connection → `IcomCiv` (the scope
///   needs the native CI-V serial owner; over network rigctld it isn't reachable).
/// - Everything else (Xiegu, other Icoms, Yaesu, Kenwood, a network Icom) → `None`
///   (audio-FFT fallback).
pub fn native_spectrum_kind(model: u32, rig_conn: &str) -> Option<SpectrumKind> {
    let is_network = rig_conn.eq_ignore_ascii_case("network");
    match model {
        2036 | 23005 if is_network => Some(SpectrumKind::FlexVita),
        _ => {
            if !is_network {
                let m = icom_scope_model(model)?;
                Some(SpectrumKind::IcomCiv {
                    civ_addr: m.default_civ_addr(),
                })
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// THE GROUND TRUTH the satellite refusal is worded from, pinned across the
    /// crate boundary.
    ///
    /// `tempo_app::settings::MAIN_SUB_SAT_RIGS` lists the four Icoms whose satellite
    /// mode fixes TX on Sub. Only some of them can reach Nexus's OWN CI-V daemon,
    /// because `native_civ_addr` gates on [`icom_scope_model`] — a table built for
    /// the `0x27` spectrum scope, not for satellites. `NATIVE_CIV_SAT_RIGS` is
    /// tempo-app's copy of that intersection, and `Engine::sat_split_tx_vfo` decides
    /// from it whether "turn on Native CI-V" is a cure that EXISTS for the rig in
    /// front of the operator. If the two tables drift, the refusal starts naming a
    /// switch that cannot help (IC-910/IC-9100) or withholding one that can.
    ///
    /// tempo-audio depends on tempo-app, so the check can only live on this side.
    ///
    /// Iterated over the UNION of the two tables, not over `MAIN_SUB_SAT_RIGS`
    /// alone (round 3, defect 4): membership-agreement over one table cannot
    /// see a model added to `NATIVE_CIV_SAT_RIGS` that is NOT a Main/Sub rig.
    /// Adding the IC-7300 (3073) there would have passed the old check while
    /// making `sat_native_civ_reachable` true for a radio with no Sub band —
    /// so `main_sub_refusal` would tell that operator to switch on Native CI-V
    /// for a mapping their rig cannot express. `NATIVE_CIV_SAT_RIGS` is an
    /// INTERSECTION: every member must be in both parents.
    #[test]
    fn the_native_capable_sat_rig_table_is_the_civ_scope_table() {
        use tempo_app::settings::{MAIN_SUB_SAT_RIGS, NATIVE_CIV_SAT_RIGS};
        let mut union: Vec<u32> = MAIN_SUB_SAT_RIGS.to_vec();
        union.extend(NATIVE_CIV_SAT_RIGS.iter().copied());
        union.sort_unstable();
        union.dedup();
        for m in union {
            assert_eq!(
                NATIVE_CIV_SAT_RIGS.contains(&m),
                MAIN_SUB_SAT_RIGS.contains(&m) && icom_scope_model(m).is_some(),
                "model {m}: NATIVE_CIV_SAT_RIGS is MAIN_SUB_SAT_RIGS ∩ icom_scope_model, \
                 and this model is on the wrong side of that"
            );
        }
        // Spelled out, so a future edit to either table has to face the names:
        // the IC-9700 and IC-905 have a native path; the IC-910 and IC-9100 have
        // none and never will in this build.
        assert!(icom_scope_model(3081).is_some(), "IC-9700");
        assert!(icom_scope_model(3090).is_some(), "IC-905");
        assert!(icom_scope_model(3044).is_none(), "IC-910 — no CI-V scope");
        assert!(icom_scope_model(3068).is_none(), "IC-9100 — no CI-V scope");
    }

    #[test]
    fn slow_serial_rig_flags_xiegu_and_vintage_kenwood_only() {
        // Xiegu family (slow CI-V backend) → the long deadline.
        for m in [3088u32, 3087, 3091, 3089, 3076] {
            assert!(is_slow_serial_rig(m), "Xiegu model {m} should be slow");
        }
        // Vintage Kenwood (4800/9600 IF-232C-era rigs) → slow. Deliberately NOT cross-checked
        // against any baud table: this is the read-deadline set, and the rate rows those rigs
        // used to carry were deleted for being transcribed guesses (see `is_slow_serial_rig`).
        for m in [
            2001u32, 2002, 2003, 2005, 2007, 2009, 2011, 2013, 2025, 2004, 2010, 2016,
        ] {
            assert!(
                is_slow_serial_rig(m),
                "vintage Kenwood model {m} should be slow"
            );
        }
        // The slow-BACKEND SDR programs on a virtual COM pair (Hamlib caps: timeout 800 ×
        // retry 10) — the com0com half of the Thetis field report.
        for m in [2048u32, 2054] {
            assert!(
                is_slow_serial_rig(m),
                "SDR-program model {m} should be slow"
            );
        }
        // Everything else keeps the fast 700 ms deadline — the working rigs are UNAFFECTED:
        // Icom 7300/9700, a Yaesu serial model (1042), MODERN Kenwood TS-590S (2031),
        // Flex SmartSDR CAT (2036) and native (23005), Elecraft K4 (2047), and "none" (0).
        for m in [3073u32, 3081, 1042, 2031, 2036, 23005, 2047, 0] {
            assert!(
                !is_slow_serial_rig(m),
                "fast/modern model {m} must stay fast"
            );
        }
    }

    /// The connection kind is a BOOLEAN here, and it is the caller's job to source it from
    /// `tempo_app::settings::rig_conn_is_network` (via `Transport::is_slow_serial_link`).
    /// Spelled out so these cases read as "serial"/"network" rather than bare `false`/`true`.
    const SERIAL: bool = false;
    const NETWORK: bool = true;

    #[test]
    fn slow_serial_link_adds_low_baud_on_any_model() {
        // The model-based slow set stays slow at ANY configured baud.
        assert!(
            is_slow_serial_link(3088, 115_200, SERIAL),
            "Xiegu is slow per model"
        );
        // ANY rig at ≤ 19200 baud is a slow link — the IC-7610 case: 19200 is that
        // rig's CI-V default (tracked by the "Auto" USB baud), and Hamlib's multi-frame
        // Icom DATA-mode set outlasts the 700 ms fast deadline there, which the mode
        // retry then misread as "rig has no PKTUSB mode".
        assert!(is_slow_serial_link(3078, 19_200, SERIAL), "IC-7610 @ 19200");
        assert!(
            is_slow_serial_link(1042, 9_600, SERIAL),
            "even a Yaesu on a slow line"
        );
        // Fast rigs on fast serial keep the short deadline — unaffected.
        assert!(
            !is_slow_serial_link(3078, 115_200, SERIAL),
            "IC-7610 @ 115200 is fast"
        );
        assert!(!is_slow_serial_link(1042, 38_400, SERIAL));
        // Baud 0 = "backend default" — not slow (the slow-default models are already
        // in the model list above).
        assert!(!is_slow_serial_link(3078, 0, SERIAL));
    }

    /// A SERIAL question may not be answered about a NETWORK link. The slow-backend entries
    /// for PowerSDR/Thetis were added for the com0com route, and their own comment said TCP
    /// was already covered — but `service.rs`'s FAST S-METER POLL is gated on
    /// `is_slow_serial_link` with no `is_network()` in front of it, so adding 2048/2054 halved
    /// the S-meter rate for an ANAN or a Hermes Lite 2 reached over TCP, which is how most of
    /// them are reached. Same for the `baud <= 19200` arm: `baud` is a serial field the
    /// network path never uses, so a stale 9600 in it must not slow a TCP link either.
    ///
    /// **Which link kind a transport IS, is not decided here** — it arrives as a boolean off
    /// `tempo_app::settings::rig_conn_is_network`. That guard is
    /// `service::tests::the_slow_cat_deadline_reads_the_network_answer_off_the_single_source_of_truth`,
    /// where a `Transport` exists to ask; this one only pins what each answer means.
    #[test]
    fn a_network_cat_link_is_never_a_slow_serial_link() {
        // The route the entries were added FOR: a virtual COM pair. Still slow.
        assert!(
            is_slow_serial_link(2054, 38_400, SERIAL),
            "Thetis over com0com: Hamlib caps are timeout 800 × retry 10"
        );
        assert!(is_slow_serial_link(2048, 38_400, SERIAL), "PowerSDR too");
        // The route most of them actually take. Not a serial link at all.
        assert!(
            !is_slow_serial_link(2054, 38_400, NETWORK),
            "a TCP Thetis keeps the fast S-meter cadence"
        );
        assert!(!is_slow_serial_link(2048, 38_400, NETWORK));
        // …including on a stale low baud left over from a serial config.
        assert!(!is_slow_serial_link(3078, 9_600, NETWORK));
    }

    #[test]
    fn native_spectrum_capability_gate() {
        // Flex over network → VITA panadapter.
        assert_eq!(
            native_spectrum_kind(2036, "network"),
            Some(SpectrumKind::FlexVita)
        );
        assert_eq!(
            native_spectrum_kind(23005, "network"),
            Some(SpectrumKind::FlexVita)
        );
        // IC-9700 on serial → CI-V scope at the 9700's default address 0xA2.
        assert_eq!(
            native_spectrum_kind(3081, "serial"),
            Some(SpectrumKind::IcomCiv { civ_addr: 0xA2 })
        );
        assert_eq!(
            native_spectrum_kind(3073, "serial"),
            Some(SpectrumKind::IcomCiv { civ_addr: 0x94 })
        );
        // A network Icom can't use the native serial CI-V scope → audio-FFT fallback.
        assert_eq!(native_spectrum_kind(3081, "network"), None);
        // Yaesu FTDX10 (no native spectrum stream) and an unlisted Icom → None.
        assert_eq!(native_spectrum_kind(1042, "serial"), None);
        assert_eq!(native_spectrum_kind(3013, "serial"), None); // IC-718: no scope
    }

    #[test]
    fn table_is_non_empty_and_has_builtins() {
        let models = rig_models();
        assert!(!models.is_empty());
        // Hamlib Dummy and NET rigctl are always present.
        assert!(models.iter().any(|(m, _)| *m == 1));
        assert!(models.iter().any(|(m, _)| *m == 2));
        // A representative real rig.
        assert!(models.iter().any(|(m, _)| *m == 3073));
    }

    #[test]
    fn name_lookup_resolves_known_and_unknown() {
        assert_eq!(rig_model_name(3073), Some("Icom IC-7300"));
        assert_eq!(rig_model_name(1), Some("Hamlib Dummy"));
        // An out-of-catalog model number has no name.
        assert_eq!(rig_model_name(999_999), None);
    }

    #[test]
    fn model_numbers_are_unique() {
        let models = rig_models();
        let mut seen = HashSet::new();
        for (m, _) in models {
            assert!(seen.insert(m), "duplicate model number {m}");
        }
    }

    #[test]
    fn full_catalog_supersets_verified_without_collisions() {
        let verified = rig_models();
        let all = all_rig_models();
        // The full catalog is strictly larger (verified + extended entries).
        assert!(all.len() > verified.len());
        assert_eq!(all.len(), verified.len() + extended_rig_models().len());

        // Every verified entry survives verbatim into the full catalog.
        let all_set: HashSet<(u32, &str)> = all.iter().copied().collect();
        for entry in &verified {
            assert!(
                all_set.contains(entry),
                "verified entry {entry:?} missing from full catalog"
            );
        }

        // No model number is duplicated across the whole catalog. This is the
        // load-bearing guard: an accidental collision between the verified and
        // extended tiers would silently mislabel a rig the operator selects.
        let mut seen = HashSet::new();
        for (m, _) in &all {
            assert!(
                seen.insert(*m),
                "duplicate model number {m} in full catalog"
            );
        }
    }

    #[test]
    fn verified_and_extended_tiers_are_disjoint() {
        let verified: HashSet<u32> = rig_models().into_iter().map(|(m, _)| m).collect();
        for (m, name) in extended_rig_models() {
            assert!(
                !verified.contains(&m),
                "extended model {m} ({name}) collides with the verified tier"
            );
        }
    }

    #[test]
    fn extended_only_model_resolves_via_name_lookup() {
        // 3011 (IC-706MKIIG) is only in the extended tier, not the verified one.
        assert!(rig_models().iter().all(|(m, _)| *m != 3011));
        assert_eq!(rig_model_name(3011), Some("Icom IC-706MKIIG"));
        // A representative extended entry from another backend.
        assert_eq!(rig_model_name(2050), Some("Lab599 Discovery TX-500"));
    }

    /// THE FIELD REPORT (2026-08). A Hermes Lite 2 owner running Thetis could not make CAT
    /// work in Nexus while it worked in WSJT-X. The catalog named no program an SDR operator
    /// launches — only hardware — so the hunt for "Hermes Lite 2" found nothing and ended on
    /// the FLEX-6xxx profile, which does connect and then drives Thetis with the FLEX-6000
    /// command set (see `only_a_real_flexradio_gets_the_vita_panadapter` and the message in
    /// `service.rs`). These entries are the route that was missing.
    ///
    /// They are in the **verified** tier deliberately: "Show all models" defaults OFF, so an
    /// entry that lives only in `extended_rig_models` is invisible to exactly the operator
    /// who has this problem.
    #[test]
    fn the_sdr_program_profiles_are_in_the_default_list() {
        let verified: HashSet<u32> = rig_models().into_iter().map(|(m, _)| m).collect();
        for (m, who) in [
            (2054u32, "Thetis"),
            (2048, "PowerSDR"),
            (2040, "piHPSDR / OpenHPSDR"),
            (2056, "SDR Console"),
        ] {
            assert!(
                verified.contains(&m),
                "{who} ({m}) must be in the DEFAULT dropdown, not behind Show-all"
            );
        }
        let name = |m| rig_model_name(m).unwrap_or("");
        // Named for the PROGRAM the operator launched, and carrying the hardware words an
        // HL2 owner actually types into the box.
        assert!(name(2054).starts_with("Thetis"), "got {:?}", name(2054));
        assert!(name(2054).contains("Hermes Lite 2"), "got {:?}", name(2054));
        assert!(name(2040).contains("Hermes Lite 2"), "got {:?}", name(2040));
        // 2048 is PowerSDR — a program, not a FlexRadio product line. The old
        // "FlexRadio PowerSDR" label is what sent an ANAN/HL2 operator to the Flex profiles.
        assert!(name(2048).starts_with("PowerSDR"), "got {:?}", name(2048));
        // And it does not emulate a TS-2000: Hamlib's powersdr/thetis backends send the
        // ZZ-prefixed extended set (`ZZMD%02d`, `ZZTX1;ZZTX`), not TS-2000 CAT.
        assert!(!name(2048).contains("TS-2000"), "got {:?}", name(2048));
    }

    /// THE QA PASS (2026-08) ranked "I can't find my radio in the list" the #1 problem in the
    /// whole rig-setup surface, and these five are the radios it named: current-production sets
    /// whose owners are buying them NOW and finding an empty search. Every number is read off
    /// the bundled Hamlib 4.7.0 `riglist.h` (`model = 1000 * backend + index`) — not guessed:
    /// `RIG_MODEL_IC7300MK2 = RIG_MAKE_MODEL(RIG_ICOM, 94)` → 3094,
    /// `RIG_MODEL_FTX1 = RIG_MAKE_MODEL(RIG_YAESU, 51)` → 1051,
    /// `RIG_MODEL_QRPLABS = RIG_MAKE_MODEL(RIG_KENWOOD, 52)` → 2052,
    /// `RIG_MODEL_FX4 = RIG_MAKE_MODEL(RIG_KENWOOD, 53)` → 2053,
    /// `RIG_MODEL_TRUSDX = RIG_MAKE_MODEL(RIG_KENWOOD, 55)` → 2055.
    /// Cross-checked against the bundled daemon itself (`rigctl -l`), which names them
    /// `IC-7300MK2`, `FTX-1`, `QCX/QDX`, `FX4/C/CR/L` and `(tr)uSDX`.
    ///
    /// **Verified tier, deliberately** — the same reason the SDR-program profiles are there:
    /// "Show all models" defaults OFF, so an entry that lives only in `extended_rig_models` is
    /// invisible to exactly the operator who cannot find his radio. That puts their names into
    /// the USB auto-detect corpus, which is the cost this buys; `no_verified_tier_name_claims_a_
    /// device_that_is_not_that_radio` and `no_extended_tier_rig_resolves_to_a_different_rig` are
    /// what say it was paid.
    #[test]
    fn the_current_production_rigs_the_qa_pass_named_are_in_the_default_list() {
        let verified: HashSet<u32> = rig_models().into_iter().map(|(m, _)| m).collect();
        for (m, who) in [
            (3094u32, "Icom IC-7300MKII"),
            (1051, "Yaesu FTX-1"),
            (2052, "QRP Labs QCX / QDX"),
            (2053, "BG2FX FX-4"),
            (2055, "(tr)uSDX"),
        ] {
            assert!(
                verified.contains(&m),
                "{who} ({m}) must be in the DEFAULT dropdown, not behind Show-all"
            );
        }
        // Named so an operator searching the box finds them by what is printed on the radio.
        let name = |m| rig_model_name(m).unwrap_or("");
        assert!(name(3094).contains("7300"), "got {:?}", name(3094));
        assert!(name(1051).contains("FTX-1"), "got {:?}", name(1051));
        assert!(name(2052).contains("QDX"), "got {:?}", name(2052));
        assert!(name(2052).contains("QCX"), "got {:?}", name(2052));
        assert!(name(2053).contains("FX-4"), "got {:?}", name(2053));
        assert!(name(2055).contains("uSDX"), "got {:?}", name(2055));
    }

    /// None of the SDR-program profiles may light the SmartSDR VITA-49 panadapter: that
    /// stream exists on a FlexRadio 6000 and nowhere else. Starting the worker against a
    /// Hermes Lite 2 is a UDP listener waiting on a radio that cannot answer.
    #[test]
    fn only_a_real_flexradio_gets_the_vita_panadapter() {
        for m in [2054u32, 2048, 2040, 2056] {
            assert_eq!(
                native_spectrum_kind(m, "network"),
                None,
                "model {m} is an SDR program, not a FlexRadio"
            );
        }
        // Unchanged — the ONE verified-good configuration keeps its stream.
        assert_eq!(
            native_spectrum_kind(2036, "network"),
            Some(SpectrumKind::FlexVita)
        );
        assert_eq!(
            native_spectrum_kind(23005, "network"),
            Some(SpectrumKind::FlexVita)
        );
    }

    /// A CAT server that greets us by name gets named back, and mapped to the profile
    /// written for it — never to a Flex profile.
    #[test]
    fn a_cat_server_greeting_names_the_profile_written_for_it() {
        // The exact bytes from the field report (Thetis 2.10.3.13 driving a Hermes Lite 2).
        let thetis = "#Thetis TCP/IP Cat - Thetis v2.10.3.13 x64 (04/01/26) HL2 Beta 2 (MI0BOT)#;";
        assert_eq!(program_from_banner(thetis), Some(("Thetis", 2054)));
        // Upstream's own build label, so the match cannot be keyed on the fork's tail.
        assert_eq!(
            program_from_banner("#Thetis TCP/IP Cat - Thetis v2.10.3.13 x64 (04/01/26) MW0LGE#;"),
            Some(("Thetis", 2054))
        );
        assert_eq!(
            program_from_banner("#PowerSDR TCP/IP Cat - PowerSDR v3.4.9#;"),
            Some(("PowerSDR", 2048))
        );
        // Everything else is unrecognised — we assert a program only when it names itself.
        assert_eq!(program_from_banner(""), None);
        assert_eq!(program_from_banner("CHKVFO 0"), None);
        assert_eq!(program_from_banner("FA00014074000;"), None);
        assert_eq!(program_from_banner("HTTP/1.1 400 Bad Request"), None);
    }

    #[test]
    fn every_catalog_number_lands_in_a_known_backend() {
        // Guards against a fat-fingered number that decodes to a nonexistent
        // Hamlib backend (backend = model / 1000). These are the backends we
        // actually draw from; anything else means a typo in a model number.
        let known_backends: HashSet<u32> = [0, 1, 2, 3, 16, 17, 23].into_iter().collect();
        for (m, name) in all_rig_models() {
            let backend = m / 1000;
            assert!(
                known_backends.contains(&backend),
                "model {m} ({name}) decodes to unexpected backend {backend}"
            );
        }
    }
}
