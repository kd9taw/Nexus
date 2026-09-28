//! What a page's offer must be before the station will answer it. Read from the SDP text, before
//! any WebRTC state exists, so a refused offer costs the station nothing but this read.
//!
//! ⛔ DTLS-SRTP OR NOTHING (security acceptance test A4). Every media line must be on a DTLS
//! profile (`UDP/TLS/RTP/SAVPF` for media, `UDP/DTLS/SCTP` for the data channel), the offer must
//! carry a SHA-256 DTLS fingerprint, and it must carry no `a=crypto` line (SDES: SRTP keys written
//! into the SDP, where the relay could read them). A plain `RTP/AVP` offer, or an `RTP/SAVP` one
//! keyed by SDES, is refused here. The transport refuses a peer without DTLS as well; this is the
//! first of the two locks, and the one that runs before any transport exists.
//!
//! It also refuses an offer the stream cannot serve: no VP8 video the page can receive, or no data
//! channel for control and receive audio.

/// Why an offer was refused. Each maps to the page-facing `invalidOffer`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfferRefusal {
    TooLarge,
    /// A media line on a profile other than DTLS-SRTP or DTLS-SCTP.
    NotDtls,
    /// No `a=fingerprint:sha-256`, or a fingerprint that is not SHA-256.
    NoFingerprint,
    /// SRTP keys in the SDP (SDES `a=crypto`).
    SdesKeys,
    /// No video line offering VP8 that the page can receive.
    NoVideo,
    /// No `webrtc-datachannel` line.
    NoDataChannel,
}

#[derive(Default)]
struct Section {
    kind: String,
    profile: String,
    formats: Vec<String>,
    vp8: Vec<String>,
    sends: bool,
}

