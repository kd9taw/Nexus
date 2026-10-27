//! The simulator's own reading of TCI's text grammar (spec §3.1), from the server's side.
//!
//! Written from the spec for the simulator, independently of Nexus's client codec, so that a codec
//! bug on one side is not mirrored by the other (two readings of one spec cross-check).
//!
//! **The grammar.** A command is `name:arg1,arg2,…;`, or `name;` when it has no arguments. The
//! reserved characters are `:` `,` `;`. Case does not matter. Several commands may share one
//! WebSocket text message. An invalid command is ignored.
//!
//! **What this reader does where the spec is silent:**
//! - Whitespace around a command, its name and each argument is trimmed. Servers write it
//!   (piHPSDR sends `audio_stream_channels: 2`), so a client may too.
//! - Text after the last `;` of a message is an unterminated command. It is reported and dropped,
//!   never joined to the next message: joined, a command cut at a message boundary and not
//!   completed by the next message would be read as a different command.
//! - `name:;` has one empty argument, which fails every arity check. `name;` has none.
//! - A name is ASCII letters, digits and `_`; anything else is reported and dropped.
//! - Bounds: a message over [`MAX_MESSAGE`] bytes, a command past the [`MAX_COMMANDS`]th in one
//!   message, more than [`MAX_ARGS`] arguments, or an argument over [`MAX_ARG`] bytes is reported
//!   and dropped. Nothing here panics on any input; a test feeds it random text.
//!
//! **CW text** (spec §3.2.1). A macro cannot carry the reserved characters, so a client writes `^`
//! for `:`, `~` for `,` and `*` for `;`, and the server turns them back: [`cw_text`].

use std::fmt::Display;

/// The longest client text message read, in bytes. Longer ones are reported and not read.
pub const MAX_MESSAGE: usize = 64 * 1024;
/// The most commands read from one message.
pub const MAX_COMMANDS: usize = 256;
/// The most arguments one command may have.
pub const MAX_ARGS: usize = 32;
/// The longest argument, in bytes (a CW macro's text is the long one).
pub const MAX_ARG: usize = 4096;

/// How much of a malformed command a [`Flaw`] keeps, in characters.
const KEEP: usize = 120;

/// One well-formed command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// The name, lower-cased.
    pub name: String,
    /// The arguments as sent, each trimmed.
    pub args: Vec<String>,
    /// The command as it arrived, trimmed, without its `;`.
    pub text: String,
}

impl Command {
    /// Argument `i`, or `""` when there are fewer.
    pub fn arg(&self, i: usize) -> &str {
        self.args.get(i).map_or("", String::as_str)
    }
}

/// Something in a message that is not a well-formed command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flaw {
    /// The message is longer than [`MAX_MESSAGE`]; nothing in it was read.
    MessageTooLong(usize),
    /// The message holds more than [`MAX_COMMANDS`] commands; the rest were not read.
    TooManyCommands,
    /// Text after the last `;`: a command with no terminator.
    Unterminated(String),
    /// A command whose name is empty or not made of letters, digits and `_`.
    BadName(String),
    /// A command with more than [`MAX_ARGS`] arguments.
    TooManyArgs(String),
    /// A command with an argument longer than [`MAX_ARG`] bytes.
    ArgTooLong(String),
}

/// What one text message held.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    pub commands: Vec<Command>,
    pub flaws: Vec<Flaw>,
}

/// Read every command in one text message.
pub fn parse(message: &str) -> Parsed {
    let mut out = Parsed::default();
    if message.len() > MAX_MESSAGE {
        out.flaws.push(Flaw::MessageTooLong(message.len()));
        return out;
    }
    let mut rest = message;
    while let Some(end) = rest.find(';') {
        let piece = rest[..end].trim();
        rest = &rest[end + 1..];
        if piece.is_empty() {
            continue;
        }
        if out.commands.len() >= MAX_COMMANDS {
            out.flaws.push(Flaw::TooManyCommands);
            return out;
        }
        match command(piece) {
            Ok(c) => out.commands.push(c),
            Err(flaw) => out.flaws.push(flaw),
        }
    }
    let tail = rest.trim();
    if !tail.is_empty() {
        out.flaws.push(Flaw::Unterminated(keep(tail)));
    }
    out
}

