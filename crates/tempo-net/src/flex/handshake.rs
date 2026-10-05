//! The connect sequence: Nexus registering as a GUI client, in the order the radio expects.
//!
//! After the prologue (`V`, then `H`), the session sends (port plan §4.5):
//!
//! 1. [`opening`]: `client program Nexus`, optionally `client low_bw_connect` (only between
//!    `program` and `gui`), then `client gui [<stored id>]`. FlexLib sends `program` first, and
//!    the bandwidth mode must be set before registration so it is part of the session.
//! 2. Then it waits for the `client gui` reply ([`Registration`]). Success carries the client id
//!    to store and present next time, so the radio restores this client's slices. Any other code
//!    ends the session, and the session is not retried until the operator connects again: a full
//!    radio (`F3000001`, "too many clients") is not fixed by asking again.
//! 3. [`after_registration`]: `client station`, `client set send_reduced_bw_dax=1`,
//!    `client set enforce_network_mtu=1 network_mtu=<n>`, `keepalive enable` (pings start), the
//!    subscriptions, `mic list`, `client udpport <port>` when a UDP port is set, and `slice list`.
//!    These go back to back: the radio processes commands in order and nothing here depends on an
//!    earlier reply.
//!
//! The UDP registration's one-byte datagram, from the socket `client udpport` registers to the
//! radio's port 4992, goes in step 3, after `mic list` and just before `client udpport`: where
//! upstream sends it (port plan §4.5, step 7). The radio learns the client's UDP endpoint from
//! the datagram's source, which firmware that answers `client udpport` with [`NOT_SUPPORTED`]
//! needs, and by then the client it comes from is registered. Sent before the session connected,
//! it reached a radio that had no client of ours yet. The session has no I/O of its own, so the
//! socket's owner gives it the sender ([`super::session::UdpRegistration`]). A `client udpport`
//! reply of [`UDP_PORT_IN_USE`] means that port and address are taken: rebind to an ephemeral
//! port and register again ([`should_retry_lan_udp_port_registration`]). Some firmware answers
//! [`NOT_SUPPORTED`], which is harmless once the datagram has gone.
//!
//! Not taken from upstream: subscribing to `radio` and `client` before `client gui` to detect a
//! live client already using our id. The plan's sequence does not need it; it is a candidate for
//! the reconnect work when the bench shows what a radio does with a reused id.
//!
//! PORTED from AetherSDR (https://github.com/aethersdr/AetherSDR, GPL-3.0; the upstream file
//! carries no per-file header, the licence is the repository's), `src/core/GuiClientRegistrationState.h`,
//! `src/core/UdpRegistrationPolicy.h` and `src/core/GuiClientIdentityPolicy.h` (`protocolSafeStation`
//! only) at commit `32fa50e4896a846a6970fa3f443bd49d667c139d` (2026-10-03), translated from C++/Qt
//! to Rust. The order of the connect sequence follows `src/models/RadioModel.cpp` (7362–7460), read
//! as protocol facts; no code is taken from it. Deliberate differences: the sequence is a pure
//! list of typed commands rather than callbacks; the automation-identity helpers are not ported
//! (Nexus has no use for them); the station sanitizer treats every Unicode whitespace character
//! as a separator. Recorded in the repo-root NOTICE (AetherSDR entry).

use super::encode::{ClientId, Command, Station, Topic};
use super::wire::Severity;

/// The program name Nexus registers under.
pub const PROGRAM: &str = "Nexus";

/// The VITA-49 packet size Nexus asks for (FlexLib's value, as upstream cites it).
pub const DEFAULT_NETWORK_MTU: u16 = 1450;

/// `client udpport` refused: this port and address are already registered by another client.
pub const UDP_PORT_IN_USE: u32 = 0x5000_00A9;

/// A command some firmware answers as not supported.
pub const NOT_SUPPORTED: u32 = 0x5000_1000;

/// The subscriptions, in the order they are sent. `tx` carries the interlock.
pub const SUBSCRIPTIONS: [Topic; 10] = [
    Topic::Slice,
    Topic::Pan,
    Topic::Tx,
    Topic::Atu,
    Topic::Meter,
    Topic::Audio,
    Topic::Gps,
    Topic::Client,
    Topic::Radio,
    Topic::Xvtr,
];

/// What the connect sequence is built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectConfig {
    pub station: Station,
    /// The client id the radio issued on an earlier connect.
    pub client_id: Option<ClientId>,
    pub low_bandwidth: bool,
    pub network_mtu: u16,
    /// The local UDP port for VITA-49, when this session registers one.
    pub udp_port: Option<u16>,
}

/// Steps 2–3 of the sequence: identify the program, set the bandwidth mode, register.
pub fn opening(config: &ConnectConfig) -> Vec<Command> {
    let mut commands = vec![Command::ClientProgram(
        Station::new(PROGRAM).expect("the program name is a safe token"),
    )];
    if config.low_bandwidth {
        commands.push(Command::ClientLowBwConnect);
    }
    commands.push(Command::ClientGui(config.client_id.clone()));
    commands
}

