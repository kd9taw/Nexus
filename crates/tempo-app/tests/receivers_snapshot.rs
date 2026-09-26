//! THE DUAL-RECEIVER SNAPSHOT CONTRACT — `radio.receivers`, as the screens read it.
//!
//! The engine models Main and Sub (`Engine::receivers`); this is that model crossing the wire,
//! ADDITIVELY: every flat single-receiver field stays exactly where it was, and the receivers
//! ride beside them until the consumers move (the flat fields go only after the Remote page is
//! deployed, which is the operator's call).
//!
//! What is pinned here, through the snapshot JSON the UI actually parses — never through the
//! Rust struct, which could be right while its serialized names are wrong:
//!
//! - **Main always; the Sub only where this build OFFERS one** (D2, D10: a confirmed
//!   independent second receiver), with the three-state capability beside it (D3) so a missing
//!   Sub always says WHY — and UNKNOWN is never ABSENT.
//! - **Per receiver, not per radio** (D7): the stages each receiver may be credited with.
//! - **One truth**: Main's values are the flat fields read again, key by key, on a state where
//!   no two of them hold the same value.
//! - **The Sub carries only what the engine knows** — its dial while an acknowledged split
//!   rides the Sub band, and nothing Main reported.

use serde_json::Value;
use tempo_app::engine::{Engine, SatCatBackend};
use tempo_app::settings::SatVfoMap;
use tempo_core::doppler::{DownlinkClass, Transponder};

/// A station on Hamlib `model`, tuned to 20 m phone.
fn station(model: u32) -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    let mut s = e.settings().clone();
    s.rig_model = model;
    e.apply_settings(s);
    assert_eq!(
        e.settings().rig_model,
        model,
        "precondition: the station runs model {model}"
    );
    e.set_frequency(14.250, "20m", "USB");
    e
}

fn radio(e: &Engine) -> Value {
    serde_json::to_value(e.snapshot()).expect("snapshot serializes")["radio"].clone()
}

fn receivers(e: &Engine) -> Value {
    let r = radio(e)["receivers"].clone();
    assert!(
        r.is_object(),
        "radio.receivers must be an object on every engine snapshot, got {r}"
    );
    r
}

fn near(v: &Value, want: f64) -> bool {
    v.as_f64().is_some_and(|x| (x - want).abs() < 1e-6)
}

#[test]
fn a_confirmed_dual_receiver_radio_carries_main_and_a_sub() {
    // IC-7610: two independent receivers, offered. D7 in one radio: the Sub owns its front end
    // and its AF (Icom: "independent AF/RF knobs"), and no vendor statement gives it a DSP.
    let rs = receivers(&station(3078));
    assert_eq!(rs["subCapability"], "present", "{rs}");
    assert_eq!(rs["main"]["id"], "main");
    assert_eq!(rs["sub"]["id"], "sub", "IC-7610 is offered a Sub: {rs}");
    assert_eq!(
        rs["main"]["stages"],
        serde_json::json!({ "frontEnd": "own", "dsp": "own", "audio": "own" })
    );
    assert_eq!(
        rs["sub"]["stages"],
        serde_json::json!({ "frontEnd": "own", "dsp": "unknown", "audio": "own" })
    );
    // IC-9700: the Sub's front end is its own, but neither DSP nor AF is documented per
    // receiver — the same radio class, a different per-receiver answer (D7).
    let rs = receivers(&station(3081));
    assert_eq!(
        rs["sub"]["stages"],
        serde_json::json!({ "frontEnd": "own", "dsp": "unknown", "audio": "unknown" })
    );
}

#[test]
fn a_radio_offered_no_sub_says_which_of_the_three_reasons_it_is() {
    // UNKNOWN (no manual read), ABSENT (a vendor-sourced single receiver), and PRESENT without
    // an offer (a shared front end, category 2) — three different answers, compared BY VALUE:
    // "no Sub" alone would pass on an implementation that collapsed them.
    let unread = receivers(&station(3073)); // IC-7300
    let single = receivers(&station(1037)); // FTDX3000
    let shared = receivers(&station(3063)); // IC-7600
    let no_cat = receivers(&station(0));
    for (who, rs) in [
        ("IC-7300", &unread),
        ("FTDX3000", &single),
        ("IC-7600", &shared),
        ("no CAT", &no_cat),
    ] {
        assert!(rs["sub"].is_null(), "{who}: no Sub is offered: {rs}");
        assert_eq!(rs["main"]["id"], "main", "{who}: Main is always there");
        assert!(rs["pairing"].is_null(), "{who}: nothing to pair: {rs}");
    }
    assert_eq!(unread["subCapability"], "unknown");
    assert_eq!(single["subCapability"], "absent");
    assert_eq!(shared["subCapability"], "present");
    assert_eq!(
        no_cat["subCapability"], "unknown",
        "no CAT is not a no either"
    );
    assert_ne!(
        unread["subCapability"], single["subCapability"],
        "UNKNOWN must never collapse into ABSENT"
    );
}