fn command(piece: &str) -> Result<Command, Flaw> {
    let (name, args) = match piece.split_once(':') {
        Some((name, args)) => (name.trim(), Some(args)),
        None => (piece, None),
    };
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(Flaw::BadName(keep(piece)));
    }
    let mut parsed = Vec::new();
    if let Some(args) = args {
        for arg in args.splitn(MAX_ARGS + 1, ',') {
            if parsed.len() == MAX_ARGS {
                return Err(Flaw::TooManyArgs(keep(piece)));
            }
            let arg = arg.trim();
            if arg.len() > MAX_ARG {
                return Err(Flaw::ArgTooLong(keep(piece)));
            }
            parsed.push(arg.to_string());
        }
    }
    Ok(Command {
        name: name.to_ascii_lowercase(),
        args: parsed,
        text: piece.to_string(),
    })
}

/// The first [`KEEP`] characters of `text`, for a report.
fn keep(text: &str) -> String {
    match text.char_indices().nth(KEEP) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_string(),
    }
}

/// A command as the server writes it: `name:a,b,c;`, or `name;` with no arguments.
pub fn notice<I, T>(name: &str, args: I) -> String
where
    I: IntoIterator<Item = T>,
    T: Display,
{
    let args: Vec<String> = args.into_iter().map(|a| a.to_string()).collect();
    if args.is_empty() {
        format!("{name};")
    } else {
        format!("{name}:{};", args.join(","))
    }
}

