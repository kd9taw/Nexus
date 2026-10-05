//! The transmitter alarm the operator must see until they dismiss it (operator ruling 2026-10-04,
//! "Sticky until dismissed"). The radio loop raises one whenever Nexus's Flex client says what the
//! operator must know about the transmitter. The same words reach the CAT status, but that line is
//! latest-wins: the next CAT message replaced the alarm, and it could scroll off unseen. Here
//! nothing clears one but the operator: not a CAT status, not a reconnect, not a radio switch, not
//! a settings save. Nothing here gates anything either: the dial, Tune, PTT and the CAT verdict
//! are exactly as they were.
//!
//! A newer alarm QUEUES behind the ones on screen and never replaces one: replacing is how the CAT
//! line lost them. The same words about the same radio again are one alarm, not two, with the new
//! time and a new id, so a dismissal of the earlier one (sent before the operator could see this
//! one) leaves it on screen.

use super::Engine;
use crate::dto::TxAlarm;

impl Engine {
    /// Raise a transmitter alarm about radio `radio_id`: `text` in the radio loop's own words, at
    /// `at_ms` (unix ms). It names the radio as the station names it now.
    pub fn raise_tx_alarm(&mut self, radio_id: u32, text: String, at_ms: u64) {
        self.tx_alarm_seq += 1;
        let alarm = TxAlarm {
            id: self.tx_alarm_seq,
            radio_name: self
                .settings
                .radios
                .iter()
                .find(|p| p.id == radio_id)
                .map(|p| p.name.clone())
                .unwrap_or_default(),
            text,
            radio_id,
            at_ms,
        };
        match self
            .tx_alarms
            .iter_mut()
            .find(|a| a.radio_id == radio_id && a.text == alarm.text)
        {
            Some(shown) => *shown = alarm,
            None => self.tx_alarms.push(alarm),
        }
    }

