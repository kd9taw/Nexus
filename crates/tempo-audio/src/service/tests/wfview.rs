//! Nexus beside wfview, the program many Icom owners (and some Kenwood and Yaesu ones) use to reach
//! their radio over the network, which serves a rigctld of its own. Checked through Nexus's real
//! `open_cat`, `Rig` and `RadioLoop::step` against a fake wfview rigctld.
//!
//! The fake answers as wfview 2.23 does, by this crate's reading of its `src/rigctld.cpp` (nothing
//! of it is ported): the complete lines of one read are one batch, answered at its end; a set
//! answers `RPRT <n>` and ends the batch; a read answers its value and no `RPRT`; a name its
//! command table does not list gets no answer at all; `\chk_vfo` is `ChkVFO: 0`, no `RPRT`, and
//! makes that connection's `\dump_state` end with key=value lines and `done`; a frequency reads
//! `14074000.000000`; and `\get_powerstat` is wfview's power cache, `0` until the radio has
//! answered wfview. That wfview itself behaves so is the model, not a measurement.
//!
//! Nexus is pointed at it with Rig Model NET rigctl and wfview's address. On this computer Nexus
//! shares wfview's rigctld, whatever rigctld TCP Port says, and while nothing answers there it
//! starts nothing and waits for it. On another computer Nexus talks to it
//! through a Hamlib rigctld of its own (`-m 2`, on rigctld TCP Port), which needs a Hamlib `rigctld`
//! here, and this machine's own address on its network to stand in for the other computer.
use super::*;
use std::io::{Read, Write};
use std::net::TcpStream;

/// The radio behind the fake wfview, and what wfview was sent.
struct Radio {
    hz: u64,
    mode: String,
    ptt: bool,
    /// wfview's power cache, which `\get_powerstat` reads.
    powered: bool,
    /// Every line, in order.
    heard: Vec<String>,
    /// The lines wfview answers with nothing, because its table does not list them.
    unanswered: Vec<String>,
    /// Lines this fake does not model (wfview's extended replies, a read with an argument):
    /// none should arrive, and a scene asserts that.
    unmodelled: Vec<String>,
}

struct FakeWfview {
    addr: String,
    radio: Arc<Mutex<Radio>>,
}

impl FakeWfview {
    /// A wfview on an ephemeral port, its radio on 14.074 MHz USB and unkeyed.
    fn start(powered: bool) -> Self {
        Self::start_on("127.0.0.1", powered)
    }

    /// [`Self::start`], listening on `ip` rather than loopback.
    fn start_on(ip: &str, powered: bool) -> Self {
        Self::serving(std::net::TcpListener::bind((ip, 0)).unwrap(), powered)
    }

    /// [`Self::start`], listening at `addr`: a wfview started after Nexus, at the address Nexus was
    /// already pointed at.
    fn at(addr: &str, powered: bool) -> Self {
        Self::serving(std::net::TcpListener::bind(addr).unwrap(), powered)
    }

    fn serving(listener: std::net::TcpListener, powered: bool) -> Self {
        let addr = listener.local_addr().unwrap().to_string();
        let radio = Arc::new(Mutex::new(Radio {
            hz: 14_074_000,
            mode: "USB".into(),
            ptt: false,
            powered,
            heard: Vec::new(),
            unanswered: Vec::new(),
            unmodelled: Vec::new(),
        }));
        let shared = radio.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let radio = shared.clone();
                std::thread::spawn(move || serve(stream, &radio));
            }
        });
        FakeWfview { addr, radio }
    }

    fn port(&self) -> u16 {
        self.addr.rsplit(':').next().unwrap().parse().unwrap()
    }

    /// The PTT lines wfview was sent, in order.
    fn keyed_with(&self) -> Vec<String> {
        let r = self.radio.lock().unwrap();
        r.heard
            .iter()
            .filter(|l| l.starts_with("T ") || l.starts_with("\\set_ptt"))
            .cloned()
            .collect()
    }
}

/// One wfview client connection: answer each read's batch of lines.
fn serve(mut stream: TcpStream, radio: &Mutex<Radio>) {
    let (mut pending, mut chk_vfo) = (Vec::new(), false);
    let mut buf = [0u8; 1024];
    loop {
        let n = match stream.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        pending.extend_from_slice(&buf[..n]);
        let Some(end) = pending.iter().rposition(|b| *b == b'\n') else {
            continue;
        };
        let batch: Vec<u8> = pending.drain(..=end).collect();
        let reply = answer(
            &String::from_utf8_lossy(&batch),
            &mut chk_vfo,
            &mut radio.lock().unwrap(),
        );
        if !reply.is_empty() && stream.write_all(reply.as_bytes()).is_err() {
            return;
        }
    }
}

/// How wfview's command table (`commands_list`) treats an entry: by its flags, and whether a
/// function stands behind it.
#[derive(Clone, Copy, PartialEq)]
enum Entry {
    /// `ARG_IN` with a function: takes it, `RPRT 0`, and the batch ends.
    Set,
    /// A read with a function: its value, no `RPRT`.
    Read,
    /// `ARG_IN` with no function (`funcNone`): nothing handles it, `RPRT -1`, and the batch ends.
    SetNothing,
    /// A read with no function: `RPRT -1`.
    ReadNothing,
    /// Listed, with a function, and answered with nothing (`\stop_morse`, `\wait_morse`: a read
    /// of a string, which wfview queues and does not answer).
    Quiet,
    /// `\dump_state`, `\chk_vfo`, the level and function verbs: their own code. The rest of
    /// these (`\get_vfo_info`, the parameter verbs, `\dump_caps`, `\dump_conf`) are not modelled.
    Own,
}

