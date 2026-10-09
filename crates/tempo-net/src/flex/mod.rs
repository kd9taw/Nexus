//! The native FlexRadio client's protocol core: the SmartSDR session, status decoding, typed
//! command encoding, ownership, the readback that proves an unkey, and the VITA-49 display
//! streams.
//!
//! **The app uses only its display side yet.** The shipped native panadapter
//! (`tempo_audio::flexspectrum`, still on the `crate::flexcat` session) assembles its FFT frames
//! with [`vita`] and reads its create reply and its pan's status with [`ownership`] and
//! [`status`]. A later change builds the daemon that runs [`session::Connection`] for a radio and
//! puts the engine's transmit gates in front of it; until then `crate::flexcat`,
//! `crate::flexvita` (the packet envelope, DAX audio, meters) and `crate::flexdisc` carry the rest.
//!
//! # Layers
//!
//! | Module | What it is |
//! |---|---|
//! | [`wire`] | the line codec: `V`, `H`, `R`, `S`, `M` lines parsed strictly; commands framed |
//! | [`kv`] | present-only, fail-closed field readers |
//! | [`status`] | the status decoders: a line to a typed delta |
//! | [`model`] | the status model: the radio as reported, with every object's owner |
//! | [`encode`] | the typed command encoder: ordinary commands, starts, stops |
//! | [`admission`] | the refuse-only Flex checks a start must pass; the only source of an admitted start |
//! | [`ptt_evidence`] | the readback that proves a transmission ended, per kind |
//! | [`ownership`] | ownership and recovery as pure policy functions |
//! | [`handshake`] | the connect sequence and GUI-client registration |
//! | [`keepalive`] | pings and the missed-reply count |
//! | [`reconnect`] | the reconnect ladder |
//! | [`session`] | one connection, from prologue to teardown, and the thread that runs it |
//! | [`vita`] | the VITA-49 display streams: FFT frames and waterfall tiles, decoded and assembled |
//! | [`streams`] | DAX audio: the refcounted DAX receive broker, the receive decoder, the transmit packet |
//!
//! # The transmit rule
//!
//! Models hold state, a separate encoder turns typed intents into wire text, and every transmit
//! check sits on the typed intent, never on rendered text (port plan §3.1):
//!
//! - A command that can key the radio ([`encode::TxStart`]) is rendered only from an
//!   [`admission::Admitted`], and only [`admission::admit`] makes one. The decoders, the model,
//!   the keepalive, the connect sequence, teardown and the reconnect ladder produce no
//!   `TxStart`; `session::tests::no_input_makes_the_session_originate_a_key` drives the session
//!   with random input in every state and checks that the only start ever written is the one the
//!   test asked for.
//! - Stops ([`encode::TxStop`]) are never gated: a stop asks only "is it ours?", and rigctld's
//!   `T 0` ends everything of ours ([`session::Session::end_ours`]).
//! - Keyed is set at the start attempt and cleared only by the readback of its kind; a reply to a
//!   stop is not RF cessation. An unconfirmed end is stopped again, the session closes and the
//!   operator is told. A tune the radio latches is ended by the session itself at its deadline.
//! - Each kind has a readback profile, but only `xmit`'s is confirmed on a radio, so admission
//!   refuses tune, ATU and CWX starts ([`admission::BENCHED`]) until a tester's bench confirms
//!   theirs.
//! - Where the transmitter takes its audio from (`stream create type=dax_tx`, `transmit set dax`)
//!   is its own typed kind ([`encode::TxAudio`]) with its own admission: never while anything is
//!   keyed, never beside another program's DAX transmit stream.
//! - No guard anywhere matches command text.
//!
//! # Provenance
//!
//! Most of this module is ported from AetherSDR (GPL-3.0) at [`UPSTREAM_COMMIT`]. [`PROVENANCE`]
//! lists each ported file, the upstream files it came from and its deliberate differences; every
//! ported file's header says the same, and the repo-root NOTICE names every upstream file. A test
//! holds the three together in both directions: a ported file missing from the table or NOTICE
//! fails it, and so does a table entry or NOTICE name with nothing ported behind it. `model`,
//! `admission` and `reconnect` are Nexus's own.