/// Steps 4–8, sent once registration succeeds.
pub fn after_registration(config: &ConnectConfig) -> Vec<Command> {
    let mut commands = vec![
        Command::ClientStation(config.station.clone()),
        Command::ClientSetReducedBwDax,
        Command::ClientSetNetworkMtu(config.network_mtu),
        Command::KeepaliveEnable,
    ];
    commands.extend(SUBSCRIPTIONS.iter().map(|t| Command::Subscribe(*t)));
    commands.push(Command::MicList);
    if let Some(port) = config.udp_port {
        commands.push(Command::ClientUdpPort(port));
    }
    commands.push(Command::SliceList);
    commands
}

/// Where GUI-client registration is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationPhase {
    Idle,
    AwaitingReply,
    Registered,
    Rejected,
}

/// What to do after the `client gui` reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationAction {
    ContinueHandshake,
    /// End the session and wait for the operator: no retry.
    DisconnectAndWaitForUser,
}

/// The outcome of registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrationResult {
    pub action: RegistrationAction,
    pub code: u32,
    /// Why the radio refused: the reply's text, or the last error or fatal message the radio sent
    /// while the reply was awaited.
    pub detail: String,
}

impl RegistrationResult {
    pub fn accepted(&self) -> bool {
        self.action == RegistrationAction::ContinueHandshake
    }
}

/// The `client gui` registration state machine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Registration {
    phase: Option<RegistrationPhase>,
    radio_error_detail: String,
}

impl Registration {
    pub fn phase(&self) -> RegistrationPhase {
        self.phase.unwrap_or(RegistrationPhase::Idle)
    }

    /// `client gui` was sent.
    pub fn begin(&mut self) {
        self.phase = Some(RegistrationPhase::AwaitingReply);
        self.radio_error_detail.clear();
    }

    /// An `M` line arrived. While the reply is awaited, an error or fatal message is kept as the
    /// detail to show if registration is refused (a full radio says why in a message before its
    /// reply).
    pub fn note_radio_message(&mut self, text: &str, severity: Severity) {
        if self.phase() != RegistrationPhase::AwaitingReply
            || !matches!(severity, Severity::Error | Severity::Fatal)
        {
            return;
        }
        let detail = text.trim();
        if !detail.is_empty() {
            self.radio_error_detail = detail.to_string();
        }
    }

    /// The `client gui` reply arrived.
    pub fn complete(&mut self, code: u32, body: &str) -> RegistrationResult {
        if code == 0 {
            self.phase = Some(RegistrationPhase::Registered);
            self.radio_error_detail.clear();
            return RegistrationResult {
                action: RegistrationAction::ContinueHandshake,
                code,
                detail: String::new(),
            };
        }
        self.phase = Some(RegistrationPhase::Rejected);
        let mut detail = body.trim().to_string();
        if detail.is_empty() {
            detail = std::mem::take(&mut self.radio_error_detail);
        }
        self.radio_error_detail.clear();
        RegistrationResult {
            action: RegistrationAction::DisconnectAndWaitForUser,
            code,
            detail,
        }
    }

    /// The session ended.
    pub fn reset(&mut self) {
        self.phase = Some(RegistrationPhase::Idle);
        self.radio_error_detail.clear();
    }
}

/// Whether a `client udpport` refusal means the port and address are taken.
pub fn is_udp_port_in_use(code: u32, body: &str) -> bool {
    code == UDP_PORT_IN_USE
        || body
            .to_ascii_lowercase()
            .contains("port/ip pair already in use")
}

/// Whether to rebind and register again: on the LAN only, and only for a taken port.
pub fn should_retry_lan_udp_port_registration(is_wan: bool, code: u32, body: &str) -> bool {
    !is_wan && is_udp_port_in_use(code, body)
}