/// wfview 2.23's table, one-letter form (where it has one), long name and treatment.
const TABLE: &[(Option<char>, &str, Entry)] = {
    use Entry::*;
    &[
        (Some('F'), "set_freq", Set),
        (Some('f'), "get_freq", Read),
        (Some('M'), "set_mode", Set),
        (Some('m'), "get_mode", Read),
        (Some('I'), "set_split_freq", Set),
        (Some('i'), "get_split_freq", Read),
        (Some('X'), "set_split_mode", Set),
        (Some('x'), "get_split_mode", Read),
        (Some('K'), "set_split_freq_mode", Set),
        (Some('k'), "get_split_freq_mode", Read),
        (Some('S'), "set_split_vfo", Set),
        (Some('s'), "get_split_vfo", Read),
        (Some('N'), "set_ts", Set),
        (Some('n'), "get_ts", Read),
        (Some('L'), "set_level", Own),
        (Some('l'), "get_level", Own),
        (Some('U'), "set_func", Own),
        (Some('u'), "get_func", Own),
        (Some('P'), "set_parm", Own),
        (Some('p'), "get_parm", Own),
        (Some('G'), "vfo_op", Set),
        (Some('g'), "scan", Set),
        (Some('A'), "set_trn", Set),
        (Some('a'), "get_trn", Read),
        (Some('R'), "set_rptr_shift", Set),
        (Some('r'), "get_rptr_shift", Read),
        (Some('O'), "set_rptr_offs", Set),
        (Some('o'), "get_rptr_offs", Read),
        (Some('C'), "set_ctcss_tone", Set),
        (Some('c'), "get_ctcss_tone", Read),
        (Some('D'), "set_dcs_code", Set),
        (Some('d'), "get_dcs_code", Read),
        (None, "set_ctcss_sql", Set),
        (None, "get_ctcss_sql", Read),
        (None, "set_dcs_sql", Set),
        (None, "get_dcs_sql", Read),
        (Some('V'), "set_vfo", Set),
        (Some('v'), "get_vfo", Read),
        (Some('T'), "set_ptt", Set),
        (Some('t'), "get_ptt", Read),
        (Some('E'), "set_mem", Set),
        (Some('e'), "get_mem", Read),
        (Some('H'), "set_channel", Set),
        (Some('h'), "get_channel", Set),
        (Some('B'), "set_bank", Set),
        (Some('_'), "get_info", ReadNothing),
        (Some('J'), "set_rit", Set),
        (Some('j'), "get_rit", Read),
        (Some('Z'), "set_xit", Set),
        (Some('z'), "get_xit", Read),
        (Some('Y'), "set_ant", Set),
        (Some('y'), "get_ant", Read),
        (None, "set_powerstat", Set),
        (None, "get_powerstat", Read),
        (None, "send_dtmf", SetNothing),
        (None, "recv_dtmf", ReadNothing),
        (Some('*'), "reset", SetNothing),
        (Some('w'), "send_cmd", ReadNothing),
        (Some('W'), "send_cmd_rx", SetNothing),
        (Some('b'), "send_morse", Set),
        (None, "stop_morse", Quiet),
        (None, "wait_morse", Quiet),
        (None, "send_voice_mem", SetNothing),
        (None, "get_dcd", ReadNothing),
        (None, "set_twiddle", SetNothing),
        (None, "get_twiddle", ReadNothing),
        (None, "uplink", SetNothing),
        (None, "set_cache", SetNothing),
        (None, "get_cache", ReadNothing),
        (Some('1'), "dump_caps", Own),
        (Some('3'), "dump_conf", Own),
        (None, "dump_state", Own),
        (None, "chk_vfo", Own),
        (None, "set_vfo_opt", SetNothing),
        (None, "get_vfo_info", Own),
        (None, "get_rig_info", ReadNothing),
        (None, "get_vfo_list", ReadNothing),
        (None, "get_modes", ReadNothing),
        (None, "get_clock", Read),
        (None, "set_clock", Set),
        (None, "halt", ReadNothing),
        (None, "pause", SetNothing),
        (None, "password", SetNothing),
        (None, "get_mode_bandwidths", SetNothing),
        (None, "set_separator", Set),
        (None, "get_separator", Read),
        (None, "set_lock_mode", Set),
        (None, "get_lock_mode", Read),
        (None, "send_raw", ReadNothing),
        (None, "client_version", ReadNothing),
    ]
};