pub mod admission;
pub mod encode;
pub mod handshake;
pub mod keepalive;
pub mod kv;
pub mod model;
pub mod ownership;
pub mod ptt_evidence;
pub mod reconnect;
pub mod session;
pub mod status;
pub mod streams;
pub mod vita;
pub mod wire;

#[cfg(test)]
mod sim_tests;

/// The upstream repository.
pub const UPSTREAM: &str = "https://github.com/aethersdr/AetherSDR";

/// The upstream commit every ported file was taken from (2026-10-03).
pub const UPSTREAM_COMMIT: &str = "32fa50e4896a846a6970fa3f443bd49d667c139d";

/// One ported file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ported {
    /// The Nexus file, relative to this directory.
    pub file: &'static str,
    /// The upstream files translated or restructured into it, relative to the upstream root.
    pub upstream: &'static [&'static str],
    /// Upstream files read only for protocol facts (order, constants, wire text); no code taken.
    pub references: &'static [&'static str],
    /// The upstream tests translated with it.
    pub tests: &'static [&'static str],
    /// Nexus's deliberate differences, in short.
    pub differences: &'static str,
}

/// Every ported file. Each later take from upstream is its own commit naming the upstream commit,
/// and updates this table.
pub const PROVENANCE: &[Ported] = &[
    Ported {
        file: "wire.rs",
        upstream: &[
            "src/core/backends/flex/CommandParser.h",
            "src/core/backends/flex/CommandParser.cpp",
            "src/core/RadioMessageTypes.h",
        ],
        references: &[],
        tests: &[],
        differences: "strict numeric parsing; no trimming; a repeated key is ambiguous; framing \
                      takes only encoder output",
    },
    Ported {
        file: "kv.rs",
        upstream: &["src/core/backends/flex/FlexKvCarry.h"],
        references: &[],
        tests: &[],
        differences: "readers return values; a flag is 0 or 1; reals are finite; ids keep 0x",
    },
    Ported {
        file: "status.rs",
        upstream: &[
            "src/core/backends/flex/FlexBackend.h",
            "src/core/backends/flex/FlexBackend.cpp",
            "src/core/backends/SliceDelta.h",
            "src/core/backends/TransmitDelta.h",
            "src/core/backends/RadioDelta.h",
            "src/core/backends/MeterDef.h",
            "src/core/backends/ProfileDelta.h",
        ],
        references: &[],
        tests: &[
            "tests/aetherd_slice_decode_test.cpp",
            "tests/aetherd_radio_decode_test.cpp",
            "tests/aetherd_meter_decode_test.cpp",
            "tests/aetherd_pan_decode_test.cpp",
            "tests/aetherd_transmit_decode_test.cpp",
            "tests/aetherd_residual_decode_test.cpp",
        ],
        differences: "deltas returned, not signalled; routing done here; strict fields; typed \
                      pan bundles; owners carried; mox not decoded; interlock sampled per line",
    },
    Ported {
        file: "encode.rs",
        upstream: &["src/core/backends/flex/FlexBackend.cpp"],
        references: &["src/models/RadioModel.cpp", "src/models/CwxModel.cpp"],
        tests: &[],
        differences: "typed intents; starts render only from an admission; kind and target on \
                      every command; values validated; free text a safe token",
    },
    Ported {
        file: "session.rs",
        upstream: &[
            "src/core/backends/flex/RadioConnection.h",
            "src/core/backends/flex/RadioConnection.cpp",
            "src/core/backends/flex/FlexPttWireSession.h",
            "src/core/backends/flex/FlexPttWireSession.cpp",
        ],
        references: &[],
        tests: &[
            "tests/radio_connection_session_test.cpp",
            "tests/flex_ptt_wire_session_test.cpp",
        ],
        differences: "sans-I/O core on a std thread; typed kinds replace the text match on other \
                      writers; no demo, kernel RTT or WAN; evidence consumed when complete; a \
                      failed write ends the session; Nexus's own stops and deadlines per kind",
    },
    Ported {
        file: "ptt_evidence.rs",
        upstream: &[
            "src/core/backends/flex/FlexPttStopTracker.h",
            "src/core/backends/flex/FlexPttStopTracker.cpp",
        ],
        references: &[],
        tests: &["tests/flex_ptt_stop_tracker_test.cpp"],
        differences: "plain operation and stop ids replace the coordinator; consume() replaces \
                      the acknowledgment; no thread check; the timeout is a parameter; Nexus's \
                      own profiles for tune, CWX and ATU, the CWX clear, and an amplifier's \
                      reason on a keying report",
    },
    Ported {
        file: "ownership.rs",
        upstream: &[
            "src/models/RadioStatusOwnership.h",
            "src/core/StreamStatus.h",
            "src/models/DisplayInventoryPolicy.h",
            "src/models/SliceRecreatePolicy.h",
        ],
        references: &[],
        tests: &[
            "tests/radio_status_ownership_test.cpp",
            "tests/display_inventory_policy_test.cpp",
            "tests/slice_recreate_policy_test.cpp",
        ],
        differences: "strict handles, never 0 and never ours when malformed; owner zero never \
                      claimed; no transmit or remote-audio decisions",
    },
    Ported {
        file: "handshake.rs",
        upstream: &[
            "src/core/GuiClientRegistrationState.h",
            "src/core/UdpRegistrationPolicy.h",
            "src/core/GuiClientIdentityPolicy.h",
        ],
        references: &["src/models/RadioModel.cpp"],
        tests: &[
            "tests/gui_client_registration_state_test.cpp",
            "tests/gui_client_identity_policy_test.cpp",
            "tests/gui_client_registration_recovery_test.cpp",
        ],
        differences: "the sequence as a list of typed commands; no automation identity; every \
                      Unicode space separates",
    },
    Ported {
        file: "keepalive.rs",
        upstream: &["src/core/backends/flex/RadioConnection.cpp"],
        references: &["src/models/RadioModel.cpp"],
        tests: &[],
        differences: "misses counted per ping; each miss reported; no kernel RTT",
    },
    Ported {
        file: "vita.rs",
        upstream: &[
            "src/core/backends/flex/PanadapterStream.h",
            "src/core/backends/flex/PanadapterStream.cpp",
            "src/core/VitaBinCoverage.h",
            "src/core/VitaTileFrequency.h",
        ],
        references: &[
            "src/models/RadioModel.cpp",
            "docs/architecture/vita49-format.md",
        ],
        tests: &[
            "tests/panadapter_dbm_range_test.cpp",
            "tests/vita_tile_frequency_test.cpp",
        ],
        differences: "pure payload decoders split from the socket; rows kept, levels and dBm a \
                      separate step; three frames in flight; a completed frame takes nothing \
                      more; a misfit fragment displaces nothing; no dBm-range echo handshake",
    },
    Ported {
        file: "streams.rs",
        upstream: &[
            "src/core/backends/flex/PanadapterStream.h",
            "src/core/backends/flex/PanadapterStream.cpp",
        ],
        references: &["src/models/RadioModel.cpp", "src/core/AudioEngine.cpp"],
        tests: &[],
        differences: "a pure broker with time passed in, timers as due times voided by \
                      generation; actions returned, not signalled; holders are bit indices; \
                      release-all covers eight channels; a disconnect forgets stream ids too; \
                      stream id 0 is never a stream; the TX packet from protocol facts only",
    },
];