/// A station name for the unquoted protocol field: trimmed, every run of whitespace, `|`, `=` or
/// control characters replaced by one `-`, cut to 48 characters, and leading or trailing dashes
/// removed (cutting first, so a cut after a dash cannot leave one at the end).
pub fn protocol_safe_station(value: &str) -> String {
    let unsafe_char =
        |c: char| c.is_whitespace() || c == '|' || c == '=' || c.is_control() || c == '\u{7f}';
    let mut out = String::new();
    let mut in_run = false;
    for c in value.trim().chars() {
        if unsafe_char(c) {
            if !in_run {
                out.push('-');
            }
            in_run = true;
        } else {
            out.push(c);
            in_run = false;
        }
    }
    let cut: String = out.chars().take(48).collect();
    cut.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    //! Translated from upstream's `gui_client_registration_state_test.cpp`, the station rows of
    //! `gui_client_identity_policy_test.cpp` and the UDP rows of `radio_status_ownership_test.cpp`.
    //! The registration recovery test (`gui_client_registration_recovery_test.cpp`) runs against
    //! the simulator in `super::super::sim_tests`.
    use super::*;

    fn config() -> ConnectConfig {
        ConnectConfig {
            station: Station::new("Shack").unwrap(),
            client_id: None,
            low_bandwidth: false,
            network_mtu: DEFAULT_NETWORK_MTU,
            udp_port: Some(4993),
        }
    }

    #[test]
    fn registration_state_machine() {
        let mut state = Registration::default();
        assert_eq!(state.phase(), RegistrationPhase::Idle, "starts idle");
        state.begin();
        let accepted = state.complete(0, "");
        assert!(accepted.accepted(), "success continues the handshake");
        assert_eq!(state.phase(), RegistrationPhase::Registered);

        state.begin();
        state.note_radio_message(
            "The maximum number of connected clients has been reached",
            Severity::Fatal,
        );
        let full = state.complete(0xF300_0001, "");
        assert!(!full.accepted(), "a nonzero reply rejects");
        assert_eq!(state.phase(), RegistrationPhase::Rejected);
        assert_eq!(full.action, RegistrationAction::DisconnectAndWaitForUser);
        assert_eq!(
            full.detail, "The maximum number of connected clients has been reached",
            "the fatal message supplies the detail when the reply body is empty"
        );

        state.begin();
        state.note_radio_message("older fatal detail", Severity::Fatal);
        let body = state.complete(0xF300_0001, "reply-specific detail");
        assert_eq!(
            body.detail, "reply-specific detail",
            "the reply's own text wins"
        );

        state.begin();
        state.note_radio_message("routine notice", Severity::Info);
        assert!(
            state.complete(0xF300_0001, "").detail.is_empty(),
            "a routine message never becomes the failure detail"
        );
        state.reset();
        assert_eq!(state.phase(), RegistrationPhase::Idle);
    }

    #[test]
    fn a_message_outside_the_wait_is_not_kept() {
        let mut state = Registration::default();
        state.note_radio_message("stale fatal", Severity::Fatal);
        state.begin();
        assert!(state.complete(0xF300_0001, "").detail.is_empty());
    }

    #[test]
    fn station_names_are_safe_for_the_unquoted_field() {
        assert_eq!(
            protocol_safe_station(" Codex / GPT-5.6 "),
            "Codex-/-GPT-5.6"
        );
        assert!(protocol_safe_station(&"x".repeat(80)).chars().count() <= 48);
        assert_eq!(protocol_safe_station("a|b=c\td"), "a-b-c-d");
        assert_eq!(protocol_safe_station("--x--"), "x");
        // Cutting first: a cut that lands right after a dash leaves none at the end.
        let name = format!("{}-tail", "y".repeat(47));
        assert_eq!(protocol_safe_station(&name), "y".repeat(47));
        assert_eq!(protocol_safe_station(" \u{a0} "), "");
    }

    #[test]
    fn udp_registration_policy() {
        assert!(is_udp_port_in_use(UDP_PORT_IN_USE, ""));
        assert!(is_udp_port_in_use(1, "Port/IP pair already in use"));
        assert!(is_udp_port_in_use(1, "port/ip PAIR already IN use"));
        assert!(!is_udp_port_in_use(1, "command syntax error"));
        assert!(should_retry_lan_udp_port_registration(
            false,
            UDP_PORT_IN_USE,
            ""
        ));
        assert!(!should_retry_lan_udp_port_registration(
            true,
            UDP_PORT_IN_USE,
            ""
        ));
    }

    #[test]
    fn the_sequence_follows_the_plan() {
        let mut cfg = config();
        assert_eq!(
            opening(&cfg),
            [
                Command::ClientProgram(Station::new("Nexus").unwrap()),
                Command::ClientGui(None)
            ]
        );
        cfg.low_bandwidth = true;
        cfg.client_id = ClientId::parse("6F1C2A3B-0000-4000-8000-00000000B001");
        let open = opening(&cfg);
        assert_eq!(
            open[1],
            Command::ClientLowBwConnect,
            "low bandwidth sits between program and gui"
        );
        assert_eq!(open[2], Command::ClientGui(cfg.client_id.clone()));

        let after = after_registration(&cfg);
        assert_eq!(
            after[0],
            Command::ClientStation(Station::new("Shack").unwrap())
        );
        assert_eq!(after[3], Command::KeepaliveEnable);
        let subs: Vec<Topic> = after
            .iter()
            .filter_map(|c| match c {
                Command::Subscribe(t) => Some(*t),
                _ => None,
            })
            .collect();
        assert_eq!(subs, SUBSCRIPTIONS);
        assert_eq!(after[after.len() - 2], Command::ClientUdpPort(4993));
        assert_eq!(
            after.last(),
            Some(&Command::SliceList),
            "slice list is the last step"
        );
        cfg.udp_port = None;
        assert!(!after_registration(&cfg)
            .iter()
            .any(|c| matches!(c, Command::ClientUdpPort(_))));
    }
}
