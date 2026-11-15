//! The JTDX-style FT8 decoder's request: WSJT-X 3.0.2's multithreaded decoder (`lib/ft8var`, the
//! WSJT-X port of JTDX's decoder), as one full pass of a period sees it.
//!
//! The operator picks a decoder style and a preset in Settings; the engine resolves them to the
//! [`JtdxKnobs`] here and adds what the decoder needs to know about the station and the QSO
//! ([`JtdxRequest`]). A [`DecodeRequest`](crate::DecodeRequest) carrying one is decoded by ft8var on
//! its full pass; without one, FT8 decodes as it always has.

/// Every ft8var setting an operator can turn (WSJT-X's Decode menu with "Use multithreaded FT8
/// decoder" on, plus JTDX's SWL mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JtdxKnobs {
    /// Decoding cycles 1..=3: 3, 6 or 9 passes (`nft8cycles`).
    pub cycles: u8,
    /// FT8 sensitivity 1..=3: 1 minimum, 2 low sync thresholds, 3 low thresholds and the metric
    /// sub-pass (`lft8lowth`, `lft8subpass`).
    pub sensitivity: u8,
    /// QSO RX-frequency sensitivity 1..=3 (`nft8rxfsens`): how hard FT8S tries for the expected
    /// message on the RX frequency.
    pub rx_freq_sensitivity: u8,
    /// Threads: 0 = WSJT-X's automatic count, 1..=12 (`nmt`; never more than the machine's
    /// logical cores).
    pub threads: u8,
    /// The band-wide DX-call search (`lwidedxcsearch`).
    pub wide_dx_search: bool,
    /// Candidate thinning, percent: 100 keeps every sync candidate (`ncandthin`).
    pub cand_thin: u8,
    /// JTDX's SWL mode: a wider DT search, lower thresholds, a fourth metric sub-pass.
    pub swl: bool,
}

/// One full pass's ft8var inputs beyond the standard decoder's
/// ([`DecodeRequest`](crate::DecodeRequest)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JtdxRequest {
    pub knobs: JtdxKnobs,
    /// AP window, Hz, from the dial (WSJT-X: 5 below 30 MHz, 15 below 100 MHz, else 50).
    pub napwid: i32,
    /// My call and the DX call as WSJT-X gives them (`mainwindow.cpp:5578-5581`): mine always,
    /// the DX call whenever one is set, `""` when none.
    pub mycall: String,
    pub hiscall: String,
    /// The DX station's grid, `""` when none.
    pub hisgrid: String,
    /// The two calls without prefix or suffix.
    pub mybcall: String,
    pub hisbcall: String,
    /// Whether my call and the DX call are standard calls.
    pub mycall_std: bool,
    pub hiscall_std: bool,
    /// Hound (WSJT-X Fox/Hound): forces RX-frequency sensitivity 1 and turns the a8 pass off.
    pub hound: bool,
    /// A DX call is set, transmit is off and the call is not yet logged (`lenabledxcsearch`).
    pub dx_search: bool,
}

/// WSJT-X's AP window for the dial frequency (`mainwindow.cpp:5511-5513`).
pub fn napwid_for_dial(dial_hz: u64) -> i32 {
    ft8::napwid_for_dial(dial_hz)
}

/// The stack, in bytes, a thread that runs a JTDX-style decode needs (the decoder keeps its local
/// arrays on the stack): every thread that runs decode jobs is spawned with at least this.
pub const DECODE_THREAD_STACK: usize = ft8::OPENMP_THREAD_STACK;

/// Where `ALLCALL7.TXT` is, and load it once per process; whether the callsign database is valid.
pub fn load_callsign_db(dir: &std::path::Path) -> bool {
    ft8::var_load_callsign_db(dir)
}

/// What WSJT-X clears when the mode changes (the average DT, the decoder's own callsign hash
/// tables); the engine calls it where it clears the standard decoder's a7 table.
pub fn reset() {
    ft8::var_reset()
}

/// The thread count WSJT-X would use for `requested` (0 = automatic) on this machine.
pub fn threads_for(requested: u8) -> i32 {
    ft8::var_threads(i32::from(requested))
}
