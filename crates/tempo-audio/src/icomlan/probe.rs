//! What the radio's own menu says, READ once per connect: its time-out timer, and where its
//! transmit audio comes from (MOD Input). Nothing here writes a menu item: each read is
//! `1A 05 <item>` with no data byte ([`commands::read_menu_item`] cannot carry one).
//!
//! Nothing transmits over the network connection yet, so both findings are reported, never
//! acted on. They are what a later transmit over the network will ask of the radio:
//! - **The time-out timer** is the radio's own backstop for a transmitter nobody can unkey. OFF is
//!   reported with the menu that turns it on.
//! - **MOD Input** says which source an over takes its audio from. Over the network it must be
//!   the network ([`network_tx_audio_refusal`]); a later Beta will refuse an over that would take
//!   its audio from somewhere else.
//!
//! A read the radio refuses, or that times out, is "unknown", and is said so.

use crate::civ::commands::{self, IcomModel, MenuItem, ModInput};
use crate::civ::engine::{CivHandle, Expect};

/// The radio's time-out timer, as read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tot {
    Off,
    /// On, after this many minutes.
    Minutes(u8),
    /// Not read: refused, timed out, or a value the guide does not list.
    Unknown,
}

impl Tot {
    /// The guides' values: `00` OFF, then 3, 5, 10, 20 and 30 minutes.
    pub fn from_value(v: u8) -> Tot {
        match v {
            0 => Tot::Off,
            1 => Tot::Minutes(3),
            2 => Tot::Minutes(5),
            3 => Tot::Minutes(10),
            4 => Tot::Minutes(20),
            5 => Tot::Minutes(30),
            _ => Tot::Unknown,
        }
    }
}

/// What was read at connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Findings {
    pub tot: Tot,
    /// DATA OFF MOD's value, when read.
    pub data_off_mod: Option<u8>,
    /// The value of the DATA MOD item the operator's data mode selects, when read.
    pub data_mod: Option<u8>,
}

/// One `1A 05` read; `None` when the radio refused it, it timed out, or the reply named another
/// item.
fn read_item(h: &CivHandle, addr: u8, item: MenuItem) -> Option<u8> {
    let f = h
        .transact(
            commands::read_menu_item(addr, item),
            Expect::Reply {
                cmd: 0x1A,
                sub: Some(0x05),
            },
        )
        .ok()?;
    commands::parse_menu_item(&f, item)
}

/// The DATA MOD item `data_mode` (1–3) selects: DATA1–DATA3 where the radio has three, its one
/// DATA MOD item where it has one.
pub fn data_mod_item(mi: &ModInput, data_mode: u8) -> MenuItem {
    let n = usize::from(data_mode.max(1)) - 1;
    mi.data[n.min(mi.data.len() - 1)]
}

/// Reads the findings from the radio at `addr`.
pub fn read(h: &CivHandle, addr: u8, model: IcomModel, data_mode: u8) -> Findings {
    let tot = commands::time_out_timer_item(model)
        .and_then(|item| read_item(h, addr, item))
        .map_or(Tot::Unknown, Tot::from_value);
    let (data_off_mod, data_mod) = match commands::mod_input(model) {
        Some(mi) => (
            read_item(h, addr, mi.data_off),
            read_item(h, addr, data_mod_item(&mi, data_mode)),
        ),
        None => (None, None),
    };
    Findings {
        tot,
        data_off_mod,
        data_mod,
    }
}

/// The guide's name for the radio's time-out timer item.
fn tot_name(model: IcomModel) -> &'static str {
    match model {
        IcomModel::Ic7610 | IcomModel::Ic7760 | IcomModel::Ic7300Mk2 => "Time-Out Timer (CI-V)",
        IcomModel::Ic9700 | IcomModel::Ic705 | IcomModel::Ic905 | IcomModel::Ic7300 => {
            "Time-Out Timer"
        }
    }
}

/// The name of the DATA MOD item `data_mode` selects on `model`: "DATA1 MOD" where the radio has
/// three, "DATA MOD" where it has one.
pub fn data_mod_name(model: IcomModel, data_mode: u8) -> &'static str {
    let three = commands::mod_input(model).is_some_and(|mi| mi.data.len() == 3);
    match (three, data_mode) {
        (true, 2) => "DATA2 MOD",
        (true, 3) => "DATA3 MOD",
        (true, _) => "DATA1 MOD",
        (false, _) => "DATA MOD",
    }
}

