//! The Icom network (LAN / Wi-Fi) client's protocol core: the packets and the login passcode,
//! the capabilities reply, per-socket sequence tracking and the session, for the six Icoms with a
//! built-in network server (IC-7610, IC-9700, IC-705, IC-905, IC-7760, IC-7300MK2).
//!
//! **The app does not use it yet.** A later change builds the daemon that runs a session for a
//! radio and carries Nexus's native CI-V engine over it. This module knows nothing of the app.
//!
//! # Layers
//!
//! | Module | What it is |
//! |---|---|
//! | [`wire`] | every datagram, built and parsed strictly as typed packets; the login passcode |
//! | [`caps`] | the capabilities reply, choosing a radio, the rate bitmaps, the six network models |
//! | [`seq`] | the replay buffer and the receive gap tracker |
//! | [`session`] | one session, from the first probe to the last disconnect, with no I/O of its own |
//! | [`conf`] | the session's typed configuration; the password as a [`conf::Secret`] |
//! | [`reconnect`] | what to do when a session ends: the retry ladder |
//! | `sim` (tests only) | a simulated radio, a fake-clock world, and the radio on loopback sockets |
//!
//! # Nothing here transmits
//!
//! This core carries CAT and the scope only.
//!
//! - The session never originates a CI-V command. The only CI-V packets it sends carry a frame
//!   its owner handed to [`session::Session::send_civ`], or are that very packet again when the
//!   radio asks for its sequence; everything else it sends is control, ping, idle, retransmit,
//!   open/close and the handshake's requests. A property test
//!   (`no_input_makes_the_session_send_a_civ_command`) drives a session with random input in
//!   every state and reads back what reaches the wire.
//! - The connection-info request always carries transmit-enable 0: [`wire::connection_info`] has
//!   no field for it, so no transmit audio path is ever reserved.
//! - No audio stream is started, and audio packets are not decoded.
//! - Keying over the network is refused before anything reaches this module. Transmit over the
//!   network is a later design with its own review and bench; nothing here anticipates it.
//!
//! # Provenance
//!
//! Most of this module is ported from Hamlib's Icom network backend (pull request #2178, at
//! [`UPSTREAM_COMMIT`]), whose files are LGPL-2.1-or-later and are converted to the GPL in the
//! copies here. [`PROVENANCE`] lists each ported file, the upstream files it came from, the ones
//! read for facts only, the upstream tests translated with it, its deliberate differences and the
//! git blob id of every upstream file at the pin. Every ported file's header says the same and
//! carries the converted upstream notice, and the repo-root NOTICE names every upstream file. A
//! test holds the three together in both directions, and checks the README credit. `conf` and
//! `reconnect` are Nexus's own.

pub mod caps;
pub mod conf;
pub mod reconnect;
pub mod seq;
pub mod session;
pub mod wire;

#[cfg(test)]
mod sim;

/// The upstream repository.
pub const UPSTREAM: &str = "https://github.com/Hamlib/Hamlib";

/// The upstream pull request the port is taken from (open when taken).
pub const UPSTREAM_PR: u32 = 2178;

/// The upstream commit every ported file was taken from (2026-10-07).
pub const UPSTREAM_COMMIT: &str = "2e3e4a6add3bd806e828d035200777608d2bdbd5";

/// One ported file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ported {
    /// The Nexus file, relative to this directory.
    pub file: &'static str,
    /// The upstream files translated or restructured into it, relative to the upstream root.
    pub upstream: &'static [&'static str],
    /// Upstream files read only for protocol facts (layouts, constants, names); no code taken.
    pub references: &'static [&'static str],
    /// The upstream tests translated with it.
    pub tests: &'static [&'static str],
    /// Nexus's deliberate differences, in short.
    pub differences: &'static str,
    /// The git blob id at [`UPSTREAM_COMMIT`] of every upstream, reference and test path above,
    /// so a re-diff can show this is exactly what was ported from.
    pub blobs: &'static [(&'static str, &'static str)],
}

