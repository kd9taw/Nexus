//! PSK Reporter **MQTT firehose** semantics — the live "who hears me / who I
//! hear" upgrade to the rate-limited XML query ([`crate::live::pskreporter`]).
//!
//! Pure (no network): the subscription topic filters + a parser that turns a
//! `pskr/filter/v2/...` topic into a [`PathSpot`] the advisor already consumes.
//! The MQTT transport itself is `tempo_net::mqtt`. The topic carries band / mode
//! / calls / locators, so we score paths from the topic alone (SNR + exact
//! frequency live only in the JSON payload, which the advisor doesn't need).
//!
//! Topic layout (verified against the official mqtt.pskreporter.info `v2` feed —
//! 11 segments; trailing fields are ADIF **DXCC numbers**, NOT regions; the
//! **frequency is NOT in the topic** — it's payload-only):
//! `pskr/filter/v2/{band}/{mode}/{txCall}/{rxCall}/{txGrid}/{rxGrid}/{txDxcc}/{rxDxcc}`

use crate::model::{Band, PathSpot};

/// MQTT topic filters for the operator's own paths: "who hears me" (we're the
/// sender) and "who I hear" (we're the receiver). `#` matches the trailing topic
/// levels so it's robust to PSK Reporter schema tweaks.
pub fn mqtt_topics(mycall: &str) -> Vec<String> {
    let c = mycall.trim().to_ascii_uppercase();
    vec![
        format!("pskr/filter/v2/+/+/{c}/#"), // sender == me  → who heard me
        format!("pskr/filter/v2/+/+/+/{c}/#"), // receiver == me → who I hear
    ]
}

fn non_empty(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// Parse a PSK Reporter MQTT topic into a [`PathSpot`] stamped `now` (Unix secs).
/// `None` if it isn't a `pskr/filter/v2` reception report on a band we model.
/// (SNR + exact frequency are payload-only, so SNR is left `None`; the band comes
/// from the topic's band-label segment and the advisor treats SNR as optional.)
pub fn parse_mqtt_report(topic: &str, now: i64) -> Option<PathSpot> {
    let p: Vec<&str> = topic.split('/').collect();
    if p.len() < 11 || p[0] != "pskr" || p[1] != "filter" || p[2] != "v2" {
        return None;
    }
    let band = Band::from_label(p[3])?; // band label, e.g. "20m"
    let mode = p[4];
    let sender = p[5];
    let receiver = p[6];
    // A real published topic carries concrete calls, never the +/# wildcards.
    if sender.is_empty()
        || receiver.is_empty()
        || sender.contains(['+', '#'])
        || receiver.contains(['+', '#'])
    {
        return None;
    }
    Some(PathSpot {
        time: now,
        tx_call: sender.to_ascii_uppercase(),
        tx_grid: non_empty(p[7]),
        rx_call: receiver.to_ascii_uppercase(),
        rx_grid: non_empty(p[8]),
        band,
        mode: non_empty(mode),
        snr: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topics_cover_both_directions() {
        let t = mqtt_topics("kd9taw");
        assert_eq!(t[0], "pskr/filter/v2/+/+/KD9TAW/#"); // who heard me (sender slot)
        assert_eq!(t[1], "pskr/filter/v2/+/+/+/KD9TAW/#"); // who I hear (receiver slot)
    }

    #[test]
    fn parses_a_reception_report_topic() {
        // Real v2 layout: …/{band}/{mode}/{tx}/{rx}/{txGrid}/{rxGrid}/{txDxcc}/{rxDxcc}
        // (trailing fields are ADIF DXCC numbers; no frequency segment).
        let topic = "pskr/filter/v2/20m/FT8/W1AW/JA1XYZ/FN31/PM95/291/339";
        let s = parse_mqtt_report(topic, 1_700_000_000).unwrap();
        assert_eq!(s.tx_call, "W1AW");
        assert_eq!(s.rx_call, "JA1XYZ");
        assert_eq!(s.tx_grid.as_deref(), Some("FN31"));
        assert_eq!(s.rx_grid.as_deref(), Some("PM95"));
        assert_eq!(s.band, Band::B20); // from the "20m" label segment
        assert_eq!(s.mode.as_deref(), Some("FT8"));
        assert_eq!(s.time, 1_700_000_000);
    }

    #[test]
    fn rejects_non_pskr_or_malformed_topics() {
        assert!(parse_mqtt_report("foo/bar/baz", 0).is_none());
        assert!(parse_mqtt_report("pskr/filter/v2/20m/FT8/W1AW", 0).is_none()); // too short
                                                                                // unknown band label → not a band we model.
        assert!(
            parse_mqtt_report("pskr/filter/v2/zz/FT8/W1AW/JA1XYZ/FN31/PM95/291/339", 0).is_none()
        );
    }

    #[test]
    fn empty_locator_becomes_none() {
        let topic = "pskr/filter/v2/40m/CW/DL1ABC/W1AW///230/291";
        let s = parse_mqtt_report(topic, 0).unwrap();
        assert_eq!(s.tx_call, "DL1ABC");
        assert!(s.tx_grid.is_none());
        assert!(s.rx_grid.is_none());
        assert_eq!(s.band, Band::B40);
    }
}