/// wfview's level and function names (`levels_str`, `functions_str`): a read of one of these is
/// answered from its cache; any other name is `RPRT -1`.
const LEVELS: &[&str] = &[
    "PREAMP",
    "ATT",
    "VOXDELAY",
    "AF",
    "RF",
    "SQL",
    "IF",
    "APF",
    "NR",
    "PBT_IN",
    "PBT_OUT",
    "CWPITCH",
    "RFPOWER",
    "MICGAIN",
    "KEYSPD",
    "NOTCHF",
    "COMP",
    "AGC",
    "BKINDL",
    "BAL",
    "METER",
    "VOXGAIN",
    "ANTIVOX",
    "SLOPE_LOW",
    "SLOPE_HIGH",
    "BKIN_DLYMS",
    "RAWSTR",
    "SWR",
    "ALC",
    "STRENGTH",
    "RFPOWER_METER",
    "COMPMETER",
    "VD_METER",
    "ID_METER",
    "NOTCHF_RAW",
    "MONITOR_GAIN",
    "NQ",
    "RFPOWER_METER_WATT",
    "SPECTRUM_MDOE",
    "SPECTRUM_SPAN",
    "SPECTRUM_EDGE_LOW",
    "SPECTRUM_EDGE_HIGH",
    "SPECTRUM_SPEED",
    "SPECTRUM_REF",
    "SPECTRUM_AVG",
    "SPECTRUM_ATT",
    "TEMP_METER",
    "BAND_SELECT",
    "USB_AF",
];
const FUNCS: &[&str] = &[
    "FAGC", "NB", "COMP", "VOX", "TONE", "TQSL", "SBKIN", "FBKIN", "ANF", "NR", "AIP", "APF",
    "MON", "MN", "RF", "ARO", "LOCK", "MUTE", "VSC", "REV", "SQL", "ABM", "BC", "MBC", "RIT",
    "AFC", "SATMODE", "SCOPE", "RESUME", "TBURST", "TUNER", "XIT",
];
/// The mode names wfview's `set_mode` takes.
const MODES: &[&str] = &[
    "AM", "CW", "USB", "LSB", "RTTY", "FM", "WFM", "CWR", "RTTYR", "AMS", "PKTLSB", "PKTUSB",
    "PKTFM", "PKTFMN", "ECSSUSB", "ECSSLSB", "FAX", "SAM", "SAL", "SAH", "DSB", "FMN", "PKTAM",
    "DPMR", "DCR", "AMN", "PSK", "PSKR", "SPEC",
];

/// The dump an IC-7300 behind wfview gives: wfview's layout, with this radio's bands, steps,
/// filters, preamps (10 and 20, wfview's `num * 10`, with its leading space) and attenuator.
const DUMP: &[&str] = &[
    "1",
    "3073",
    "0",
    "30000.000000 74800000.000000 0x1ff -1 -1 0x10000003 0x0",
    "0 0 0 0 0 0 0",
    "1800000.000000 2000000.000000 0x1ff 2000 100000 0x16000000 0x0",
    "14000000.000000 14350000.000000 0x1ff 2000 100000 0x16000000 0x0",
    "0 0 0 0 0 0 0",
    "0x1ff 10",
    "0x1ff 100",
    "0 0",
    "0x3 3000",
    "0x3 2400",
    "0x3 1800",
    "0 0",
    "9900",
    "9900",
    "10000",
    "0",
    " 10 20",
    " 20",
    "0x000000000000a00c",
    "0x000000000000a00c",
    "0x0000000040000416",
    "0x0000000040000416",
    "0x0000000000000000",
    "0x0000000000000000",
];
/// What `\dump_state` adds on a connection that has sent `\chk_vfo`.
const DUMP_AFTER_CHK_VFO: &[&str] = &[
    "vfo_ops=0xff",
    "ptt_type=0x1",
    "has_set_vfo=0x1",
    "has_get_vfo=0x1",
    "has_set_freq=0x1",
    "has_get_freq=0x1",
    "has_set_conf=0x1",
    "has_get_conf=0x1",
    "has_power2mW=0x1",
    "has_mW2power=0x1",
    "timeout=0x3e8",
    "done",
];

/// wfview's answer to one read's lines (`socketReadyRead`), empty for none.
fn answer(batch: &str, chk_vfo: &mut bool, radio: &mut Radio) -> String {
    let mut lines: Vec<String> = Vec::new();
    // The last line's verdict: (found, sends a status, ret). A set ends the batch.
    let (mut found, mut status, mut ret, mut set) = (false, true, -1, false);
    for raw in batch.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        radio.heard.push(line.to_string());
        if line.starts_with([';', '|', ',', '+']) {
            radio.unmodelled.push(line.to_string());
            found = false;
            continue;
        }
        let long = line.starts_with('\\');
        let words: Vec<&str> = line.trim_start_matches('\\').split_whitespace().collect();
        if !long && words[0].len() > 1 {
            radio.unmodelled.push(line.to_string());
            found = false;
            continue;
        }
        let entry = TABLE.iter().find(|(short, name, _)| {
            if long {
                *name == words[0]
            } else {
                *short == words[0].chars().next()
            }
        });
        let Some(&(_, name, kind)) = entry else {
            radio.unanswered.push(line.to_string());
            found = false;
            continue;
        };
        let args = &words[1..];
        (found, status) = (true, true);
        match (kind, name) {
            (Entry::Own, "chk_vfo") => {
                *chk_vfo = true;
                lines.push("ChkVFO: 0".into());
                (status, ret) = (false, 0);
            }
            (Entry::Own, "dump_state") => {
                lines.extend(DUMP.iter().map(|l| l.to_string()));
                if *chk_vfo {
                    lines.extend(DUMP_AFTER_CHK_VFO.iter().map(|l| l.to_string()));
                }
                ret = 0;
            }
            (Entry::Own, "get_level" | "get_func") => {
                let names = if name == "get_level" { LEVELS } else { FUNCS };
                match args {
                    [n] if names.contains(n) => {
                        lines.push(if *n == "STRENGTH" {
                            "-54.000000".into()
                        } else if name == "get_level" {
                            "0.000000".into()
                        } else {
                            "0".into()
                        });
                        ret = 0;
                    }
                    _ => ret = -1,
                }
            }
            (Entry::Own, "set_level" | "set_func") => {
                let names = if name == "set_level" { LEVELS } else { FUNCS };
                ret = if args.len() > 1 && names.contains(&args[0]) {
                    0
                } else {
                    -1
                };
                set = true;
            }
            (Entry::Own, _) => {
                radio.unmodelled.push(line.to_string());
                found = false;
            }
            (Entry::Quiet, _) => ret = 0,
            (Entry::Set, _) if !args.is_empty() => {
                ret = 0;
                match name {
                    "set_freq" => match args.last().and_then(|a| a.parse::<f64>().ok()) {
                        Some(hz) => radio.hz = hz as u64,
                        None => ret = -1,
                    },
                    // VFO, mode and passband, or mode and passband.
                    "set_mode" => match args {
                        [_, mode, _] | [mode, _] if MODES.contains(mode) => {
                            radio.mode = mode.to_string()
                        }
                        _ => ret = -1,
                    },
                    // A VFO before the flag selects it first; the flag is read as a number, and any
                    // number but 0 keys (`T 3` does).
                    "set_ptt" => {
                        radio.ptt =
                            args.last().and_then(|a| a.parse::<i32>().ok()).unwrap_or(0) != 0
                    }
                    _ => {}
                }
                set = true;
            }
            (Entry::SetNothing, _) => {
                ret = -1;
                set = true;
            }
            (Entry::ReadNothing, _) => ret = -1,
            // A read, or a set with nothing to set, which wfview answers as the read.
            (Entry::Set | Entry::Read, _) => {
                if !args.is_empty() {
                    radio.unmodelled.push(line.to_string());
                }
                ret = 0;
                match name {
                    "get_freq" | "get_split_freq" | "set_freq" | "set_split_freq" => {
                        lines.push(format!("{}.000000", radio.hz))
                    }
                    "get_mode" | "get_split_mode" | "set_mode" | "set_split_mode" => {
                        lines.push(radio.mode.clone());
                        lines.push("2400".into());
                    }
                    "get_ptt" | "set_ptt" => lines.push(u8::from(radio.ptt).to_string()),
                    "get_powerstat" | "set_powerstat" => {
                        lines.push(u8::from(radio.powered).to_string())
                    }
                    "get_vfo" | "set_vfo" => lines.push("VFOA".into()),
                    "get_split_vfo" | "set_split_vfo" => {
                        lines.push("0".into());
                        lines.push("VFOA".into());
                    }
                    _ => lines.push("0".into()),
                }
            }
        }
        if set {
            break;
        }
    }
    let mut out: String = lines
        .iter()
        .filter(|l| !l.is_empty())
        .map(|l| format!("{l}\n"))
        .collect();
    if found && status && (ret < 0 || set) {
        out.push_str(&format!("RPRT {ret}\n"));
    }
    out
}