/// Every ported file. Each later take from upstream is its own commit naming the upstream commit,
/// and updates this table and the blob ids together.
pub const PROVENANCE: &[Ported] = &[
    Ported {
        file: "wire.rs",
        upstream: &["rigs/icom/network_proto.c", "rigs/icom/network_proto.h"],
        references: &[],
        tests: &["test/test_icom_network_proto.c"],
        differences:
            "typed packets; strict lengths, types, opcodes and flags; retransmit ranges; \
                      the cipher refuses what it cannot carry; transmit-enable fixed at 0; no audio",
        blobs: &[
            (
                "rigs/icom/network_proto.c",
                "06c29d31e21fd2fcd6c571fe5b230470d09a7c71",
            ),
            (
                "rigs/icom/network_proto.h",
                "1ad33d4a8ddeac884c162efea0ee623ab5744a4e",
            ),
            (
                "test/test_icom_network_proto.c",
                "ba09a98578ea17b3fd1dfa12ff6ee0b6475856a7",
            ),
        ],
    },
    Ported {
        file: "caps.rs",
        upstream: &["rigs/icom/network_proto.c", "rigs/icom/network_proto.h"],
        references: &[
            "rigs/icom/ICOM.md",
            "rigs/icom/ic7610net.c",
            "rigs/icom/ic9700net.c",
            "rigs/icom/ic705net.c",
            "rigs/icom/ic905net.c",
            "rigs/icom/ic7760net.c",
            "rigs/icom/ic7300mk2net.c",
        ],
        tests: &["test/test_icom_network_proto.c"],
        differences: "a count that disagrees with the length is an error; no eight-radio limit; \
                      the rate list is a value; typed link and models",
        blobs: &[
            (
                "rigs/icom/network_proto.c",
                "06c29d31e21fd2fcd6c571fe5b230470d09a7c71",
            ),
            (
                "rigs/icom/network_proto.h",
                "1ad33d4a8ddeac884c162efea0ee623ab5744a4e",
            ),
            (
                "rigs/icom/ICOM.md",
                "08793f6ab6ae9880622acb7f27e6b7c58af8c236",
            ),
            (
                "rigs/icom/ic7610net.c",
                "6f0a9dacdf7438a92df4b428e89cf98d9cc4d5bc",
            ),
            (
                "rigs/icom/ic9700net.c",
                "71e43306f5fd19a9768ca614fc7f8de9dead51b8",
            ),
            (
                "rigs/icom/ic705net.c",
                "20b336ab979844138cf81788be8d2d0cd22ac97b",
            ),
            (
                "rigs/icom/ic905net.c",
                "8fe0b2322ec039c71cfa7a1f5a6078650f3d6b98",
            ),
            (
                "rigs/icom/ic7760net.c",
                "9e450d0a8edba3ddb7b340e3c58043f3c5cfcef7",
            ),
            (
                "rigs/icom/ic7300mk2net.c",
                "110f66c32dceb51f52f46c379bae4be9f088f4c9",
            ),
            (
                "test/test_icom_network_proto.c",
                "ba09a98578ea17b3fd1dfa12ff6ee0b6475856a7",
            ),
        ],
    },
    Ported {
        file: "seq.rs",
        upstream: &["rigs/icom/network_seqbuf.c", "rigs/icom/network_seqbuf.h"],
        references: &[],
        tests: &["test/test_icom_network_seqbuf.c"],
        differences: "unsigned milliseconds with an explicit never-asked state; packets kept at \
                      their own length; a typed error for one too large; private state",
        blobs: &[
            (
                "rigs/icom/network_seqbuf.c",
                "26729847a9723d48184ac3bba804c04471d15bae",
            ),
            (
                "rigs/icom/network_seqbuf.h",
                "c8aeab8747c57298b10a632356e364a78fe0201a",
            ),
            (
                "test/test_icom_network_seqbuf.c",
                "6cd79edc18cf16b2ab5444641061707e9632ac8e",
            ),
        ],
    },
    Ported {
        file: "session.rs",
        upstream: &["rigs/icom/network_session.c", "rigs/icom/network_session.h"],
        references: &[
            "rigs/icom/ICOM.md",
            "rigs/icom/network_conf.c",
            "rigs/icom/icom_network.c",
        ],
        tests: &[
            "test/test_icom_network_session.c",
            "test/test_icom_network_reconnect.c",
            "test/test_icom_network_conf.c",
        ],
        differences: "one owner with time passed in, returning actions; no reconnect inside; \
                      liveness fixed at 5000 ms; transmit-enable 0 and no audio; frames never \
                      truncated; a frame refused before a sequence is used; one advertised radio \
                      with another name is a warning; the control replay buffer purged at 10 s",
        blobs: &[
            (
                "rigs/icom/network_session.c",
                "eb5178896e494592f23cf33f7750e49068d5d502",
            ),
            (
                "rigs/icom/network_session.h",
                "3e25eda753ad5890b31ebda6ae5faeefbe2c1206",
            ),
            (
                "rigs/icom/ICOM.md",
                "08793f6ab6ae9880622acb7f27e6b7c58af8c236",
            ),
            (
                "rigs/icom/network_conf.c",
                "543cca9d48c62cefa3626885a7d891b0de291a96",
            ),
            (
                "rigs/icom/icom_network.c",
                "9172b8c802af09735d714eb42043388da06e043b",
            ),
            (
                "test/test_icom_network_session.c",
                "b55bf19eea5c3ddbfbe0630cb46de71bcd8504e8",
            ),
            (
                "test/test_icom_network_reconnect.c",
                "0de39e963e04572507ca74f92b30ddadb18363ae",
            ),
            (
                "test/test_icom_network_conf.c",
                "a37af74efc1e3d4ed3f410da60fa3ae80d9b6553",
            ),
        ],
    },
    Ported {
        file: "session/tests.rs",
        upstream: &[
            "test/test_icom_network_session.c",
            "test/test_icom_network_reconnect.c",
            "test/test_icom_network_conf.c",
        ],
        references: &[],
        tests: &[],
        differences: "a fake clock in place of sleeps; expectations follow the session's own \
                      differences; the reconnect cases against Nexus's policy; typed configuration",
        blobs: &[
            (
                "test/test_icom_network_session.c",
                "b55bf19eea5c3ddbfbe0630cb46de71bcd8504e8",
            ),
            (
                "test/test_icom_network_reconnect.c",
                "0de39e963e04572507ca74f92b30ddadb18363ae",
            ),
            (
                "test/test_icom_network_conf.c",
                "a37af74efc1e3d4ed3f410da60fa3ae80d9b6553",
            ),
        ],
    },
    Ported {
        file: "sim.rs",
        upstream: &["test/icom_network_mock.c", "test/icom_network_mock.h"],
        references: &[],
        tests: &[],
        differences: "a pure radio value with a thin socket wrapper; a fake-clock world; a login \
                      it can refuse; a responder the test supplies; no spectrum or audio script",
        blobs: &[
            (
                "test/icom_network_mock.c",
                "fa7107ba1a8c6f5f2e3d104e6cce83bb27beb71e",
            ),
            (
                "test/icom_network_mock.h",
                "744cccb9ac189e34ddba386e194e6c023227bf46",
            ),
        ],
    },
];