    /// The operator dismissed alarm `id`: clear exactly it. An id no longer on screen (dismissed
    /// already, or the same alarm raised again since) clears nothing.
    pub fn dismiss_tx_alarm(&mut self, id: u64) {
        self.tx_alarms.retain(|a| a.id != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{RadioProfile, Settings};

    const UNCONFIRMED: &str =
        "the radio did not confirm the unkey — it may still be transmitting. Check the radio now.";
    const HELD: &str = "an earlier Nexus session (0x7A3C0001) still holds the transmitter; Nexus \
                        will not key until it is released";

    /// A station with two radios, the Flex active.
    fn two_radios() -> Engine {
        Engine::with_settings(Settings {
            radios: vec![
                RadioProfile {
                    id: 1,
                    name: "FLEX-6600".into(),
                    ..RadioProfile::default()
                },
                RadioProfile {
                    id: 2,
                    name: "IC-9700".into(),
                    ..RadioProfile::default()
                },
            ],
            active_radio: 1,
            ..Settings::default()
        })
    }

    /// The alarms on screen, in order: the snapshot as the screen receives it.
    fn shown(e: &Engine) -> Vec<serde_json::Value> {
        let snap = serde_json::to_value(e.snapshot()).unwrap();
        snap["txAlarms"].as_array().cloned().unwrap_or_default()
    }

    fn texts(e: &Engine) -> Vec<String> {
        shown(e)
            .iter()
            .map(|a| a["text"].as_str().unwrap().to_string())
            .collect()
    }

    fn ids(e: &Engine) -> Vec<u64> {
        shown(e).iter().map(|a| a["id"].as_u64().unwrap()).collect()
    }

    /// ⭐ A dismissal clears exactly the alarm it names: never another one, and an id no longer
    /// on screen clears nothing.
    #[test]
    fn a_dismissal_clears_exactly_that_alarm() {
        let mut e = two_radios();
        e.raise_tx_alarm(1, UNCONFIRMED.into(), 1_000);
        e.raise_tx_alarm(1, HELD.into(), 2_000);
        let [first, second] = ids(&e)[..] else {
            panic!("two alarms on screen: {:?}", shown(&e))
        };
        e.dismiss_tx_alarm(second);
        assert_eq!(texts(&e), [UNCONFIRMED], "the newer one, and only it");
        e.dismiss_tx_alarm(second);
        e.dismiss_tx_alarm(second + 100);
        assert_eq!(texts(&e), [UNCONFIRMED], "no id on screen clears nothing");
        e.dismiss_tx_alarm(first);
        assert!(shown(&e).is_empty(), "{:?}", shown(&e));
        let snap = serde_json::to_value(e.snapshot()).unwrap();
        assert!(
            snap.get("txAlarms").is_none(),
            "with none on screen the snapshot is the one it always was"
        );
    }

    /// ⭐ A newer alarm queues behind the one on screen; it never replaces it.
    #[test]
    fn a_newer_alarm_queues_behind_the_one_on_screen() {
        let mut e = two_radios();
        e.raise_tx_alarm(1, UNCONFIRMED.into(), 1_000);
        e.raise_tx_alarm(2, HELD.into(), 2_000);
        e.raise_tx_alarm(1, HELD.into(), 3_000);
        assert_eq!(texts(&e), [UNCONFIRMED, HELD, HELD]);
        let radios: Vec<_> = shown(&e).iter().map(|a| a["radioId"].clone()).collect();
        assert_eq!(radios, [1, 2, 1]);
    }

    /// The same words about the same radio again are one alarm, in its place, with the new time
    /// and a new id: a dismissal sent before the operator could see the repeat leaves it.
    #[test]
    fn the_same_alarm_again_is_one_alarm_that_a_stale_dismissal_leaves() {
        let mut e = two_radios();
        e.raise_tx_alarm(1, UNCONFIRMED.into(), 1_000);
        e.raise_tx_alarm(1, HELD.into(), 2_000);
        let stale = ids(&e)[0];
        e.raise_tx_alarm(1, UNCONFIRMED.into(), 5_000);
        assert_eq!(texts(&e), [UNCONFIRMED, HELD], "one alarm, in its place");
        assert_eq!(
            shown(&e)[0]["atMs"],
            5_000,
            "with the time it was raised again"
        );
        e.dismiss_tx_alarm(stale);
        assert_eq!(
            texts(&e),
            [UNCONFIRMED, HELD],
            "the stale dismissal leaves it"
        );
        // About ANOTHER radio, the same words are another alarm.
        e.raise_tx_alarm(2, UNCONFIRMED.into(), 6_000);
        assert_eq!(texts(&e), [UNCONFIRMED, HELD, UNCONFIRMED]);
    }

    /// Its text, its radio and when.
    #[test]
    fn an_alarm_carries_its_text_its_radio_and_when() {
        let mut e = two_radios();
        e.raise_tx_alarm(1, UNCONFIRMED.into(), 1_790_778_153_000);
        e.raise_tx_alarm(9, HELD.into(), 1_790_778_154_000);
        let [flex, unnamed] = &shown(&e)[..] else {
            panic!("two alarms on screen: {:?}", shown(&e))
        };
        assert_eq!(flex["text"], UNCONFIRMED);
        assert_eq!(flex["radioId"], 1);
        assert_eq!(flex["radioName"], "FLEX-6600");
        assert_eq!(flex["atMs"], 1_790_778_153_000_u64);
        assert_eq!(unnamed["radioId"], 9);
        assert_eq!(
            unnamed["radioName"], "",
            "a radio the station does not name"
        );
    }

    /// ⭐ Only the operator clears it: not a later CAT status (the bug), not the reconnect's
    /// teardown, not a radio switch, not a settings save, not a Stop.
    #[test]
    fn only_a_dismissal_clears_it() {
        let mut e = two_radios();
        e.raise_tx_alarm(1, UNCONFIRMED.into(), 1_000);
        e.set_cat_status(Some(true), "Connected — 14.074 MHz".into());
        e.set_cat_status(
            Some(false),
            "the CAT helper (rigctld) stopped — restarting it.".into(),
        );
        e.remote_close_radio();
        e.set_active_radio(2);
        assert_eq!(e.settings().active_radio, 2, "premise: the radio switched");
        let saved = e.settings().clone();
        e.apply_settings(saved);
        e.halt_tx();
        assert_eq!(texts(&e), [UNCONFIRMED]);
        assert_eq!(
            shown(&e)[0]["radioName"],
            "FLEX-6600",
            "still about the Flex"
        );
    }
}