/// What a MOD Input value means on `model`, as its guide lists them.
pub fn mod_source_name(model: IcomModel, value: u8) -> Option<&'static str> {
    const MIC_ACC_USB_LAN: &[&str] = &["MIC", "ACC", "MIC, ACC", "USB", "MIC, USB", "LAN"];
    let names: &[&str] = match model {
        // A7380-7EX-4 PDF pp. 6–7; A7508-3EX-4 PDF p. 8.
        IcomModel::Ic7610 | IcomModel::Ic9700 => MIC_ACC_USB_LAN,
        // A7560-8EX-6 PDF p. 9.
        IcomModel::Ic705 => &["MIC", "USB", "MIC, USB", "WLAN"],
        // A7711-9EX-2 PDF p. 9.
        IcomModel::Ic905 => &["MIC", "USB", "MIC, USB", "LAN"],
        // A7788-8EX-2 PDF p. 9.
        IcomModel::Ic7760 => &[
            "MIC",
            "USB",
            "LINE-IN",
            "ACC",
            "MIC, USB",
            "MIC, LINE-IN",
            "MIC, ACC",
            "MIC, USB, ACC",
            "MIC, LINE-IN, ACC",
            "LAN",
        ],
        // The IC-7300MK2's reference, rev 0, PDF p. 9.
        IcomModel::Ic7300Mk2 => &["MIC", "USB", "ACC", "MIC, USB", "MIC, ACC", "LAN"],
        IcomModel::Ic7300 => &[],
    };
    names.get(usize::from(value)).copied()
}

/// The findings as the operator reads them in Rig & CAT and Test CAT.
pub fn summary(model: IcomModel, data_mode: u8, f: &Findings) -> String {
    let tot = match f.tot {
        Tot::Off => format!(
            "The radio's {} is OFF. Transmitting over the network, when a later Beta offers it, \
             will ask for it to be on: MENU › SET › Function.",
            tot_name(model)
        ),
        Tot::Minutes(m) => format!("{}: {m} min.", tot_name(model)),
        Tot::Unknown => format!(
            "{}: unknown (the radio did not answer the read).",
            tot_name(model)
        ),
    };
    let source = |v: Option<u8>| match v {
        Some(v) => {
            mod_source_name(model, v).map_or_else(|| format!("value {v:02X}"), str::to_string)
        }
        None => "unknown".to_string(),
    };
    format!(
        "{tot} DATA OFF MOD: {}. {}: {}.",
        source(f.data_off_mod),
        data_mod_name(model, data_mode),
        source(f.data_mod)
    )
}

/// What an over is, for where it takes its audio from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverClass {
    /// A soundcard digital over: the radio in a DATA mode, audio from DATA *n* MOD.
    Digital,
    /// A voice over in a plain mode: audio from DATA OFF MOD.
    Phone,
    /// A keyed carrier: no audio source at all.
    Cw,
}

/// Why an over of `class` would take its audio from somewhere other than the network, naming the
/// menu that fixes it; `None` when it would take it from the network (or takes no audio).
///
/// Built and tested now; nothing transmits over the network yet, so it only feeds the status line.
/// A later transmit over the network refuses with it, before PTT.
pub fn network_tx_audio_refusal(
    model: IcomModel,
    class: OverClass,
    data_mode: u8,
    f: &Findings,
) -> Option<String> {
    let mi = commands::mod_input(model)?;
    let (item, value) = match class {
        OverClass::Cw => return None,
        OverClass::Digital => (data_mod_name(model, data_mode), f.data_mod),
        OverClass::Phone => ("DATA OFF MOD", f.data_off_mod),
    };
    let lan = mod_source_name(model, mi.lan).unwrap_or("LAN");
    match value {
        Some(v) if v == mi.lan => None,
        Some(v) => Some(format!(
            "The radio takes this over's audio from {} ({item}), not from the network: set MENU › \
             SET › Connectors › MOD Input › {item} = {lan}",
            mod_source_name(model, v).unwrap_or("another source")
        )),
        None => Some(format!(
            "Nexus could not read the radio's {item}, so it cannot tell where this over's audio \
             would come from"
        )),
    }
}