/// Check an offer's SDP. The whole text is read, every media line judged.
pub fn check(sdp: &str) -> Result<(), OfferRefusal> {
    if sdp.len() > crate::protocol::SDP_BYTES {
        return Err(OfferRefusal::TooLarge);
    }
    let mut sections: Vec<Section> = Vec::new();
    let mut fingerprint = false;
    for line in sdp.lines().map(str::trim) {
        if let Some(media) = line.strip_prefix("m=") {
            let mut words = media.split_whitespace();
            let kind = words.next().unwrap_or_default().to_string();
            let _port = words.next();
            let profile = words.next().unwrap_or_default().to_string();
            sections.push(Section {
                kind,
                profile,
                formats: words.map(str::to_string).collect(),
                // The page's default direction is sendrecv, which lets the station send.
                sends: true,
                ..Section::default()
            });
            continue;
        }
        if let Some(value) = line.strip_prefix("a=fingerprint:") {
            // Every fingerprint must be SHA-256; one other hash anywhere refuses the offer.
            let hash = value.split_whitespace().next().unwrap_or_default();
            if !hash.eq_ignore_ascii_case("sha-256") {
                return Err(OfferRefusal::NoFingerprint);
            }
            fingerprint = true;
            continue;
        }
        if line.starts_with("a=crypto:") {
            return Err(OfferRefusal::SdesKeys);
        }
        let Some(section) = sections.last_mut() else {
            continue;
        };
        if let Some(map) = line.strip_prefix("a=rtpmap:") {
            let mut words = map.split_whitespace();
            let pt = words.next().unwrap_or_default();
            let codec = words.next().unwrap_or_default();
            if codec.to_ascii_uppercase().starts_with("VP8/") {
                section.vp8.push(pt.to_string());
            }
        } else if line == "a=sendonly" || line == "a=inactive" {
            // The page would send and not receive, or neither: nothing to show it on this line.
            section.sends = false;
        } else if line == "a=recvonly" || line == "a=sendrecv" {
            section.sends = true;
        }
    }
    for section in &sections {
        let dtls = match section.kind.as_str() {
            "application" => section.profile == "UDP/DTLS/SCTP",
            _ => section.profile == "UDP/TLS/RTP/SAVPF",
        };
        if !dtls {
            return Err(OfferRefusal::NotDtls);
        }
    }
    if !fingerprint {
        return Err(OfferRefusal::NoFingerprint);
    }
    let video = sections
        .iter()
        .any(|s| s.kind == "video" && s.sends && s.vp8.iter().any(|pt| s.formats.contains(pt)));
    if !video {
        return Err(OfferRefusal::NoVideo);
    }
    let data = sections
        .iter()
        .any(|s| s.kind == "application" && s.formats.iter().any(|f| f == "webrtc-datachannel"));
    if !data {
        return Err(OfferRefusal::NoDataChannel);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// The page's offer from the contract fixtures.
    fn fixture_offer() -> String {
        let file: Value = serde_json::from_str(include_str!(
            "../../../remote/test/fixtures/stream/signal.json"
        ))
        .unwrap();
        file["roomToStation"][0]["message"]["payload"]["sdp"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// CONTROL: the contract's own offer is answered. Every refusal below is this offer with one
    /// thing changed, so a refusal is about that one thing.
    #[test]
    fn the_contract_offer_is_accepted() {
        assert_eq!(check(&fixture_offer()), Ok(()));
    }

    /// ★ A4: a hand-built plain-RTP offer is refused, and so is every other non-DTLS profile.
    #[test]
    fn a_plain_rtp_offer_is_refused() {
        let offer = fixture_offer();
        for profile in [
            "RTP/AVP",
            "RTP/AVPF",
            "RTP/SAVP",
            "RTP/SAVPF",
            "TCP/TLS/RTP/SAVPF",
        ] {
            let plain = offer.replace("UDP/TLS/RTP/SAVPF", profile);
            assert_eq!(
                check(&plain),
                Err(OfferRefusal::NotDtls),
                "{profile} was accepted"
            );
        }
        let plain = offer.replace("UDP/DTLS/SCTP", "DTLS/SCTP");
        assert_eq!(check(&plain), Err(OfferRefusal::NotDtls));
    }

    #[test]
    fn an_offer_without_a_sha256_fingerprint_is_refused() {
        let offer = fixture_offer();
        let none: String = offer
            .lines()
            .filter(|l| !l.starts_with("a=fingerprint:"))
            .map(|l| format!("{l}\r\n"))
            .collect();
        assert_eq!(check(&none), Err(OfferRefusal::NoFingerprint));
        let sha1 = offer.replacen("a=fingerprint:sha-256", "a=fingerprint:sha-1", 1);
        assert_eq!(check(&sha1), Err(OfferRefusal::NoFingerprint));
    }

    #[test]
    fn sdes_keys_in_the_sdp_are_refused() {
        let offer = fixture_offer().replace(
            "a=mid:0\r\n",
            "a=mid:0\r\na=crypto:1 AES_CM_128_HMAC_SHA1_80 inline:WVNfX19zZW1jdGwgKCkgewkyMjA7fQp9CnVubGVz\r\n",
        );
        assert_eq!(check(&offer), Err(OfferRefusal::SdesKeys));
    }

    #[test]
    fn an_offer_the_stream_cannot_serve_is_refused() {
        let offer = fixture_offer();
        let no_vp8 = offer.replace("VP8/90000", "H264/90000");
        assert_eq!(check(&no_vp8), Err(OfferRefusal::NoVideo));
        // The page would only send video, never receive it.
        let sends = offer.replace("a=recvonly", "a=sendonly");
        assert_eq!(check(&sends), Err(OfferRefusal::NoVideo));
        let no_data = offer.replace("webrtc-datachannel", "something-else");
        assert_eq!(check(&no_data), Err(OfferRefusal::NoDataChannel));
        let big = format!("{offer}{}", "a=x\r\n".repeat(2000));
        assert_eq!(check(&big), Err(OfferRefusal::TooLarge));
    }
}