#[test]
fn main_is_the_flat_snapshot_read_again() {
    let mut e = station(3078);
    // Every reading given a value no other field holds, so a crossed wire cannot pass.
    e.observe_rig_mode("LSB".into());
    e.observe_rig_smeter(-7);
    e.observe_rig_agc("slow".into());
    e.observe_rig_passband(Some(2_700));
    e.observe_rig_funcs([
        Some(true),
        Some(false),
        Some(true),
        None,
        None,
        Some(false),
        None,
    ]);
    e.observe_rig_nr_level(0.22);
    e.observe_rig_notch_freq_hz(1_234.0);
    e.observe_rig_rf_gain(0.44);
    e.observe_rig_squelch(0.55);
    e.observe_rig_af_gain(0.77);
    e.observe_rig_att_db(12);
    e.observe_rig_preamp_db(2);
    e.observe_rig_db_steps(Some(vec![6, 12, 18]), Some(vec![1, 2]));
    e.observe_rig_rx_ranges(Some(vec![(30_000, 60_000_000)]));
    e.request_rit(150);
    e.request_vfo(true);
    let r = radio(&e);
    let main = &r["receivers"]["main"];
    for key in [
        "dialMhz",
        "band",
        "sideband",
        "sidebandOverride",
        "rigMode",
        "ritHz",
        "refusedDialMhz",
        "smeterDb",
        "agc",
        "refusedAgc",
        "filterWidthHz",
        "nb",
        "nr",
        "nrLevel",
        "notch",
        "manualNotch",
        "notchFreqHz",
        "rfGain",
        "squelch",
        "attDb",
        "preampDb",
        "attStepsDb",
        "preampStepsDb",
        "afGain",
        "activeVfo",
        "rxRangesMhz",
        "scopeModeCode",
        "scopeFixStartMhz",
        "scopeSpanRefused",
        "scopeError",
    ] {
        assert!(
            main.get(key).is_some(),
            "receivers.main has no `{key}` — the name must match the flat field: {main}"
        );
        assert_eq!(
            main[key], r[key],
            "receivers.main.{key} differs from radio.{key}"
        );
    }
    // The discriminator for the loop above: the state really is non-default, so equality is
    // not two nulls agreeing.
    for (key, want) in [
        ("smeterDb", serde_json::json!(-7)),
        ("attDb", serde_json::json!(12)),
        ("activeVfo", serde_json::json!("B")),
        ("ritHz", serde_json::json!(150)),
        ("rigMode", serde_json::json!("LSB")),
    ] {
        assert_eq!(main[key], want, "receivers.main.{key}");
    }
    assert!(near(&main["afGain"], 0.77), "{}", main["afGain"]);
}

/// An IC-9700 working RS-44 through the native CI-V daemon, built through the verbs the product
/// uses: the pick, then the radio loop's acknowledgement naming the receiver it wrote.
fn rs44_pass() -> Engine {
    let rs44 = Transponder {
        uplink_centre_hz: 145_965_000,
        downlink_centre_hz: 435_640_000,
        invert: true,
        half_width_hz: 30_000,
    };
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    let mut s = e.settings().clone();
    s.rig_model = 3081;
    s.rig_conn = "serial".into();
    s.rig_addr = String::new();
    s.icom_native_cat = true;
    e.apply_settings(s);
    e.confirm_sat_uplink(None, Some(SatVfoMap::MainDownSubUp));
    e.set_license_class("general");
    e.set_operating_mode("phone", false);
    e.set_sat_transponder(Some(("RS-44|linear".into(), 0, rs44)));
    e.sat_tune_nominal(DownlinkClass::Usb, 1_000_000);
    let up = e
        .split_tx_mhz()
        .map(|m| (m * 1e6).round() as u64)
        .expect("the pick put the uplink on the split");
    e.rig_dial_applied(e.settings().dial_hz());
    let vfo = e
        .sat_split_tx_vfo(up, SatCatBackend::NativeCiv)
        .expect("the native daemon carries Main/Sub");
    assert_eq!(vfo, "Sub", "precondition: the uplink rides the Sub band");
    e.rig_split_applied_on(up, tempo_app::dualrx::ReceiverId::Sub);
    e
}

#[test]
fn the_sub_carries_only_what_the_engine_knows() {
    let mut e = rs44_pass();
    e.observe_rig_smeter(-3);
    let rs = receivers(&e);
    let (main, sub) = (&rs["main"], &rs["sub"]);
    assert!(
        near(&main["dialMhz"], 435.640),
        "Main is the downlink: {main}"
    );
    assert!(
        near(&sub["dialMhz"], 145.965),
        "the Sub is the uplink: {sub}"
    );
    assert_eq!(sub["band"], "2m");
    assert_eq!(
        sub["sideband"], "LSB",
        "an inverting bird: USB down, LSB up"
    );
    assert_eq!(main["smeterDb"], -3);
    // Main's reading never lands on the Sub — and "unknown" is null, never a zero.
    for key in [
        "smeterDb",
        "afGain",
        "rfGain",
        "squelch",
        "ritHz",
        "activeVfo",
    ] {
        assert!(sub[key].is_null(), "sub.{key} must be unknown: {sub}");
    }
    // D6: 430 and 144 are different groups on an IC-9700, so the pair it sits in is legal.
    assert_eq!(rs["pairing"]["state"], "allowed", "{rs}");
}