#[cfg(test)]
mod provenance_tests {
    //! The provenance table, the ported files' headers and NOTICE's AetherSDR entry, held together.
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    /// How a ported file's header starts its provenance paragraph.
    const MARKER: &str = "//! PORTED from AetherSDR (";
    /// NOTICE's entry for this port.
    const NOTICE_HEADING: &str = "### AetherSDR — the FlexRadio SmartSDR client core";

    /// One source file: its path relative to this directory, its leading `//!` block, all of it.
    struct Source {
        name: String,
        header: String,
        text: String,
    }

    fn is_ported(source: &Source) -> bool {
        source.header.lines().any(|l| l.starts_with(MARKER))
    }

    fn stem(path: &str) -> &str {
        let file = path.rsplit('/').next().unwrap_or(path);
        file.split('.').next().unwrap_or(file)
    }

    /// The NOTICE entry's text: from its heading to the next heading or rule.
    fn notice_entry(notice: &str) -> Option<String> {
        let start = notice.find(NOTICE_HEADING)?;
        let rest = &notice[start + NOTICE_HEADING.len()..];
        let end = rest
            .lines()
            .scan(0usize, |offset, line| {
                let at = *offset;
                *offset += line.len() + 1;
                Some((at, line))
            })
            .find(|(_, l)| l.starts_with("## ") || l.starts_with("### ") || l.starts_with("---"))
            .map_or(rest.len(), |(at, _)| at);
        Some(rest[..end].to_string())
    }