/// The station as Settings ▸ Radio has it: Connection Network, Rig Model NET rigctl, Network
/// Address `wfview`, rigctld TCP Port `rigctld_port`, PTT over CAT, the CAT broker off; Phone on
/// 20 m.
fn station(wfview: &str, rigctld_port: u16) -> (Arc<Mutex<Engine>>, tempo_app::settings::Settings) {
    let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
    let settings = {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.rig_model = 2;
        s.rig_conn = "network".into();
        s.rig_addr = wfview.into();
        s.rigctld_port = rigctld_port;
        s.ptt_method = "cat".into();
        s.cat_broker = false;
        e.apply_settings(s);
        e.set_operating_mode("phone", false);
        e.set_frequency(14.074, "20m", "USB");
        e.settings().clone()
    };
    (engine, settings)
}

/// ⭐ NEXUS SHARES WFVIEW'S RIGCTLD AND KEYS THROUGH IT. rigctld TCP Port is wfview's port: the
/// open finds a rigctld there and shares it rather than starting one, reads the radio's dial and
/// mode, and keys and unkeys it with `T 1` and, for Rear/Data, `T 3`; `t` reads the key back. The
/// whole session, line by line, is lines wfview answers. The probe took wfview for some other
/// program on the port (`ChkVFO: 0`), and past that the open's first read failed on
/// `14074000.000000`.
#[test]
fn nexus_shares_wfviews_rigctld_and_keys_through_it() {
    let wf = FakeWfview::start(false);
    let (_engine, settings) = station(&wf.addr, wf.port());
    let (mut rig, daemon, probe) = open_cat(
        &Transport::from_settings(&settings),
        PttMode::Cat,
        true,
        None,
    );
    let opened = (
        probe.ok,
        probe.detail,
        probe.freq_hz,
        probe.mode,
        daemon.is_none(),
    );
    let keyed = || wf.radio.lock().unwrap().ptt;
    let mic = [
        rig.ptt(true).is_ok(),
        keyed(),
        rig.read_ptt() == Some(true),
        rig.ptt(false).is_ok(),
        keyed(),
    ];
    rig.set_ptt_mode(PttMode::CatData);
    let data = [
        rig.ptt(true).is_ok(),
        keyed(),
        rig.ptt(false).is_ok(),
        keyed(),
    ];
    let r = wf.radio.lock().unwrap();
    assert_eq!(
        (
            opened,
            mic,
            data,
            r.heard.clone(),
            r.unanswered.clone(),
            r.unmodelled.clone()
        ),
        (
            (
                Some(true),
                format!(
                    "Sharing the rigctld already on :{} — Connected — 14.074 MHz",
                    wf.port()
                ),
                Some(14_074_000),
                Some("USB".to_string()),
                true,
            ),
            [true, true, true, true, false],
            [true, true, true, false],
            ["\\chk_vfo", "f", "m", "T 1", "t", "T 0", "T 3", "T 0"]
                .map(String::from)
                .to_vec(),
            vec![],
            vec![],
        )
    );
}