#[cfg(test)]
mod provenance_tests {
    //! The provenance table, the ported files' headers, NOTICE's Hamlib entry and the README
    //! credit, held together. Every check yields a sentence and the test fails when any does; each
    //! check is also driven red by a positive control on a small consistent world.
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::Path;

    /// How a ported file's header starts its provenance paragraph.
    const MARKER: &str = "//! PORTED from Hamlib (";
    /// NOTICE's entry for this port.
    const NOTICE_HEADING: &str = "### Hamlib — the Icom network (LAN / Wi-Fi) protocol core";
    /// The README section that carries the credit.
    const README_HEADING: &str = "## License & credits";
    /// This module's directory, as NOTICE and README name it.
    const DIR: &str = "crates/tempo-net/src/icom/";
    /// The date of the pin; no modification of these copies predates it.
    const PIN_DATE: (u32, u32, u32) = (2026, 10, 7);

    // What every ported header carries, compared with whitespace and `//!` normalised.
    const COPYRIGHT: &str = "Copyright (c) 2026 by Mikael Nousiainen OH3BHX";
    const HOLDER: &str = "Mikael Nousiainen";
    const GRANT: &str = "the GNU General Public License as published by the Free Software \
                         Foundation; either version 3 of the License, or (at your option) any \
                         later version";
    const WARRANTY: &str = "WITHOUT ANY WARRANTY";
    const CONVERSION: &str = "The upstream notice, converted to the GNU General Public License \
                              under section 3 of the GNU Lesser General Public License, version \
                              2.1, and otherwise unchanged:";
    const SPDX: &str = "SPDX-License-Identifier: GPL-3.0-or-later";
    const MODIFIED: &str = "Modified by KD9TAW, ";
    // What no file may carry: the upstream notice unconverted.
    const LGPL_GRANT: &str = "Lesser General Public License as published by the Free Software \
                              Foundation";
    const LGPL_SPDX: &str = "SPDX-License-Identifier: LGPL-2.1-or-later";

    // What NOTICE's entry must reproduce of kappanhang's notice.
    const MIT_COPYRIGHT: &str = "Copyright (c) 2020 Norbert Varga HA2NON, Akos Marton ES1AKOS";
    const MIT_PERMISSION: &str = "Permission is hereby granted, free of charge, to any person \
                                  obtaining a copy of this software and associated documentation \
                                  files (the \"Software\"), to deal in the Software without \
                                  restriction, including without limitation the rights to use, \
                                  copy, modify, merge, publish, distribute, sublicense, and/or \
                                  sell copies of the Software, and to permit persons to whom the \
                                  Software is furnished to do so, subject to the following \
                                  conditions:";
    const MIT_CONDITION: &str = "The above copyright notice and this permission notice shall be \
                                 included in all copies or substantial portions of the Software.";
    const MIT_DISCLAIMER: &str = "THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND";
    const MIT_TRADEMARK: &str = "/Icom is a registered trademark of Icom Incorporated (Japan)/";

    /// One source file: its path relative to this directory, its leading `//!` block, all of it.
    struct Source {
        name: String,
        header: String,
        text: String,
    }

    fn is_ported(source: &Source) -> bool {
        source.header.lines().any(|l| l.starts_with(MARKER))
    }