/// `true` or `false`, in any case. Anything else is not a TCI boolean.
pub fn boolean(text: &str) -> Option<bool> {
    if text.eq_ignore_ascii_case("true") {
        Some(true)
    } else if text.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// How a boolean is written on the wire.
pub fn flag(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

/// A CW macro's text with the protocol's stand-ins turned back (spec §3.2.1): `^` is `:`, `~` is
/// `,` and `*` is `;`.
pub fn cw_text(raw: &str) -> String {
    raw.chars()
        .map(|c| match c {
            '^' => ':',
            '~' => ',',
            '*' => ';',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(p: &Parsed) -> Vec<&str> {
        p.commands.iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn every_example_in_the_spec_reads_as_its_command() {
        // A sample of the spec's own examples (§4), one per argument shape.
        for (text, name, args) in [
            ("VFO:0,1,7100000;", "vfo", vec!["0", "1", "7100000"]),
            ("VFO:0,1;", "vfo", vec!["0", "1"]),
            ("TRX:0,true,tci;", "trx", vec!["0", "true", "tci"]),
            ("START;", "start", vec![]),
            ("IF:0,1,-17550;", "if", vec!["0", "1", "-17550"]),
            ("MODULATION:1,NFM;", "modulation", vec!["1", "NFM"]),
            ("CW_MACROS_SPEED;", "cw_macros_speed", vec![]),
            (
                "SPOT:RN6LHF,CW,7100000,16711680,ANY_TEXT;",
                "spot",
                vec!["RN6LHF", "CW", "7100000", "16711680", "ANY_TEXT"],
            ),
            ("RX_SENSORS:0, -71.5;", "rx_sensors", vec!["0", "-71.5"]),
            (
                "cw_msg:0,TU,RA6LH$2,599 004;",
                "cw_msg",
                vec!["0", "TU", "RA6LH$2", "599 004"],
            ),
        ] {
            let p = parse(text);
            assert_eq!(p.flaws, vec![], "{text}");
            assert_eq!(p.commands.len(), 1, "{text}");
            assert_eq!(p.commands[0].name, name, "{text}");
            assert_eq!(p.commands[0].args, args, "{text}");
        }
    }

    #[test]
    fn several_commands_share_a_message_and_empty_ones_are_skipped() {
        let p = parse("vfo:0,0,7074000;;  modulation:0,DIGU ;\r\nready;");
        assert_eq!(names(&p), ["vfo", "modulation", "ready"]);
        assert_eq!(p.commands[1].args, ["0", "DIGU"]);
        assert!(p.flaws.is_empty());
    }

    #[test]
    fn an_unterminated_tail_is_reported_and_never_read_as_a_command() {
        // piHPSDR's burst carries two lines with no terminator.
        let p = parse("audio_stream_channels: 2");
        assert!(p.commands.is_empty());
        assert_eq!(
            p.flaws,
            [Flaw::Unterminated("audio_stream_channels: 2".into())]
        );
        let p = parse("trx:0,true;trx:0,fal");
        assert_eq!(names(&p), ["trx"]);
        assert_eq!(p.flaws, [Flaw::Unterminated("trx:0,fal".into())]);
    }

    #[test]
    fn malformed_names_and_bounds_are_flaws_not_commands() {
        let p = parse(":0,1;vf o:1;ünï:1;x-y;ok;");
        assert_eq!(names(&p), ["ok"]);
        assert_eq!(p.flaws.len(), 4, "{:?}", p.flaws);
        let many = format!("spot:{};", vec!["a"; MAX_ARGS + 1].join(","));
        assert!(matches!(parse(&many).flaws[..], [Flaw::TooManyArgs(_)]));
        let exactly = format!("spot:{};", vec!["a"; MAX_ARGS].join(","));
        assert_eq!(parse(&exactly).commands[0].args.len(), MAX_ARGS);
        let long = format!("cw_macros:0,{};", "E".repeat(MAX_ARG + 1));
        assert!(matches!(parse(&long).flaws[..], [Flaw::ArgTooLong(_)]));
        let huge = "x;".repeat(MAX_MESSAGE);
        assert_eq!(parse(&huge).flaws, [Flaw::MessageTooLong(huge.len())]);
        let crowded = "x;".repeat(MAX_COMMANDS + 5);
        let p = parse(&crowded);
        assert_eq!(p.commands.len(), MAX_COMMANDS);
        assert_eq!(p.flaws, [Flaw::TooManyCommands]);
    }

    #[test]
    fn an_empty_argument_list_differs_from_none() {
        assert_eq!(parse("drive;").commands[0].args, Vec::<String>::new());
        assert_eq!(parse("drive:;").commands[0].args, [""]);
    }

    #[test]
    fn random_text_never_panics_and_every_command_read_is_well_formed() {
        // A deterministic xorshift over an alphabet weighted towards the grammar's own characters,
        // plus arbitrary unicode: 20 000 messages.
        let alphabet: Vec<char> = "abcdefz_09:;,, ;;::\t\r\n^~*-.ü€\u{0}\u{7f}TRUEfalse"
            .chars()
            .collect();
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..20_000 {
            let len = (next() % 80) as usize;
            let text: String = (0..len)
                .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
                .collect();
            for c in parse(&text).commands {
                assert!(!c.name.is_empty());
                assert!(c
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_'));
                assert!(!c.text.contains(';'));
                assert!(c.args.len() <= MAX_ARGS);
            }
        }
    }

    #[test]
    fn notices_and_cw_text_follow_the_spec() {
        assert_eq!(notice("ready", Vec::<u8>::new()), "ready;");
        assert_eq!(notice("vfo", [0, 1, 7_100_000]), "vfo:0,1,7100000;");
        assert_eq!(boolean("TRUE"), Some(true));
        assert_eq!(boolean("False"), Some(false));
        assert_eq!(boolean("1"), None);
        assert_eq!(cw_text("TU^ 5NN~ 73*"), "TU: 5NN, 73;");
    }
}