/// ⭐ NET RIGCTL SHARES WFVIEW'S RIGCTLD ON THIS COMPUTER, WHATEVER RIGCTLD TCP PORT SAYS. The
/// guide's setup: Network Address wfview's port, rigctld TCP Port left at a number of its own.
/// Nexus asks the Network Address, finds a rigctld, and talks to it itself, as above: no rigctld
/// of its own, the radio's dial and mode read, and `T 1` and Rear/Data's `T 3` keying the radio,
/// whether or not wfview has heard from it yet (its power cache `0` or `1`), because nothing in
/// between asks. Nexus used to start Hamlib's rigctld in front of wfview here, the one below that
/// refuses every command while wfview says the radio is off.
#[test]
fn net_rigctl_shares_wfviews_rigctld_on_this_computer_whatever_rigctld_tcp_port_says() {
    let session = |powered: bool| {
        let wf = FakeWfview::start(powered);
        let (_engine, settings) = station(&wf.addr, free_port());
        let (mut rig, daemon, probe) = open_cat(
            &Transport::from_settings(&settings),
            PttMode::Cat,
            true,
            None,
        );
        let opened = (
            probe.ok,
            probe.detail,
            probe.freq_hz,
            probe.mode,
            daemon.is_none(),
        );
        let keyed = || wf.radio.lock().unwrap().ptt;
        let mic = [
            rig.ptt(true).is_ok(),
            keyed(),
            rig.read_ptt() == Some(true),
            rig.ptt(false).is_ok(),
            keyed(),
        ];
        rig.set_ptt_mode(PttMode::CatData);
        let data = [
            rig.ptt(true).is_ok(),
            keyed(),
            rig.ptt(false).is_ok(),
            keyed(),
        ];
        drop(daemon);
        let r = wf.radio.lock().unwrap();
        (
            (powered, opened, mic, data),
            (r.heard.clone(), r.unanswered.clone(), r.unmodelled.clone()),
            wf.addr.clone(),
        )
    };
    let sessions = [false, true].map(session);
    let shared = sessions.clone().map(|((powered, ..), _, addr)| {
        (
            (
                powered,
                (
                    Some(true),
                    format!("Sharing the rigctld at {addr} — Connected — 14.074 MHz"),
                    Some(14_074_000),
                    Some("USB".to_string()),
                    true,
                ),
                [true, true, true, true, false],
                [true, true, true, false],
            ),
            (
                ["\\chk_vfo", "f", "m", "T 1", "t", "T 0", "T 3", "T 0"]
                    .map(String::from)
                    .to_vec(),
                vec![],
                vec![],
            ),
            addr,
        )
    });
    assert_eq!(sessions, shared);
}

/// A port nothing was listening on a moment ago.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// ⭐ THE RADIO LOOP RUNS A RADIO THROUGH WFVIEW'S RIGCTLD. On the shared rigctld the loop reads
/// the radio's range and attenuator and preamp steps from wfview's `\dump_state`, follows a turn
/// of the radio's own dial, and tunes the radio on a QSY; every line it sends is one wfview
/// answers, so no read waits out a deadline.
#[test]
fn the_radio_loop_runs_a_radio_through_wfviews_rigctld() {
    let wf = FakeWfview::start(false);
    let (engine, settings) = station(&wf.addr, wf.port());
    let (mut rig, _, _) = open_cat(
        &Transport::from_settings(&settings),
        PttMode::Cat,
        true,
        None,
    );
    let cfg = RadioConfig {
        rig_model: settings.rig_model,
        ..RadioConfig::default()
    };
    // Built from the same settings as the open, so the loop never rebuilds the link.
    let mut state = RadioLoop::new(Transport::from_settings(&settings), None, &cfg);
    let mut backend = MockBackend::new();
    let rebuilt = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut run_until =
        |state: &mut RadioLoop, rig: &mut Rig, done: &dyn Fn(&RadioLoop) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(4);
            while !done(state) && Instant::now() < deadline {
                let sinks = no_sinks();
                let mut station = StationSinks::new();
                let mut reopen_audio = mock_reopen_audio();
                let flag = rebuilt.clone();
                let mut reopen_rig = move |_t: &Transport, _coexist: bool| {
                    flag.store(true, std::sync::atomic::Ordering::Relaxed);
                    (Rig::vox(), None, CatProbe::status(None, ""))
                };
                state
                    .step(
                        &engine,
                        &mut backend,
                        rig,
                        &sinks,
                        now_unix_ms(),
                        &mut reopen_audio,
                        &mut reopen_rig,
                        &mut station,
                    )
                    .unwrap();
                std::thread::sleep(Duration::from_millis(20));
            }
        };
    run_until(&mut state, &mut rig, &|s| s.rx_ranges_probed);
    let probed = {
        let snap = engine.lock().unwrap().snapshot();
        (
            state.rx_ranges.clone(),
            snap.radio.att_steps_db,
            snap.radio.preamp_steps_db,
        )
    };
    // The operator turns the radio's dial.
    wf.radio.lock().unwrap().hz = 14_080_000;
    let dial = || engine.lock().unwrap().settings().dial_mhz;
    run_until(&mut state, &mut rig, &|_| dial() == 14.08);
    let followed = dial();
    // A QSY in Nexus.
    engine.lock().unwrap().set_frequency(7.074, "40m", "USB");
    run_until(&mut state, &mut rig, &|_| {
        wf.radio.lock().unwrap().hz == 7_074_000
    });
    let r = wf.radio.lock().unwrap();
    assert_eq!(
        (
            probed,
            followed,
            r.hz,
            dial(),
            state.cat_ok,
            r.unanswered.clone(),
            r.unmodelled.clone(),
            rebuilt.load(std::sync::atomic::Ordering::Relaxed),
        ),
        (
            (
                Some(vec![(30_000, 74_800_000)]),
                Some(vec![20]),
                Some(vec![10, 20]),
            ),
            14.08,
            7_074_000,
            7.074,
            Some(true),
            vec![],
            vec![],
            false,
        )
    );
}