    /// Whitespace runs collapsed to one space, so a phrase matches across line wraps.
    fn normalise(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// A `//!` block's text, without the comment markers.
    fn doc_text(header: &str) -> String {
        normalise(
            &header
                .lines()
                .map(|l| l.strip_prefix("//!").unwrap_or(l))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    /// Every comment line's text in a file (`//`, `//!`, `///` and `/* … */` lines), without the
    /// markers. Code, string literals included, is not a notice and is left out.
    fn comment_text(text: &str) -> String {
        let lines = text.lines().filter_map(|l| {
            let t = l.trim_start();
            if t.starts_with("//") {
                Some(t.trim_start_matches('/').trim_start_matches('!'))
            } else if let Some(rest) = t.strip_prefix("/*") {
                Some(rest.trim_end().trim_end_matches("*/"))
            } else {
                None
            }
        });
        normalise(&lines.collect::<Vec<_>>().join("\n"))
    }

    /// A section's text, from the line that is `heading`. A NOTICE entry (`entry`) ends at the
    /// next heading or rule; a README section at the next heading of its own level or higher.
    fn section(doc: &str, heading: &str, entry: bool) -> Option<String> {
        let mut lines = doc.lines();
        lines.find(|l| l.trim_end() == heading)?;
        let level = heading.chars().take_while(|&c| c == '#').count();
        let ends = |l: &str| {
            let hashes = l.chars().take_while(|&c| c == '#').count();
            let is_heading = hashes > 0 && l[hashes..].starts_with(' ');
            if entry {
                is_heading || l.starts_with("---")
            } else {
                is_heading && hashes <= level
            }
        };
        Some(
            lines
                .take_while(|l| !ends(l))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    /// The backticked upstream-looking paths a NOTICE entry names: containing `/` and ending
    /// `.c`, `.h` or `.md`.
    fn upstream_paths(entry: &str) -> BTreeSet<String> {
        entry
            .split('`')
            .skip(1)
            .step_by(2)
            .filter(|t| {
                t.contains('/') && (t.ends_with(".c") || t.ends_with(".h") || t.ends_with(".md"))
            })
            .map(str::to_string)
            .collect()
    }

    /// A Markdown quote's text without its `>` markers.
    fn unquote(text: &str) -> String {
        text.lines()
            .map(|l| {
                l.strip_prefix('>')
                    .map_or(l, |r| r.strip_prefix(' ').unwrap_or(r))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// A section's bullets, each with its continuation lines.
    fn bullets(section: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut open = false;
        for line in section.lines() {
            if let Some(rest) = line.strip_prefix("- ") {
                out.push(rest.to_string());
                open = true;
            } else if open && line.starts_with(char::is_whitespace) && !line.trim().is_empty() {
                let last = out.last_mut().expect("an open bullet");
                last.push(' ');
                last.push_str(line.trim());
            } else {
                open = false;
            }
        }
        out
    }

    fn is_blob(id: &str) -> bool {
        id.len() == 40
            && id
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    }

    /// The modification line's date: `None` without the line, `Some(false)` when the date is not
    /// a real calendar date on or after the pin.
    fn modified_date_is_real(header: &str) -> Option<bool> {
        let at = header.find(MODIFIED)? + MODIFIED.len();
        let date: Vec<char> = header[at..].chars().take(11).collect();
        let digits = |r: std::ops::Range<usize>| -> Option<u32> {
            let s: String = date.get(r)?.iter().collect();
            if s.chars().all(|c| c.is_ascii_digit()) {
                s.parse().ok()
            } else {
                None
            }
        };
        let shape = date.len() == 11 && date[4] == '-' && date[7] == '-' && date[10] == ':';
        let (Some(y), Some(m), Some(d)) = (digits(0..4), digits(5..7), digits(8..10)) else {
            return Some(false);
        };
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let days = match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => 0,
        };
        Some(shape && (1..=days).contains(&d) && (y, m, d) >= PIN_DATE)
    }

    /// Everything wrong, as sentences. Empty means the table, the headers, NOTICE and README
    /// agree.
    fn check(sources: &[Source], table: &[Ported], notice: &str, readme: &str) -> Vec<String> {
        let mut problems = Vec::new();
        let listed: BTreeSet<&str> = table.iter().map(|p| p.file).collect();

        for s in sources {
            let ported = is_ported(s);
            // 1. a marked file is in the table
            if ported && !listed.contains(s.name.as_str()) {
                problems.push(format!(
                    "{} says it is ported but is not in PROVENANCE",
                    s.name
                ));
            }
            // 8. the upstream notice only with the marker, and never unconverted
            let comments = comment_text(&s.text);
            if !ported && (comments.contains(HOLDER) || comments.contains(&normalise(GRANT))) {
                problems.push(format!(
                    "{} carries the upstream notice but no ported-from header",
                    s.name
                ));
            }
            if comments.contains(&normalise(LGPL_GRANT)) || comments.contains(LGPL_SPDX) {
                problems.push(format!(
                    "{} carries the upstream notice unconverted",
                    s.name
                ));
            }
        }

        let mut blob_of: BTreeMap<&str, &str> = BTreeMap::new();
        let mut drawn: BTreeMap<&str, &str> = BTreeMap::new(); // path -> the file drawing on it
        for p in table {
            let paths: BTreeSet<&str> = p
                .upstream
                .iter()
                .chain(p.references)
                .chain(p.tests)
                .copied()
                .collect();
            for path in &paths {
                drawn.entry(path).or_insert(p.file);
            }
            // 13. one well-formed blob id per path, and the same id wherever a path recurs
            for (path, id) in p.blobs {
                if !is_blob(id) {
                    problems.push(format!(
                        "{}: {id} for {path} is not a 40-digit blob id",
                        p.file
                    ));
                }
                if !paths.contains(path) {
                    problems.push(format!(
                        "{} has a blob id for {path}, which it does not draw on",
                        p.file
                    ));
                }
                if let Some(previous) = blob_of.insert(path, id) {
                    if previous != *id {
                        problems.push(format!("PROVENANCE gives two blob ids for {path}"));
                    }
                }
            }
            for path in &paths {
                match p.blobs.iter().filter(|(b, _)| b == path).count() {
                    0 => problems.push(format!("{} has no blob id for {path}", p.file)),
                    1 => {}
                    _ => problems.push(format!("{} has more than one blob id for {path}", p.file)),
                }
            }
            // 6. the differences
            if p.differences.trim().is_empty() {
                problems.push(format!("{} lists no deliberate differences", p.file));
            }
            // 2. the file exists and is marked
            let Some(source) = sources.iter().find(|s| s.name == p.file) else {
                problems.push(format!("PROVENANCE names {}, which does not exist", p.file));
                continue;
            };
            if !is_ported(source) {
                problems.push(format!("{} has no ported-from header", p.file));
            }
            // 3. the commit, in full
            if !source.header.contains(UPSTREAM_COMMIT) {
                problems.push(format!("{}'s header does not name the commit", p.file));
            }
            // 4. every upstream and reference path
            for path in p.upstream.iter().chain(p.references) {
                if !source.header.contains(path) {
                    problems.push(format!("{}'s header does not name {path}", p.file));
                }
            }
            // 5. every translated test, in the file or in its own test module beside it
            let stem = p.file.trim_end_matches(".rs");
            let beside: String = sources
                .iter()
                .filter(|s| s.name.starts_with(&format!("{stem}/")))
                .map(|s| s.text.as_str())
                .collect();
            for test in p.tests {
                let name = test.rsplit('/').next().unwrap_or(test);
                if !source.text.contains(name) && !beside.contains(name) {
                    problems.push(format!(
                        "{} does not say where {name} was translated",
                        p.file
                    ));
                }
            }
            // 7. the converted notice and the dated modification line
            let header = doc_text(&source.header);
            for (part, message) in [
                (COPYRIGHT, "the upstream copyright line"),
                (GRANT, "the converted grant"),
                (WARRANTY, "the notice of no warranty"),
                (CONVERSION, "the conversion sentence"),
                (SPDX, "the converted SPDX line"),
            ] {
                if !header.contains(&normalise(part)) {
                    problems.push(format!("{}'s header lacks {message}", p.file));
                }
            }
            match modified_date_is_real(&header) {
                None => problems.push(format!("{}'s header lacks the modification line", p.file)),
                Some(false) => {
                    problems.push(format!("{}'s modification line has no real date", p.file))
                }
                Some(true) => {}
            }
        }

        // 9. the NOTICE entry, its commit and its directory
        match section(notice, NOTICE_HEADING, true) {
            None => problems.push(format!("NOTICE has no `{NOTICE_HEADING}` entry")),
            Some(entry) => {
                let short = &UPSTREAM_COMMIT[..8];
                if !entry.contains(&format!("`{short}`")) {
                    problems.push(format!("NOTICE's entry does not name commit {short}"));
                }
                if !entry.contains(DIR) {
                    problems.push(format!("NOTICE's entry does not name {DIR}"));
                }
                // 10. the table's paths and the entry's, both ways
                for (path, file) in &drawn {
                    if !entry.contains(&format!("`{path}`")) {
                        problems.push(format!("NOTICE's entry does not name `{path}` ({file})"));
                    }
                }
                for path in upstream_paths(&entry) {
                    if !drawn.contains_key(path.as_str()) {
                        problems.push(format!(
                            "NOTICE's entry names `{path}`, which nothing in PROVENANCE draws on"
                        ));
                    }
                }
                // 11. kappanhang's notice
                let quoted = normalise(&unquote(&entry));
                for (part, message) in [
                    (MIT_COPYRIGHT, "kappanhang's copyright line"),
                    (MIT_PERMISSION, "the MIT permission notice"),
                    (MIT_CONDITION, "the MIT permission notice"),
                    (MIT_DISCLAIMER, "the MIT permission notice"),
                    (MIT_TRADEMARK, "kappanhang's trademark line"),
                ] {
                    if !quoted.contains(&normalise(part)) {
                        problems.push(format!("NOTICE's entry lacks {message}"));
                    }
                }
            }
        }

        // 12. the README credit
        match section(readme, README_HEADING, false) {
            None => problems.push(format!("README has no `{README_HEADING}` section")),
            Some(credits) => {
                if !bullets(&credits)
                    .iter()
                    .any(|b| b.contains("Hamlib") && b.contains("kappanhang") && b.contains(DIR))
                {
                    problems.push(format!(
                        "README's credits have no bullet naming Hamlib, kappanhang and {DIR}"
                    ));
                }
            }
        }
        problems
    }

    fn read_sources(dir: &Path, prefix: &str, out: &mut Vec<Source>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .expect("the icom directory")
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
    fn every_ported_file_is_in_the_table_its_header_notice_and_readme() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut sources = Vec::new();
        read_sources(&root.join("src/icom"), "", &mut sources);
        // Parser sanity: the scan found the ported files, so a clean result means something.
        assert!(
            sources
                .iter()
                .filter(|s| s.header.lines().any(|l| l.starts_with(MARKER)))
                .count()
                >= PROVENANCE.len(),
            "the scan found the ported files"
        );
        let notice = std::fs::read_to_string(root.join("../../NOTICE")).expect("the repo NOTICE");
        let readme =
            std::fs::read_to_string(root.join("../../README.md")).expect("the repo README");
        let problems = check(&sources, PROVENANCE, &notice, &readme);
        assert!(problems.is_empty(), "{problems:#?}");
    }

    /// A ported header as the real files carry it, naming `upstream`. Built line by line, so no
    /// line of this file's own source reads as a comment carrying the notice.
    fn ported_header(upstream: &str, date: &str) -> String {
        [
            "A module.".to_string(),
            "PORTED from Hamlib (https://github.com/Hamlib/Hamlib, pull request #2178, open when \
             taken),"
                .to_string(),
            format!("`{upstream}` at commit"),
            format!("`{UPSTREAM_COMMIT}` (2026-10-07), translated from C to Rust."),
            "Deliberate differences: some.".to_string(),
            String::new(),
            "The upstream notice, converted to the GNU General Public License under section 3"
                .to_string(),
            "of the GNU Lesser General Public License, version 2.1, and otherwise unchanged:"
                .to_string(),
            String::new(),
            COPYRIGHT.to_string(),
            String::new(),
            "This library is free software; you can redistribute it and/or modify it under the"
                .to_string(),
            "terms of the GNU General Public License as published by the Free Software".to_string(),
            "Foundation; either version 3 of the License, or (at your option) any later"
                .to_string(),
            "version.".to_string(),
            String::new(),
            "This library is distributed in the hope that it will be useful, but WITHOUT ANY \
             WARRANTY;"
                .to_string(),
            String::new(),
            SPDX.to_string(),
            String::new(),
            format!("{MODIFIED}{date}: translated from C to Rust."),
        ]
        .iter()
        .map(|l| format!("//! {l}").trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
    }

    const A_BLOB: &str = "06c29d31e21fd2fcd6c571fe5b230470d09a7c71";
    const T_BLOB: &str = "ba09a98578ea17b3fd1dfa12ff6ee0b6475856a7";

    /// A small consistent world for the checker's positive controls.
    fn world() -> (Vec<Source>, Vec<Ported>, String, String) {
        let header = ported_header("rigs/icom/a.c", "2026-10-07");
        let sources = vec![
            Source {
                name: "a.rs".into(),
                text: format!("{header}\nmod tests {{}} // test/test_a.c"),
                header,
            },
            Source {
                name: "own.rs".into(),
                header: "//! Nexus's own.".into(),
                text: "//! Nexus's own.\nfn f() {}".into(),
            },
        ];
        let table = vec![Ported {
            file: "a.rs",
            upstream: &["rigs/icom/a.c"],
            references: &[],
            tests: &["test/test_a.c"],
            differences: "some",
            blobs: &[("rigs/icom/a.c", A_BLOB), ("test/test_a.c", T_BLOB)],
        }];
        let notice = format!(
            "## Upstream lineage\n\n{NOTICE_HEADING}\nPorted from `2e3e4a6a` into `{DIR}`:\n\
             - the code (`rigs/icom/a.c`);\n- the tests (`test/test_a.c`).\n\n\
             > MIT License\n>\n> {MIT_COPYRIGHT}\n>\n> Permission is hereby granted, free of \
             charge, to any person obtaining a copy\n> of this software and associated \
             documentation files (the \"Software\"), to deal\n> in the Software without \
             restriction, including without limitation the rights\n> to use, copy, modify, \
             merge, publish, distribute, sublicense, and/or sell\n> copies of the Software, and \
             to permit persons to whom the Software is\n> furnished to do so, subject to the \
             following conditions:\n>\n> The above copyright notice and this permission notice \
             shall be included in all\n> copies or substantial portions of the Software.\n>\n> \
             THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR\n>\n> \
             {MIT_TRADEMARK}\n\n### Next entry\nNames `rigs/icom/b.c`.\n"
        );
        let readme = format!(
            "# Nexus\n\n{README_HEADING}\n\n- **Other** — something.\n- **[Hamlib](x)'s Icom \
             network backend** — the core\n  (`{DIR}`) descends from **[kappanhang](y)**.\n\n\
             ## Next section\n"
        );
        (sources, table, notice, readme)
    }

    fn fires(problems: &[String], needle: &str) -> bool {
        problems.iter().any(|m| m.contains(needle))
    }

    #[test]
    fn the_checker_accepts_a_consistent_world() {
        let (sources, table, notice, readme) = world();
        assert_eq!(
            check(&sources, &table, &notice, &readme),
            Vec::<String>::new()
        );
    }

    #[test]
    fn the_checker_fires_on_each_kind_of_gap() {
        // Each change below must be caught: the positive controls for the test above, one or
        // more per check.

        // 1. a file marked as ported that the table does not list
        let (mut sources, table, notice, readme) = world();
        sources[1].header.push_str(&format!("\n{MARKER}...)"));
        let p = check(&sources, &table, &notice, &readme);
        assert!(
            fires(&p, "own.rs says it is ported but is not in PROVENANCE"),
            "{p:?}"
        );

        // 2. a table row with no file behind it, and one whose file is not marked
        let (sources, mut table, notice, readme) = world();
        table.push(Ported {
            file: "gone.rs",
            ..table[0]
        });
        table.push(Ported {
            file: "own.rs",
            ..table[0]
        });
        let p = check(&sources, &table, &notice, &readme);
        assert!(
            fires(&p, "PROVENANCE names gone.rs, which does not exist"),
            "{p:?}"
        );
        assert!(fires(&p, "own.rs has no ported-from header"), "{p:?}");

        // 3. a header that does not name the commit in full
        let (mut sources, table, notice, readme) = world();
        sources[0].header = sources[0].header.replace(UPSTREAM_COMMIT, "2e3e4a6a");
        let p = check(&sources, &table, &notice, &readme);
        assert!(fires(&p, "a.rs's header does not name the commit"), "{p:?}");

        // 4. a header that does not name an upstream or reference path of its row
        let (sources, mut table, notice, readme) = world();
        table[0].upstream = &["rigs/icom/a.c", "rigs/icom/extra.c"];
        table[0].references = &["rigs/icom/fact.md"];
        let p = check(&sources, &table, &notice, &readme);
        assert!(
            fires(&p, "a.rs's header does not name rigs/icom/extra.c"),
            "{p:?}"
        );
        assert!(
            fires(&p, "a.rs's header does not name rigs/icom/fact.md"),
            "{p:?}"
        );

        // 5. an upstream test not named where it was translated
        let (mut sources, table, notice, readme) = world();
        sources[0].text = sources[0].header.clone();
        let p = check(&sources, &table, &notice, &readme);
        assert!(
            fires(&p, "does not say where test_a.c was translated"),
            "{p:?}"
        );

        // 6. no deliberate differences
        let (sources, mut table, notice, readme) = world();
        table[0].differences = " ";
        let p = check(&sources, &table, &notice, &readme);
        assert!(fires(&p, "a.rs lists no deliberate differences"), "{p:?}");

        // 7. each part of the converted notice, and the modification date
        for (part, message) in [
            (COPYRIGHT, "the upstream copyright line"),
            ("version 3 of the License", "the converted grant"),
            (WARRANTY, "the notice of no warranty"),
            ("under section 3", "the conversion sentence"),
            (SPDX, "the converted SPDX line"),
            (MODIFIED, "the modification line"),
        ] {
            let (mut sources, table, notice, readme) = world();
            sources[0].header = sources[0].header.replace(part, "x");
            sources[0].text = format!("{}\n// test/test_a.c", sources[0].header);
            let p = check(&sources, &table, &notice, &readme);
            assert!(
                fires(&p, &format!("a.rs's header lacks {message}")),
                "{part}: {p:?}"
            );
        }
        for date in [
            "2026-13-01",
            "2026-02-30",
            "2026-10-06",
            "26-10-07",
            "2026-1-07",
        ] {
            let (mut sources, table, notice, readme) = world();
            sources[0].header = ported_header("rigs/icom/a.c", date);
            sources[0].text = format!("{}\n// test/test_a.c", sources[0].header);
            let p = check(&sources, &table, &notice, &readme);
            assert!(
                fires(&p, "a.rs's modification line has no real date"),
                "{date}: {p:?}"
            );
        }
        let (mut sources, table, notice, readme) = world();
        sources[0].header = ported_header("rigs/icom/a.c", "2027-02-28");
        sources[0].text = format!("{}\n// test/test_a.c", sources[0].header);
        assert!(check(&sources, &table, &notice, &readme).is_empty());

        // 8. the upstream notice without the marker, or left unconverted
        let (mut sources, table, notice, readme) = world();
        sources[1].text.push_str(&format!("\n// {COPYRIGHT}"));
        let p = check(&sources, &table, &notice, &readme);
        assert!(
            fires(
                &p,
                "own.rs carries the upstream notice but no ported-from header"
            ),
            "{p:?}"
        );
        let (mut sources, table, notice, readme) = world();
        sources[1].text.push_str("\n    // the terms of the GNU General Public License as published by the Free Software Foundation; either version 3 of the License, or (at your option) any later version");
        let p = check(&sources, &table, &notice, &readme);
        assert!(
            fires(
                &p,
                "own.rs carries the upstream notice but no ported-from header"
            ),
            "{p:?}"
        );
        let (mut sources, table, notice, readme) = world();
        sources[0].text.push_str(&format!("\n// GNU {LGPL_GRANT}"));
        let p = check(&sources, &table, &notice, &readme);
        assert!(
            fires(&p, "a.rs carries the upstream notice unconverted"),
            "{p:?}"
        );
        let (mut sources, table, notice, readme) = world();
        sources[1].text.push_str(&format!("\n/* {LGPL_SPDX} */"));
        let p = check(&sources, &table, &notice, &readme);
        assert!(
            fires(&p, "own.rs carries the upstream notice unconverted"),
            "{p:?}"
        );
        // a string in code is not a notice: the checker's own constants live in code
        let (mut sources, table, notice, readme) = world();
        sources[1]
            .text
            .push_str(&format!("\nconst C: &str = \"{COPYRIGHT}\";"));
        assert!(check(&sources, &table, &notice, &readme).is_empty());

        // 9. the NOTICE entry, its commit and its directory
        let (sources, table, notice, readme) = world();
        let p = check(&sources, &table, "no entry here", &readme);
        assert!(fires(&p, "NOTICE has no"), "{p:?}");
        let p = check(
            &sources,
            &table,
            &notice.replace("2e3e4a6a", "deadbeef"),
            &readme,
        );
        assert!(
            fires(&p, "NOTICE's entry does not name commit 2e3e4a6a"),
            "{p:?}"
        );
        let p = check(&sources, &table, &notice.replace(DIR, "crates/x/"), &readme);
        assert!(
            fires(
                &p,
                "NOTICE's entry does not name crates/tempo-net/src/icom/"
            ),
            "{p:?}"
        );

        // 10. a path in the table that the entry does not name, and one the entry names that
        //     nothing draws on
        let (sources, table, notice, readme) = world();
        let p = check(
            &sources,
            &table,
            &notice.replace("`test/test_a.c`", "tests"),
            &readme,
        );
        assert!(
            fires(&p, "NOTICE's entry does not name `test/test_a.c` (a.rs)"),
            "{p:?}"
        );
        let lingering = notice.replace("(`rigs/icom/a.c`)", "(`rigs/icom/a.c`, `rigs/icom/old.h`)");
        let p = check(&sources, &table, &lingering, &readme);
        assert!(
            fires(
                &p,
                "NOTICE's entry names `rigs/icom/old.h`, which nothing in PROVENANCE draws on"
            ),
            "{p:?}"
        );
        let doc = notice.replace(
            "(`rigs/icom/a.c`)",
            "(`rigs/icom/a.c`, `rigs/icom/NOTES.md`)",
        );
        let p = check(&sources, &table, &doc, &readme);
        assert!(fires(&p, "`rigs/icom/NOTES.md`, which nothing"), "{p:?}");

        // 11. kappanhang's notice in the entry
        for (part, message) in [
            (MIT_COPYRIGHT, "kappanhang's copyright line"),
            ("Permission is hereby granted", "the MIT permission notice"),
            ("shall be included in all", "the MIT permission notice"),
            ("PROVIDED \"AS IS\"", "the MIT permission notice"),
            ("registered trademark", "kappanhang's trademark line"),
        ] {
            let (sources, table, notice, readme) = world();
            let p = check(&sources, &table, &notice.replace(part, "x"), &readme);
            assert!(
                fires(&p, &format!("NOTICE's entry lacks {message}")),
                "{part}: {p:?}"
            );
        }

        // 12. the README credit
        for gone in ["Hamlib", "kappanhang", DIR] {
            let (sources, table, notice, readme) = world();
            let p = check(&sources, &table, &notice, &readme.replace(gone, "x"));
            assert!(
                fires(&p, "README's credits have no bullet naming Hamlib"),
                "{gone}: {p:?}"
            );
        }
        let (sources, table, notice, readme) = world();
        let moved = readme.replace("## Next section\n", "") + "\n## Elsewhere\n- Hamlib\n";
        let p = check(
            &sources,
            &table,
            &notice,
            &moved.replace(README_HEADING, "## Credits"),
        );
        assert!(fires(&p, "README has no"), "{p:?}");

        // 13. the blob ids
        for (blobs, message) in [
            (
                &[
                    ("rigs/icom/a.c", "06c29d31e21fd2fcd6c571fe5b230470d09a7c7"),
                    ("test/test_a.c", T_BLOB),
                ][..],
                "is not a 40-digit blob id",
            ),
            (
                &[
                    ("rigs/icom/a.c", "06C29D31E21FD2FCD6C571FE5B230470D09A7C71"),
                    ("test/test_a.c", T_BLOB),
                ][..],
                "is not a 40-digit blob id",
            ),
            (
                &[("test/test_a.c", T_BLOB)][..],
                "has no blob id for rigs/icom/a.c",
            ),
            (
                &[
                    ("rigs/icom/a.c", A_BLOB),
                    ("rigs/icom/a.c", A_BLOB),
                    ("test/test_a.c", T_BLOB),
                ][..],
                "has more than one blob id for rigs/icom/a.c",
            ),
            (
                &[
                    ("rigs/icom/a.c", A_BLOB),
                    ("test/test_a.c", T_BLOB),
                    ("rigs/icom/z.c", A_BLOB),
                ][..],
                "has a blob id for rigs/icom/z.c",
            ),
        ] {
            let (sources, mut table, notice, readme) = world();
            table[0].blobs = blobs;
            let p = check(&sources, &table, &notice, &readme);
            assert!(fires(&p, message), "{message}: {p:?}");
        }
        let (mut sources, mut table, notice, readme) = world();
        sources.push(Source {
            name: "b.rs".into(),
            text: format!("{}\n// test/test_a.c", sources[0].header),
            header: sources[0].header.clone(),
        });
        table.push(Ported {
            file: "b.rs",
            blobs: &[("rigs/icom/a.c", T_BLOB), ("test/test_a.c", T_BLOB)],
            ..table[0]
        });
        let p = check(&sources, &table, &notice, &readme);
        assert!(fires(&p, "two blob ids for rigs/icom/a.c"), "{p:?}");

        // 14. a name in the NEXT entry is not this entry's: the section ends at the next heading
        let (sources, table, notice, readme) = world();
        assert!(!fires(&check(&sources, &table, &notice, &readme), "b.c"));
    }
}