    /// Backticked names in CamelCase: the upstream files an entry names.
    fn named_classes(entry: &str) -> BTreeSet<String> {
        entry
            .split('`')
            .skip(1)
            .step_by(2)
            .filter(|t| {
                t.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                    && t.chars().all(|c| c.is_ascii_alphanumeric())
            })
            .map(str::to_string)
            .collect()
    }

    /// Everything wrong, as sentences. Empty means the three agree.
    fn check(sources: &[Source], table: &[Ported], notice: &str) -> Vec<String> {
        let mut problems = Vec::new();
        let listed: BTreeSet<&str> = table.iter().map(|p| p.file).collect();
        for s in sources.iter().filter(|s| is_ported(s)) {
            if !listed.contains(s.name.as_str()) {
                problems.push(format!(
                    "{} says it is ported but is not in PROVENANCE",
                    s.name
                ));
            }
        }
        let Some(entry) = notice_entry(notice) else {
            problems.push(format!("NOTICE has no `{NOTICE_HEADING}` entry"));
            return problems;
        };
        let short = &UPSTREAM_COMMIT[..8];
        if !entry.contains(short) {
            problems.push(format!("NOTICE's entry does not name commit {short}"));
        }
        if !entry.contains("crates/tempo-net/src/flex/") {
            problems.push("NOTICE's entry does not name crates/tempo-net/src/flex/".into());
        }
        let named = named_classes(&entry);
        let mut ported_stems = BTreeSet::new();
        for p in table {
            let Some(source) = sources.iter().find(|s| s.name == p.file) else {
                problems.push(format!("PROVENANCE names {}, which does not exist", p.file));
                continue;
            };
            if !is_ported(source) {
                problems.push(format!("{} has no ported-from header", p.file));
            }
            if !source.header.contains(UPSTREAM_COMMIT) {
                problems.push(format!("{}'s header does not name the commit", p.file));
            }
            for path in p.upstream.iter().chain(p.references) {
                if !source.header.contains(path) {
                    problems.push(format!("{}'s header does not name {path}", p.file));
                }
            }
            // A translated test is named where it was translated: in the file, or in its own
            // test module beside it.
            let tests_beside: String = sources
                .iter()
                .filter(|s| s.name.starts_with(&format!("{}/", stem(p.file))))
                .map(|s| s.text.as_str())
                .collect();
            for test in p.tests {
                let name = test.rsplit('/').next().unwrap_or(test);
                if !source.text.contains(name) && !tests_beside.contains(name) {
                    problems.push(format!(
                        "{} does not say where {name} was translated",
                        p.file
                    ));
                }
            }
            if p.differences.trim().is_empty() {
                problems.push(format!("{} lists no deliberate differences", p.file));
            }
            for path in p.upstream {
                let s = stem(path);
                ported_stems.insert(s.to_string());
                if !named.contains(s) {
                    problems.push(format!("NOTICE's entry does not name `{s}` ({})", p.file));
                }
            }
        }
        for name in named.difference(&ported_stems) {
            problems.push(format!(
                "NOTICE's entry names `{name}`, which nothing ports"
            ));
        }
        problems
    }

    fn read_sources(dir: &Path, prefix: &str, out: &mut Vec<Source>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .expect("the flex directory")
            .map(|e| e.expect("an entry").path())
            .collect();
        entries.sort();
        for path in entries {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if path.is_dir() {
                read_sources(&path, &format!("{prefix}{name}/"), out);
            } else if name.ends_with(".rs") {
                let text = std::fs::read_to_string(&path).expect("a source file");
                let header = text
                    .lines()
                    .take_while(|l| l.starts_with("//!"))
                    .collect::<Vec<_>>()
                    .join("\n");
                out.push(Source {
                    name: format!("{prefix}{name}"),
                    header,
                    text,
                });
            }
        }
    }