/// Is there a Hamlib `rigctld` for the open to start? On GitHub Actions there must be: `ci.yml`
/// installs `libhamlib-utils` so that scenes like these run there, and a pass that ran nothing
/// would be a false one. Elsewhere its absence is said, loudly, and the scene is not run.
fn hamlib_rigctld_here() -> bool {
    let here = tempo_core::process::command("rigctld")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !here {
        assert!(
            std::env::var_os("GITHUB_ACTIONS").is_none_or(|v| v.is_empty()),
            "no Hamlib rigctld on PATH, and this is GitHub Actions, where ci.yml installs \
             libhamlib-utils so that this scene runs"
        );
        let mut err = std::io::stderr().lock();
        let _ = writeln!(
            err,
            "!! NOT RUN, and NOT A PASS: {} — no Hamlib rigctld on PATH.",
            std::thread::current().name().unwrap_or("<unnamed test>")
        );
    }
    here
}

/// This machine's own address on its network, the source address of its route out, found without
/// sending anything (a UDP `connect` only picks the route). It stands in for another computer:
/// `host_is_this_machine` judges an address by its form, and this is not a loopback form. `None`
/// where there is no route out and the scene cannot be staged, which is said, loudly; on GitHub
/// Actions, whose runners always have one, that is a failure.
fn this_machine_on_its_network() -> Option<String> {
    let ip = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("192.0.2.1:9")?;
            s.local_addr()
        })
        .map(|a| a.ip())
        .ok()
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified());
    if ip.is_none() {
        assert!(
            std::env::var_os("GITHUB_ACTIONS").is_none_or(|v| v.is_empty()),
            "no address of this machine outside loopback, and this is GitHub Actions"
        );
        let mut err = std::io::stderr().lock();
        let _ = writeln!(
            err,
            "!! NOT RUN, and NOT A PASS: {} — no address of this machine outside loopback.",
            std::thread::current().name().unwrap_or("<unnamed test>")
        );
    }
    ip.map(|ip| ip.to_string())
}

/// Nexus's own Hamlib rigctld in front of a wfview listening on `ip`, NET rigctl on a port of its
/// own: whether the open read the radio, whether each key and unkey was taken (`T 1`, `T 0`, then
/// Rear/Data's `T 3`, `T 0`), the PTT lines that reached wfview, and what the open said.
fn through_hamlib(ip: &str, powered: bool) -> (Option<bool>, [bool; 4], Vec<String>, String) {
    let wf = FakeWfview::start_on(ip, powered);
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let (_engine, settings) = station(&wf.addr, port);
    let (mut rig, daemon, probe) = open_cat(
        &Transport::from_settings(&settings),
        PttMode::Cat,
        true,
        None,
    );
    let mic = [rig.ptt(true).is_ok(), rig.ptt(false).is_ok()];
    rig.set_ptt_mode(PttMode::CatData);
    let data = [rig.ptt(true).is_ok(), rig.ptt(false).is_ok()];
    drop(daemon);
    (
        probe.ok,
        [mic[0], mic[1], data[0], data[1]],
        wf.keyed_with(),
        probe.detail,
    )
}

/// ⭐ THROUGH NEXUS'S OWN RIGCTLD, WFVIEW KEYS ONLY WHILE IT SAYS THE RADIO IS ON. wfview on
/// another computer is not shared directly: Nexus starts Hamlib's rigctld (`-m 2`) and talks
/// through it. Hamlib's rigctld asks `\get_powerstat` as it opens, and wfview answers from its
/// power cache: `1` once the radio has answered it, `0` until then (and after it decides the radio
/// is off). With `1` the chain opens and every key and unkey reaches wfview and is taken. With `0`
/// Hamlib refuses Nexus's commands itself, before any reaches wfview: no `T` arrives there, and the
/// open does not read the radio. (Hamlib 4.5.5 fails its own open and answers nothing; 4.7.1,
/// which Nexus ships, opens and answers each command `RPRT -20`, "command not allowed when rig is
/// powered off".) Sharing wfview's rigctld on this computer, above, asks no power state.
#[test]
fn through_nexuss_own_rigctld_wfview_keys_only_while_it_says_the_radio_is_on() {
    if !hamlib_rigctld_here() {
        return;
    }
    let Some(ip) = this_machine_on_its_network() else {
        return;
    };
    let (on, off) = (through_hamlib(&ip, true), through_hamlib(&ip, false));
    eprintln!(
        "two hops, wfview's power cache 1: {}\ntwo hops, wfview's power cache 0: {}",
        on.3, off.3
    );
    assert_eq!(
        ((on.0, on.1, on.2), (off.0, off.1, off.2)),
        (
            (
                Some(true),
                [true; 4],
                ["T 1", "T 0", "T 3", "T 0"].map(String::from).to_vec(),
            ),
            (Some(false), [false; 4], vec![]),
        )
    );
}

