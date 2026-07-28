//! The unified decode record produced by every [`Mode`](crate::Mode) and every
//! [`SignalSource`](crate::SignalSource).
//!
//! Each mode crate (`ft1`/`ft8`/`ft4`) has its own near-identical `Decode`
//! struct; this normalizes them into one type the rest of the app (roster, log,
//! spots, map, UI) consumes without caring which mode or source produced it.

use crate::mode::ModeKind;

/// A single decoded signal, normalized across all modes (FT8/FT4/FT1) and
/// signal sources (native decode engine, or an upstream WSJT-X over UDP).
#[derive(Debug, Clone, PartialEq)]
pub struct Decode {
    /// Decoded message text.
    pub message: String,
    /// Sync correlation metric (0.0 when not reported, e.g. UDP decodes).
    pub sync: f32,
    /// SNR estimate (dB, 2500 Hz BW).
    pub snr: i32,
    /// Time offset in seconds (WSJT-X convention `xdt = t − 0.5`).
    pub dt: f32,
    /// Audio carrier frequency (Hz).
    pub freq: f32,
    /// A-priori decode type used (0 = none).
    pub nap: i32,
    /// Decode quality in `[0, 1]` (1.0 = perfect; not meaningful for UDP decodes).
    pub qual: f32,
    /// IR-HARQ redundancy version recovered (FT1 only): `Some(0/1/2)`. `None` for
    /// FT8/FT4, UDP decodes, or when not reported.
    pub rv: Option<i32>,
    /// The mode that produced this decode, when known. A [`NativeSource`] tags
    /// its own [`ModeKind`]; a companion (WSJT-X UDP) source tags the upstream
    /// app's mode. `None` when the mode is unknown (e.g. DX1's robust path, which
    /// has no [`ModeKind`], or an unrecognized companion mode) — consumers then
    /// fall back to the operator's selected tier.
    ///
    /// [`NativeSource`]: crate::NativeSource
    pub mode: Option<ModeKind>,
}

impl From<tempo_fast::Decode> for Decode {
    fn from(d: tempo_fast::Decode) -> Self {
        Self {
            message: d.message,
            sync: d.sync,
            snr: d.snr,
            dt: d.dt,
            freq: d.freq,
            nap: d.nap,
            qual: d.qual,
            // ft1 reports rv = -1 when not applicable; normalize that to None.
            rv: (d.rv >= 0).then_some(d.rv),
            // The source (NativeSource / WsjtxUdpSource) tags the mode; the raw
            // type conversion can't tell native FT1 from DX1's reuse of it.
            mode: None,
        }
    }
}

impl From<fst4::Decode> for Decode {
    fn from(d: fst4::Decode) -> Self {
        Self {
            message: d.message,
            sync: d.sync,
            snr: d.snr,
            dt: d.dt,
            freq: d.freq,
            nap: d.nap,
            qual: d.qual,
            // FST4 has no redundancy-version concept (that is FT1's IR-HARQ).
            rv: None,
            mode: None,
        }
    }
}

impl From<q65::Decode> for Decode {
    fn from(d: q65::Decode) -> Self {
        Self {
            message: d.message,
            sync: d.sync,
            snr: d.snr,
            dt: d.dt,
            freq: d.freq,
            // Q65 reports `idec` (which decode strategy succeeded: 0=q0..3=q3),
            // not FT8's `iaptype`. Both answer "was a-priori information used and
            // how", so idec lands in `nap` — but they are NOT the same scale, and
            // anything comparing nap across modes has to know that. idec==3 in
            // particular means the message was matched against a candidate list
            // built from mycall/hiscall/hisgrid rather than independently
            // recovered.
            nap: d.idec,
            // Q65 has no [0,1] quality metric. Upstream's closest analogue is the
            // sync-curve value already carried in `sync`, so this stays 0.0 rather
            // than inventing a number that would rank against FT8's real one.
            qual: 0.0,
            // No redundancy-version concept (that is FT1's IR-HARQ).
            rv: None,
            mode: None,
        }
    }
}

impl From<msk144::Decode> for Decode {
    fn from(d: msk144::Decode) -> Self {
        Self {
            message: d.message,
            // MSK144 reports no sync metric — mskrtd's output line carries only
            // UTC, SNR, dt, frequency, a decode-type symbol and the message.
            sync: 0.0,
            snr: d.snr,
            dt: d.dt,
            freq: d.freq,
            // MSK144 has no a-priori decode types. `dtype` says HOW the decode was
            // recovered (single ping vs long average), which is a different axis
            // from FT8's iaptype, so it is deliberately NOT crammed into `nap` —
            // that would make a single-ping decode compare against AP levels it
            // has nothing to do with.
            nap: 0,
            qual: 0.0,
            rv: None,
            mode: None,
        }
    }
}

impl From<jt65::Decode> for Decode {
    fn from(d: jt65::Decode) -> Self {
        Self {
            message: d.message,
            sync: d.sync,
            snr: d.snr,
            dt: d.dt,
            freq: d.freq,
            // JT65 reports `ft` — Reed-Solomon vs deep search — which answers the
            // same question FT8's iaptype does: was outside information used. A
            // deep-search decode was matched against candidates built from the
            // callsigns in play, so it is the JT65 analogue of Q65's idec==3.
            nap: match d.dtype {
                jt65::DecodeType::ReedSolomon => 0,
                jt65::DecodeType::DeepSearch => 2,
                jt65::DecodeType::Other(n) => n,
            },
            // JT65's qual is a deep-search confidence COUNT, not a [0,1] fraction,
            // so it is deliberately not crammed into `qual` where it would rank
            // against FT8's normalised metric.
            qual: 0.0,
            rv: None,
            mode: None,
        }
    }
}

impl From<ft8::Decode> for Decode {
    fn from(d: ft8::Decode) -> Self {
        Self {
            message: d.message,
            sync: d.sync,
            snr: d.snr,
            dt: d.dt,
            freq: d.freq,
            nap: d.nap,
            qual: d.qual,
            rv: None,
            mode: None,
        }
    }
}

impl From<ft4::Decode> for Decode {
    fn from(d: ft4::Decode) -> Self {
        Self {
            message: d.message,
            sync: d.sync,
            snr: d.snr,
            dt: d.dt,
            freq: d.freq,
            nap: d.nap,
            qual: d.qual,
            rv: None,
            mode: None,
        }
    }
}