    #[test]
    fn every_ported_file_is_in_the_table_its_header_and_notice() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut sources = Vec::new();
        read_sources(&root.join("src/flex"), "", &mut sources);
        assert!(
            sources.iter().filter(|s| is_ported(s)).count() >= PROVENANCE.len(),
            "the scan found the ported files"
        );
        let notice = std::fs::read_to_string(root.join("../../NOTICE")).expect("the repo NOTICE");
        let problems = check(&sources, PROVENANCE, &notice);
        assert!(problems.is_empty(), "{problems:#?}");
    }

    /// A small consistent world for the checker's positive controls.
    fn world() -> (Vec<Source>, Vec<Ported>, String) {
        let header = |upstream: &str| {
            format!("//! A module.\n{MARKER}...) `{upstream}` at commit `{UPSTREAM_COMMIT}`.")
        };
        let sources = vec![
            Source {
                name: "a.rs".into(),
                header: header("src/A.h"),
                text: "mod tests; // a_test.cpp".into(),
            },
            Source {
                name: "own.rs".into(),
                header: "//! Nexus's own.".into(),
                text: String::new(),
            },
        ];
        let table = vec![Ported {
            file: "a.rs",
            upstream: &["src/A.h"],
            references: &[],
            tests: &["tests/a_test.cpp"],
            differences: "some",
        }];
        let notice = format!(
            "## Upstream lineage\n\n{NOTICE_HEADING}\nPorted from `32fa50e4` into \
             `crates/tempo-net/src/flex/`: `A`.\n\n### Next entry\nNames `B`.\n"
        );
        (sources, table, notice)
    }

    #[test]
    fn the_checker_accepts_a_consistent_world() {
        let (sources, table, notice) = world();
        assert_eq!(check(&sources, &table, &notice), Vec::<String>::new());
    }

    #[test]
    fn the_checker_fires_on_each_kind_of_gap() {
        // Each change below must be caught: the positive controls for the test above.
        let (mut sources, table, notice) = world();
        sources[1].header.push_str(&format!("\n{MARKER}...)"));
        let p = check(&sources, &table, &notice);
        assert!(
            p.iter().any(|m| m.contains("own.rs says it is ported")),
            "{p:?}"
        );

        let (sources, mut table, notice) = world();
        table.push(Ported {
            file: "gone.rs",
            upstream: &["src/A.h"],
            ..table[0]
        });
        let p = check(&sources, &table, &notice);
        assert!(
            p.iter()
                .any(|m| m.contains("gone.rs, which does not exist")),
            "{p:?}"
        );

        let (sources, mut table, notice) = world();
        table[0].upstream = &["src/A.h", "src/Extra.cpp"];
        let p = check(&sources, &table, &notice);
        assert!(
            p.iter().any(|m| m.contains("does not name src/Extra.cpp")),
            "{p:?}"
        );
        assert!(
            p.iter().any(|m| m.contains("does not name `Extra`")),
            "{p:?}"
        );

        let (sources, table, notice) = world();
        let p = check(&sources, &table, &notice.replace("`A`", "`A`, `Lingering`"));
        assert!(
            p.iter()
                .any(|m| m.contains("`Lingering`, which nothing ports")),
            "{p:?}"
        );

        let (sources, table, notice) = world();
        let p = check(&sources, &table, &notice.replace("`A`", "A"));
        assert!(p.iter().any(|m| m.contains("does not name `A`")), "{p:?}");

        let (sources, table, notice) = world();
        let p = check(&sources, &table, &notice.replace("32fa50e4", "deadbeef"));
        assert!(
            p.iter().any(|m| m.contains("does not name commit")),
            "{p:?}"
        );

        let (mut sources, table, notice) = world();
        sources[0].text.clear();
        let p = check(&sources, &table, &notice);
        assert!(p.iter().any(|m| m.contains("a_test.cpp")), "{p:?}");

        let (sources, table, _) = world();
        let p = check(&sources, &table, "no entry here");
        assert!(p.iter().any(|m| m.contains("has no")), "{p:?}");

        // A name in the NEXT entry is not this entry's: the section ends at the next heading.
        let (sources, table, notice) = world();
        assert!(!check(&sources, &table, &notice)
            .iter()
            .any(|m| m.contains("`B`")));
    }
}