/// ⭐ WHERE THE NETWORK ADDRESS HOLDS NO RIGCTLD NEXUS MAY SHARE, NEXUS STARTS ITS OWN, AS BEFORE.
/// NET rigctl on this computer, rigctld TCP Port at a number of its own, and at the Network
/// Address: Thetis's CAT server, which answers, but not as a rigctld; wfview, opened by a
/// dual-radio switch that reuses its own rigctld's port, which shares nothing it finds; wfview on
/// the port this station's CAT broker is set to, where Nexus could be talking to itself; and
/// wfview behind a Rig Model that is not NET rigctl (Hamlib's Dummy, which opens at once). Each
/// starts Nexus's own rigctld, and none shares what is at the address. Where nothing answers
/// there, Nexus starts nothing at all (below).
#[test]
fn nexus_starts_its_own_rigctld_where_the_network_address_is_not_one_to_share() {
    if !hamlib_rigctld_here() {
        return;
    }
    let wf = FakeWfview::start(true);
    let net = |addr: &str| station(addr, free_port()).1;
    let mut broker = net(&wf.addr);
    broker.cat_broker = true;
    broker.cat_broker_port = wf.port();
    let mut dummy = net(&wf.addr);
    dummy.rig_model = 1;
    let rows = [
        (
            "Thetis's CAT server",
            net(&format!("127.0.0.1:{}", fake_thetis_cat_server())),
            true,
        ),
        ("a switch reusing its own port", net(&wf.addr), false),
        ("the CAT broker's port", broker, true),
        ("Rig Model Dummy", dummy, true),
    ];
    let opened: Vec<_> = std::thread::scope(|scope| {
        let opens: Vec<_> = rows
            .iter()
            .map(|(what, settings, coexist)| {
                scope.spawn(move || {
                    let (_rig, daemon, probe) = open_cat(
                        &Transport::from_settings(settings),
                        PttMode::Cat,
                        *coexist,
                        None,
                    );
                    eprintln!("{what}: {}", probe.detail);
                    (*what, daemon.is_some(), probe.detail.starts_with("Sharing"))
                })
            })
            .collect();
        opens.into_iter().map(|o| o.join().unwrap()).collect()
    });
    assert_eq!(
        opened,
        rows.map(|(what, _, _)| (what, true, false)).to_vec()
    );
}

/// What the CAT status says while Nexus waits for the rigctld at `addr`.
fn nothing_answering_at(addr: &str) -> String {
    format!("Nothing is answering at {addr} — start wfview (or your rigctld)")
}

/// ⭐ WITH NOTHING ANSWERING AT THE NETWORK ADDRESS, NEXUS STARTS NOTHING AND SAYS SO. NET rigctl
/// on this computer, rigctld TCP Port at a number of its own, and at the Network Address nothing
/// listening, or a listener that says nothing: no rigctld of Nexus's own, and the CAT status names
/// the address and what to start there. Nexus used to start Hamlib's rigctld in front of the
/// address, so the start order decided the path.
#[test]
fn with_nothing_answering_at_the_network_address_nexus_starts_nothing_and_says_so() {
    let quiet = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let rows = [
        ("nothing listening", format!("127.0.0.1:{}", free_port())),
        (
            "a listener that says nothing",
            quiet.local_addr().unwrap().to_string(),
        ),
    ];
    let opened: Vec<_> = std::thread::scope(|scope| {
        let opens: Vec<_> = rows
            .iter()
            .map(|(what, addr)| {
                scope.spawn(move || {
                    let (_rig, daemon, probe) = open_cat(
                        &Transport::from_settings(&station(addr, free_port()).1),
                        PttMode::Cat,
                        true,
                        None,
                    );
                    eprintln!("{what}: {}", probe.detail);
                    (*what, daemon.is_some(), probe.ok, probe.detail)
                })
            })
            .collect();
        opens.into_iter().map(|o| o.join().unwrap()).collect()
    });
    assert_eq!(
        opened,
        rows.map(|(what, addr)| (what, false, Some(false), nothing_answering_at(&addr)))
            .to_vec()
    );
}

/// ⭐ STARTED BEFORE WFVIEW, NEXUS WAITS FOR IT AND THEN SHARES IT. The launch as `run_radio`
/// makes it, NET rigctl on this computer with nothing at the Network Address yet: the open starts
/// no rigctld in between and says what is missing; the loop asks the address again on the reopen
/// backoff and says the same; once wfview is up, the next ask shares its rigctld directly, and
/// `T 1` reaches it. Every line Nexus sends there is one wfview answers.
#[test]
fn started_before_wfview_nexus_waits_for_it_and_then_shares_it() {
    let addr = format!("127.0.0.1:{}", free_port());
    let (engine, settings) = station(&addr, free_port());
    let applied = Transport::from_settings(&settings);
    let (mut rig, daemon, probe) = open_rig(&applied, true);
    let launched = (daemon.is_some(), probe.ok, probe.detail.clone());
    engine
        .lock()
        .unwrap()
        .set_cat_status(probe.ok, probe.detail);
    let cfg = RadioConfig {
        rig_model: settings.rig_model,
        ..RadioConfig::default()
    };
    let mut state = RadioLoop::new(applied, daemon, &cfg);
    state.after_the_launch_open(&rig, probe.ok);
    let said = || engine.lock().unwrap().snapshot().radio.cat_detail.clone();
    let mut backend = MockBackend::new();
    // Ticks half a second apart by the loop's clock, each reopen the real one, until `done` or
    // `ticks` run out.
    let mut now = 0.0f64;
    let mut run =
        |state: &mut RadioLoop, rig: &mut Rig, ticks: usize, done: &dyn Fn(&RadioLoop) -> bool| {
            for _ in 0..ticks {
                if done(state) {
                    return;
                }
                now += 500.0;
                let mut reopen_rig = |t: &Transport, coexist: bool| open_rig(t, coexist);
                state
                    .step(
                        &engine,
                        &mut backend,
                        rig,
                        &no_sinks(),
                        now,
                        &mut mock_reopen_audio(),
                        &mut reopen_rig,
                        &mut StationSinks::new(),
                    )
                    .unwrap();
            }
        };
    run(&mut state, &mut rig, 4, &|_| false);
    let waiting = (state.rigctld_proc.is_some(), state.cat_ok, said());
    let wf = FakeWfview::at(&addr, true);
    // The backoff is 10 s after the first ask again.
    run(&mut state, &mut rig, 40, &|s| s.cat_ok == Some(true));
    let shared = (
        state.rigctld_proc.is_some(),
        state.cat_ok,
        rig.control_addr().map(str::to_string),
        said(),
    );
    let keyed = || wf.radio.lock().unwrap().ptt;
    let ptt = [
        rig.ptt(true).is_ok(),
        keyed(),
        rig.ptt(false).is_ok(),
        keyed(),
    ];
    let keyed_with = wf.keyed_with();
    let r = wf.radio.lock().unwrap();
    assert_eq!(
        (
            launched,
            waiting,
            shared,
            ptt,
            keyed_with,
            r.unanswered.clone(),
            r.unmodelled.clone()
        ),
        (
            (false, Some(false), nothing_answering_at(&addr)),
            (false, Some(false), nothing_answering_at(&addr)),
            (
                false,
                Some(true),
                Some(addr.clone()),
                format!("Sharing the rigctld at {addr} — Connected — 14.074 MHz"),
            ),
            [true, true, true, false],
            ["T 0", "T 1", "T 0"].map(String::from).to_vec(),
            vec![],
            vec![],
        )
    );
}

/// Which launch opens the loop asks again on the reopen backoff: the one that found nothing at a
/// NET rigctl Network Address on this computer, and only that one. A link that opened, a verdict
/// that is not a failure, and each station the direct share leaves alone start as they did: no
/// verdict until CAT reads down.
#[test]
fn the_loop_asks_again_after_a_launch_that_found_nothing_at_the_network_address() {
    let net = |addr: &str, rigctld_port: u16| station(addr, rigctld_port).1;
    let mut broker = net("127.0.0.1:4532", 4534);
    broker.cat_broker = true;
    broker.cat_broker_port = 4532;
    let mut dummy = net("127.0.0.1:4533", 4534);
    dummy.rig_model = 1;
    let here = net("127.0.0.1:4533", 4534);
    let linked = || Rig::with_control(Some("127.0.0.1:4533".into()), PttMode::Cat);
    let rows = [
        (
            "nothing at the address",
            &here,
            Rig::vox(),
            Some(false),
            Some(false),
        ),
        (
            "shared, its first read failed",
            &here,
            linked(),
            Some(false),
            None,
        ),
        ("not a failure", &here, Rig::vox(), Some(true), None),
        ("no verdict", &here, Rig::vox(), None, None),
        (
            "rigctld TCP Port's own number",
            &net("127.0.0.1:4534", 4534),
            Rig::vox(),
            Some(false),
            None,
        ),
        (
            "another computer",
            &net("192.168.1.50:4533", 4534),
            Rig::vox(),
            Some(false),
            None,
        ),
        (
            "the CAT broker's port",
            &broker,
            Rig::vox(),
            Some(false),
            None,
        ),
        ("Rig Model Dummy", &dummy, Rig::vox(), Some(false), None),
    ];
    let seen: Vec<_> = rows
        .iter()
        .map(|(what, settings, rig, ok, _)| {
            let mut state = RadioLoop::new(
                Transport::from_settings(settings),
                None,
                &RadioConfig::default(),
            );
            state.after_the_launch_open(rig, *ok);
            (*what, state.cat_ok)
        })
        .collect();
    assert_eq!(
        seen,
        rows.iter()
            .map(|(what, .., want)| (*what, *want))
            .collect::<Vec<_>>()
    );
}

/// Test CAT asks a waiting Network Address again at once, ahead of the backoff: pressed while the
/// loop waits for the rigctld on this computer, it re-runs the open, which says what the address
/// answered, rather than probe a link that never opened ("the control channel didn't open — check
/// the rig model, serial port…", which is not this station's fault). The matching-port station,
/// which waits under its own message, keeps Test CAT's probe as it was.
#[test]
fn test_cat_asks_a_waiting_network_address_again_at_once() {
    let press = |addr: String, rigctld_port: u16| {
        let (engine, settings) = station(&addr, rigctld_port);
        let applied = Transport::from_settings(&settings);
        let (mut rig, daemon, probe) = open_rig(&applied, true);
        engine
            .lock()
            .unwrap()
            .set_cat_status(probe.ok, probe.detail);
        let cfg = RadioConfig {
            rig_model: settings.rig_model,
            ..RadioConfig::default()
        };
        let mut state = RadioLoop::new(applied, daemon, &cfg);
        state.after_the_launch_open(&rig, probe.ok);
        state.cat_reopen_at = 60_000.0; // the backoff is not due
        engine.lock().unwrap().request_cat_reprobe();
        let mut opens = 0;
        let mut reopen_rig = |t: &Transport, coexist: bool| {
            opens += 1;
            open_rig(t, coexist)
        };
        state
            .step(
                &engine,
                &mut MockBackend::new(),
                &mut rig,
                &no_sinks(),
                1_000.0,
                &mut mock_reopen_audio(),
                &mut reopen_rig,
                &mut StationSinks::new(),
            )
            .unwrap();
        let said = engine.lock().unwrap().snapshot().radio.cat_detail.clone();
        (opens, said)
    };
    let waiting = format!("127.0.0.1:{}", free_port());
    let matching = free_port();
    assert_eq!(
        [
            press(waiting.clone(), free_port()),
            press(format!("127.0.0.1:{matching}"), matching),
        ],
        [
            (1, nothing_answering_at(&waiting)),
            (
                0,
                "CAT rig configured, but the control channel didn't open — check the rig model, \
                 serial port, and that the CAT daemon (rigctld) could start (or a port conflict)."
                    .to_string()
            ),
        ]
    );
}
