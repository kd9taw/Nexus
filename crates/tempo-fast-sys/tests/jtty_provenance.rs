//! The JTTY modem sources vendored from WSJT-X, and the upstream tests vendored with them, held
//! to the upstream commit they were taken from.
//!
//! Every file in [`VENDORED`] is upstream's file at [`UPSTREAM_COMMIT`], byte for byte: the table
//! pins the git blob id of each one at that commit, and the test computes the blob id of the copy
//! in this tree. The JTTY directories hold nothing else. NOTICE names the tag, the commit and
//! every modem file; the vendor README lists every file vendored and, separately, what was not
//! taken, and both lists must agree with the table; the README credit names the directory. While
//! the tag is a WSJT-X pre-release, NOTICE and the credit say that it is one, and once it is not,
//! they must stop saying so.
//!
//! Re-taking the files (at the WSJT-X general release that carries JTTY) is one commit that
//! changes the files, [`UPSTREAM_TAG`], [`UPSTREAM_COMMIT`], every blob id, NOTICE and both
//! READMEs together. In a WSJT-X clone, `git ls-tree -r <commit> lib/jtty
//! lib/77bit/packjt77_grammar.f90 tests/unit/jtty tests/fixtures/jtty` prints the ids.
//!
//! A Nexus change to a vendored file fails this test by design. The commit that makes one marks
//! the change in the file the way the 3.0.2 sources do (`! MODIFIED FOR NEXUS (KD9TAW, <date>):
//! <why>`), says so in NOTICE and the vendor README, and changes this test to hold the modified
//! file.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The upstream repository.
const UPSTREAM: &str = "https://github.com/WSJTX/wsjtx";
/// The upstream tag the files were taken at.
const UPSTREAM_TAG: &str = "v3.3.0-beta4";
/// The commit that tag names (2026-10-21).
const UPSTREAM_COMMIT: &str = "0f7d5928449ec747c3d1f6da1306f8c94404c3a9";

/// The vendored WSJT-X tree, from the repository root.
const VENDOR: &str = "libtempo/vendor/wsjtx/";
/// The directories under [`VENDOR`] that hold vendored JTTY files and nothing else.
const JTTY_DIRS: &[&str] = &["lib/jtty/", "tests/unit/jtty/", "tests/fixtures/jtty/"];

/// Every vendored file: its path under [`VENDOR`], which is its path upstream too, and the git
/// blob id of that path at [`UPSTREAM_COMMIT`].
const VENDORED: &[(&str, &str)] = &[
    (
        "lib/77bit/packjt77_grammar.f90",
        "fb6939a9d2ba35d4c00cba3d1f9233b148a2e026",
    ),
    (
        "lib/jtty/ana64a.f90",
        "fe7c7fb8cca3995c87ace3a5bf78247ec243f11e",
    ),
    (
        "lib/jtty/gen_jttywave.f90",
        "88c7eafbb3fa386a1c1028586dbaad5e4396b539",
    ),
    (
        "lib/jtty/gen_syncwave.f90",
        "07c0ae439656a0605d09a7888b3b4138b0e6ab97",
    ),
    (
        "lib/jtty/genjtty.f90",
        "6ad719fe92118a7d81e229d9ad9a7cf50353dd84",
    ),
    (
        "lib/jtty/jtty_block_pow.f90",
        "795e85595e23ad14e5e86f4423bf20fc65ec6ecb",
    ),
    (
        "lib/jtty/jtty_decode.f90",
        "357c687c2ae9bc18ffab332b27293bf36473fd75",
    ),
    (
        "lib/jtty/jtty_fec_mod.f90",
        "b73a3e626566e51e95e8d14f1a4b2e070393e48e",
    ),
    (
        "lib/jtty/jtty_mdecode.f90",
        "381eea0e47d0e24503d3cd914a5074aa65582ae9",
    ),
    (
        "lib/jtty/jtty_mod.f90",
        "e043552fe909d2d384bcde5e7a9651d154f3dda9",
    ),
    (
        "lib/jtty/jtty_payload_correlators.f90",
        "296ffe697d620aa2f3a5b0c9ddc3ec81a3a4e16a",
    ),
    (
        "lib/jtty/jtty_peakup.f90",
        "80afcbe2cbb7f023954a7ff1b7af5136a16cc6a4",
    ),
    (
        "lib/jtty/jtty_receive_context.f90",
        "08ca347978c939c0c9af86949549e175c1913a75",
    ),
    (
        "lib/jtty/jtty_source_codec.f90",
        "1c1058a9c75765876244823527c7f12e34e9f01c",
    ),
    (
        "lib/jtty/jtty_tbcc_code_profile.f90",
        "9037d79095250684ce12b15770d38b140f8fdda4",
    ),
    (
        "lib/jtty/jtty_tbcc_decoder.f90",
        "c010331d3cf0a547e5f0ca59578d085d972c6146",
    ),
    (
        "lib/jtty/jtty_tbcc_list_decoder.f90",
        "ca4a4bd123ca82bcbf25fe356f82bd458468f9cd",
    ),
    (
        "lib/jtty/rjtty_sub.f90",
        "2ab3ab2598767ec0a6b08118e72c2a5244a574a2",
    ),
    (
        "lib/jtty/subtract_jtty.f90",
        "eeb062a0817cac1044ea01775887233e5b50774b",
    ),
    (
        "lib/jtty/tbcc.f90",
        "c3a89c0c35bde627d85fafbf8859c7a846ac96e5",
    ),
    (
        "tests/fixtures/jtty/jtty_msgs.txt",
        "d1b1c04d7314add476ea2c039bd1d65c0a1b7383",
    ),
    (
        "tests/unit/jtty/test_jtty_adjacent_decode.f90",
        "dc3e6cdd8de899d200e1a516801f0293a68bf316",
    ),
    (
        "tests/unit/jtty/test_jtty_block_pow.f90",
        "e4366970ab0cd2599ca7d5a5404bf9511bbfae7e",
    ),
    (
        "tests/unit/jtty/test_jtty_continuous_decode.f90",
        "38de8224c6a140572a7090ee3c19159f843c3774",
    ),
    (
        "tests/unit/jtty/test_jtty_frequency_window.f90",
        "897fd823cacbef6b78546afec4330b17dcf72db6",
    ),
    (
        "tests/unit/jtty/test_jtty_gap_merge.f90",
        "ff13e28e36d21c7c96cbd493cae4baf381195501",
    ),
    (
        "tests/unit/jtty/test_jtty_get_updates.f90",
        "7baaae6c5d8ff32be3c72912302fe2d10eba32b1",
    ),
    (
        "tests/unit/jtty/test_jtty_message_matching.f90",
        "1308009a74e3fecc655193cc9e5bd2c0c69383bd",
    ),
    (
        "tests/unit/jtty/test_jtty_overlap_decode.f90",
        "977778595617291b03a5ba3384958b059390006b",
    ),
    (
        "tests/unit/jtty/test_jtty_pack.f90",
        "2ffd23abeef199c6d0fb727f6741ec8b80cf764d",
    ),
    (
        "tests/unit/jtty/test_jtty_payload_correlators.f90",
        "a87fd6c40cb60d99c6391656545a1ecef580b984",
    ),
    (
        "tests/unit/jtty/test_jtty_receive_state.f90",
        "232679ceb8186a877a390beddc4a7d720bf5f657",
    ),
    (
        "tests/unit/jtty/test_jtty_source_codec.f90",
        "3d1ee9b05538a5aa042f0e59b474c64ccbebb4f4",
    ),
    (
        "tests/unit/jtty/test_jtty_sticky_retry.f90",
        "719b1ecfbb6a8c962629d1f68e629f1007d9b690",
    ),
    (
        "tests/unit/jtty/test_jtty_surface_ranges.f90",
        "97f9b65c30f9ba0003bb30d11b9cc4d467af2fac",
    ),
    (
        "tests/unit/jtty/test_jtty_tbcc_code_profile.f90",
        "f49bcf60040f14c2af11538f063e50a75f9a4165",
    ),
    (
        "tests/unit/jtty/test_jtty_tbcc_decoder.f90",
        "09d5f032579282b8dd40ce5fe74161f2a3c5aff2",
    ),
    (
        "tests/unit/jtty/test_jtty_tbcc_list_decoder.f90",
        "545e02214c25895a8e98282db485891bd6bbfe80",
    ),
    (
        "tests/unit/jtty/test_jtty_windowed_decode.f90",
        "19bb345f517474e856802b4b76e71feeeb68816f",
    ),
    (
        "tests/unit/jtty/test_tbcc.f90",
        "97fe50d166e0c1ced3cecb0c176d275baf972903",
    ),
];

/// NOTICE's entry for these files.
const NOTICE_HEADING: &str = "### JTTY — the keyboard-mode modem core, from WSJT-X";
/// The vendor README's section for them, and its two lists: the per-file treatment.
const VENDOR_README_HEADING: &str = "## JTTY — vendored from WSJT-X";
const VENDORED_LIST: &str = "### Vendored";
const NOT_VENDORED_LIST: &str = "### Not vendored";
/// The README section that carries the credit.
const README_HEADING: &str = "## License & credits";
/// The directory NOTICE and the README credit name.
const DIR: &str = "libtempo/vendor/wsjtx/lib/jtty/";
/// What NOTICE's entry says while the tag is a pre-release, and stops saying once it is not.
const PRE_RELEASE: &str = "No WSJT-X general release carries JTTY yet";
/// NOTICE's disclaimer.
const NOT_ENDORSED: &str = "not endorsed by nor affiliated with the WSJT Development";

/// What the checker reads: the pin, the files found in the tree, and the three documents.
struct World {
    tag: &'static str,
    commit: &'static str,
    table: Vec<(&'static str, &'static str)>,
    /// Every file found in a JTTY directory, and every table file found anywhere, by its path
    /// under [`VENDOR`].
    files: BTreeMap<String, Vec<u8>>,
    notice: String,
    vendor_readme: String,
    readme: String,
}

/// A WSJT-X tag for a pre-release (`v3.2.0-rc1`, `v3.3.0-beta1`), not a general release.
fn is_pre_release(tag: &str) -> bool {
    ["-rc", "-beta", "-alpha"].iter().any(|m| tag.contains(m))
}

/// SHA-1 (FIPS 180-4), for git's blob ids. The workspace has no hash crate and this test is its
/// only use; the known-answer test below pins it.
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&(data.len() as u64 * 8).to_be_bytes());
    for block in message.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut digest = [0u8; 20];
    for (out, x) in digest.chunks_exact_mut(4).zip(h) {
        out.copy_from_slice(&x.to_be_bytes());
    }
    digest
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The id git gives a file's bytes: the SHA-1 of `blob <length>\0` and the bytes.
fn git_blob_id(bytes: &[u8]) -> String {
    let mut object = format!("blob {}\0", bytes.len()).into_bytes();
    object.extend_from_slice(bytes);
    hex(&sha1(&object))
}

fn is_blob(id: &str) -> bool {
    id.len() == 40
        && id
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// Whitespace runs collapsed to one space, so a phrase matches across line wraps.
fn normalise(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The spans of a Markdown text between backticks.
fn ticked(text: &str) -> BTreeSet<&str> {
    text.split('`').skip(1).step_by(2).collect()
}

/// The part of a path after its last `/`.
fn base(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// A section's text, from the line that is `heading`. A NOTICE entry (`entry`) ends at the next
/// heading or rule; a README section at the next heading of its own level or higher.
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

/// Everything wrong, as sentences. Empty means every vendored file is upstream's file at the pin
/// and the documents agree with the table.
fn check(w: &World) -> Vec<String> {
    let mut problems = Vec::new();
    let short = &w.commit[..8];
    let pre_release = is_pre_release(w.tag);

    // 4. one well-formed pin per path
    let mut listed = BTreeSet::new();
    for (path, blob) in &w.table {
        if !is_blob(blob) {
            problems.push(format!("{blob} for {path} is not a 40-digit blob id"));
        }
        if !listed.insert(*path) {
            problems.push(format!("VENDORED lists {path} twice"));
        }
    }
    // 1. nothing in a JTTY directory that the table does not list
    for path in w.files.keys() {
        if JTTY_DIRS.iter().any(|d| path.starts_with(d)) && !listed.contains(path.as_str()) {
            problems.push(format!("{path} is in a JTTY directory but not in VENDORED"));
        }
    }
    // 2, 3. every listed file is here, and is upstream's file at the pin
    for (path, blob) in &w.table {
        match w.files.get(*path) {
            None => problems.push(format!("VENDORED names {path}, which is not in the tree")),
            Some(bytes) => {
                let id = git_blob_id(bytes);
                if id != *blob {
                    problems.push(format!(
                        "{path} is not upstream's file at {short}: its blob id is {id}, the \
                         pin is {blob}"
                    ));
                }
            }
        }
    }
    let vendored_names: BTreeSet<&str> = w.table.iter().map(|(p, _)| base(p)).collect();

    // 5, 6. NOTICE's entry
    match section(&w.notice, NOTICE_HEADING, true) {
        None => problems.push(format!("NOTICE has no `{NOTICE_HEADING}` entry")),
        Some(entry) => {
            let names = ticked(&entry);
            let commit = format!("`{short}`");
            for (part, what) in [
                (UPSTREAM, "the upstream repository"),
                (w.tag, "the tag"),
                (commit.as_str(), "the commit"),
                (DIR, "the directory"),
            ] {
                if !entry.contains(part) {
                    problems.push(format!("NOTICE's entry does not name {what}, {part}"));
                }
            }
            for (path, _) in w.table.iter().filter(|(p, _)| p.starts_with("lib/")) {
                if !names.contains(base(path)) {
                    problems.push(format!("NOTICE's entry does not name `{}`", base(path)));
                }
            }
            for name in names.iter().filter(|n| n.ends_with(".f90")) {
                if !vendored_names.contains(base(name)) {
                    problems.push(format!(
                        "NOTICE's entry names `{name}`, which is not vendored"
                    ));
                }
            }
            let text = normalise(&entry);
            if !text.contains(NOT_ENDORSED) {
                problems.push("NOTICE's entry lacks the disclaimer".to_string());
            }
            match (pre_release, text.contains(PRE_RELEASE)) {
                (true, false) => problems.push(format!(
                    "{} is a pre-release, but NOTICE's entry does not say no general release \
                     carries JTTY yet",
                    w.tag
                )),
                (false, true) => problems.push(format!(
                    "{} is a general release, but NOTICE's entry still says none carries JTTY",
                    w.tag
                )),
                _ => {}
            }
        }
    }

    // 7. the vendor README's section: the pin, and the per-file treatment both ways
    match section(&w.vendor_readme, VENDOR_README_HEADING, false) {
        None => problems.push(format!(
            "the vendor README has no `{VENDOR_README_HEADING}` section"
        )),
        Some(text) => {
            for (part, what) in [(w.tag, "the tag"), (w.commit, "the commit")] {
                if !text.contains(part) {
                    problems.push(format!(
                        "the vendor README's section does not name {what}, {part}"
                    ));
                }
            }
            match section(&text, VENDORED_LIST, true) {
                None => problems.push(format!("the vendor README has no `{VENDORED_LIST}` list")),
                Some(list) => {
                    let names = ticked(&list);
                    for (path, _) in &w.table {
                        if !names.contains(path) {
                            problems.push(format!(
                                "the vendor README's `{VENDORED_LIST}` list does not name \
                                 `{path}`"
                            ));
                        }
                    }
                    let files = names
                        .iter()
                        .filter(|n| n.contains('/') && !n.ends_with('/'));
                    for name in files {
                        if !listed.contains(name) {
                            problems.push(format!(
                                "the vendor README lists `{name}` as vendored, but VENDORED \
                                 does not"
                            ));
                        }
                    }
                }
            }
            match section(&text, NOT_VENDORED_LIST, true) {
                None => problems.push(format!(
                    "the vendor README has no `{NOT_VENDORED_LIST}` list"
                )),
                Some(list) => {
                    for name in ticked(&list).iter().filter(|n| n.contains('/')) {
                        let here = if name.ends_with('/') {
                            w.files.keys().any(|f| f.starts_with(name))
                        } else {
                            listed.contains(name) || w.files.contains_key(*name)
                        };
                        if here {
                            problems.push(format!(
                                "the vendor README lists `{name}` as not vendored, but it is"
                            ));
                        }
                    }
                }
            }
        }
    }

    // 8. the README credit
    match section(&w.readme, README_HEADING, false) {
        None => problems.push(format!("README has no `{README_HEADING}` section")),
        Some(credits) => {
            let credit = bullets(&credits)
                .into_iter()
                .find(|b| b.contains("WSJT-X") && b.contains("JTTY") && b.contains(DIR));
            match credit {
                None => problems.push(format!(
                    "README's credits have no bullet naming WSJT-X, JTTY and {DIR}"
                )),
                Some(b) if b.contains("pre-release") != pre_release => {
                    problems.push(format!(
                        "README's JTTY credit and the tag {} disagree about a pre-release",
                        w.tag
                    ));
                }
                Some(_) => {}
            }
        }
    }
    problems
}

/// Every file under `dir`, a directory under `vendor`, by its path under `vendor`.
fn read_dir(vendor: &Path, dir: &str, files: &mut BTreeMap<String, Vec<u8>>) {
    let Ok(entries) = std::fs::read_dir(vendor.join(dir)) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        let name = path.file_name().expect("a name").to_string_lossy();
        if path.is_dir() {
            read_dir(vendor, &format!("{dir}{name}/"), files);
        } else {
            let bytes = std::fs::read(&path).expect("a vendored file");
            files.insert(format!("{dir}{name}"), bytes);
        }
    }
}

#[test]
fn every_vendored_jtty_file_is_upstreams_and_the_documents_agree() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let vendor = root.join(VENDOR);
    let mut files = BTreeMap::new();
    for dir in JTTY_DIRS {
        read_dir(&vendor, dir, &mut files);
    }
    for (path, _) in VENDORED {
        if let Ok(bytes) = std::fs::read(vendor.join(path)) {
            files.insert(path.to_string(), bytes);
        }
    }
    let read = |path: &str| {
        std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
    };
    let world = World {
        tag: UPSTREAM_TAG,
        commit: UPSTREAM_COMMIT,
        table: VENDORED.to_vec(),
        files,
        notice: read("NOTICE"),
        vendor_readme: read("libtempo/vendor/wsjtx/README.md"),
        readme: read("README.md"),
    };
    let problems = check(&world);
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn sha1_and_the_blob_id_give_the_published_values() {
    let million = vec![b'a'; 1_000_000];
    for (input, digest) in [
        (&b""[..], "da39a3ee5e6b4b0d3255bfef95601890afd80709"),
        (&b"abc"[..], "a9993e364706816aba3e25717850c26c9cd0d89d"),
        (
            &b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"[..],
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1",
        ),
        (&[b'x'; 55][..], "cef734ba81a024479e09eb5a75b6ddae62e6abf1"),
        (&[b'x'; 64][..], "bb2fa3ee7afb9f54c6dfb5d021f14b1ffe40c163"),
        (&million[..], "34aa973cd4c4daa4f61eeb2bdbad27316534016f"),
    ] {
        assert_eq!(hex(&sha1(input)), digest, "{} bytes", input.len());
    }
    // As `git hash-object` gives them.
    assert_eq!(git_blob_id(b""), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
    assert_eq!(
        git_blob_id(b"what is up, doc?"),
        "bd9dbf5aae1a3862dd1526723246b20206e5fc37"
    );
}

const CODEC: &[u8] = b"module jtty_mod\nend module jtty_mod\n";
const CODEC_BLOB: &str = "b2d789f60910022e5d5d0e1fd857e628e01bf8bf";
const GRAMMAR: &[u8] = b"module packjt77_grammar\nend module packjt77_grammar\n";
const GRAMMAR_BLOB: &str = "6655b14392cd91344541ce1f02bf8e55e2e7e573";
const TEST: &[u8] = b"program test_jtty_pack\nend program test_jtty_pack\n";
const TEST_BLOB: &str = "6bdcd4ec5980edc90716e6f76d854d1915e3ef2b";

/// A small consistent world for the checker's positive controls. Its blob ids are the ones
/// `git hash-object` gives its three files.
fn world() -> World {
    let files = [
        ("lib/jtty/jtty_mod.f90", CODEC),
        ("lib/77bit/packjt77_grammar.f90", GRAMMAR),
        ("tests/unit/jtty/test_jtty_pack.f90", TEST),
    ]
    .into_iter()
    .map(|(p, b)| (p.to_string(), b.to_vec()))
    .collect();
    World {
        tag: UPSTREAM_TAG,
        commit: UPSTREAM_COMMIT,
        table: vec![
            ("lib/77bit/packjt77_grammar.f90", GRAMMAR_BLOB),
            ("lib/jtty/jtty_mod.f90", CODEC_BLOB),
            ("tests/unit/jtty/test_jtty_pack.f90", TEST_BLOB),
        ],
        files,
        notice: format!(
            "## Upstream lineage\n\n{NOTICE_HEADING}\n<{UPSTREAM}>, taken at the tag \
             `{UPSTREAM_TAG}`, commit `0f7d5928`.\n\nVendored at `{DIR}`: the codec \
             (`jtty_mod.f90`, with\n`packjt77_grammar.f90`), and its test.\n\n{PRE_RELEASE}.\n\n\
             Nexus is {NOT_ENDORSED}\nTeam.\n\n### Next entry\nNames `other.f90`.\n"
        ),
        vendor_readme: format!(
            "# Vendored\n\n{VENDOR_README_HEADING}\n\nTaken at `{UPSTREAM_TAG}`, commit \
             `{UPSTREAM_COMMIT}`, into `lib/jtty/`.\n\n{VENDORED_LIST}\n\n\
             - `lib/jtty/jtty_mod.f90`\n- `lib/77bit/packjt77_grammar.f90`\n\
             - `tests/unit/jtty/test_jtty_pack.f90`\n\n{NOT_VENDORED_LIST}\n\n\
             - `lib/jtty/jtty.f90`, the program; `lib/jtty/wava/`, two simulations.\n\n\
             ## Next section\nNames `lib/jtty/later.f90`.\n"
        ),
        readme: format!(
            "# Nexus\n\n{README_HEADING}\n\n- **Other** — something.\n- **WSJT-X** — the \
             modem, and the JTTY modem sources\n  (`{DIR}`, from a WSJT-X pre-release).\n\n\
             ## Next section\n"
        ),
    }
}

fn fires(problems: &[String], needle: &str) -> bool {
    problems.iter().any(|m| m.contains(needle))
}

#[test]
fn the_checker_accepts_a_consistent_world() {
    assert_eq!(check(&world()), Vec::<String>::new());
    // A Nexus file beside the JTTY files, outside the JTTY directories, is not the table's
    // business.
    let mut w = world();
    w.files
        .insert("lib/77bit/packjt77.f90".into(), b"module packjt77".to_vec());
    assert_eq!(check(&w), Vec::<String>::new());
}

#[test]
fn the_checker_fires_on_each_kind_of_gap() {
    // Each change below must be caught: the positive controls for the test above, one or more
    // per check.

    // 1. a file in a JTTY directory that the table does not list, at any depth
    for extra in [
        "lib/jtty/portaudio.h",
        "lib/jtty/wava/tbcc_4fsk_simulation.f90",
        "tests/unit/jtty/test_jtty_structured_decode.f90",
        "tests/fixtures/jtty/other.txt",
    ] {
        let mut w = world();
        w.files.insert(extra.into(), b"x".to_vec());
        let p = check(&w);
        assert!(
            fires(
                &p,
                &format!("{extra} is in a JTTY directory but not in VENDORED")
            ),
            "{extra}: {p:?}"
        );
    }

    // 2. a table row with no file behind it
    let mut w = world();
    w.table.push(("lib/jtty/gone.f90", CODEC_BLOB));
    let p = check(&w);
    assert!(
        fires(
            &p,
            "VENDORED names lib/jtty/gone.f90, which is not in the tree"
        ),
        "{p:?}"
    );

    // 3. a file that is not upstream's: one byte changed, a byte dropped, CRLF line endings, a
    //    header added
    let crlf = String::from_utf8(CODEC.to_vec())
        .unwrap()
        .replace('\n', "\r\n");
    for changed in [
        b"module jtty_mod\nend module jtty_mud\n".to_vec(),
        CODEC[..CODEC.len() - 1].to_vec(),
        crlf.into_bytes(),
        [
            &b"! MODIFIED FOR NEXUS (KD9TAW, 2026-10-09): why\n"[..],
            CODEC,
        ]
        .concat(),
    ] {
        let mut w = world();
        w.files.insert("lib/jtty/jtty_mod.f90".into(), changed);
        let p = check(&w);
        assert!(
            fires(
                &p,
                "lib/jtty/jtty_mod.f90 is not upstream's file at 0f7d5928"
            ),
            "{p:?}"
        );
    }
    // ... and a file held to another file's pin
    let mut w = world();
    w.table[1].1 = GRAMMAR_BLOB;
    assert!(fires(&check(&w), "jtty_mod.f90 is not upstream's file"));

    // 4. a pin that is not a blob id, and a path listed twice
    for bad in [
        "b2d789f60910022e5d5d0e1fd857e628e01bf8b",
        "B2D789F60910022E5D5D0E1FD857E628E01BF8BF",
        "g2d789f60910022e5d5d0e1fd857e628e01bf8bf",
    ] {
        let mut w = world();
        w.table[1].1 = bad;
        let p = check(&w);
        assert!(fires(&p, "is not a 40-digit blob id"), "{bad}: {p:?}");
    }
    let mut w = world();
    w.table.push(("lib/jtty/jtty_mod.f90", CODEC_BLOB));
    assert!(fires(
        &check(&w),
        "VENDORED lists lib/jtty/jtty_mod.f90 twice"
    ));

    // 5. NOTICE's entry: missing, or missing the repository, the tag, the commit, the directory
    //    or a vendored file; or naming a file that is not vendored
    let mut w = world();
    w.notice = "## Upstream lineage\n".into();
    assert!(fires(&check(&w), "NOTICE has no"));
    for (part, what) in [
        (UPSTREAM, "the upstream repository"),
        ("`v3.3.0-beta4`", "the tag"),
        ("`0f7d5928`", "the commit"),
        (DIR, "the directory"),
    ] {
        let mut w = world();
        w.notice = w.notice.replace(part, "x");
        let p = check(&w);
        assert!(
            fires(&p, &format!("NOTICE's entry does not name {what}")),
            "{part}: {p:?}"
        );
    }
    for gone in ["jtty_mod.f90", "packjt77_grammar.f90"] {
        let mut w = world();
        w.notice = w.notice.replace(&format!("`{gone}`"), "it");
        let p = check(&w);
        assert!(
            fires(&p, &format!("NOTICE's entry does not name `{gone}`")),
            "{gone}: {p:?}"
        );
    }
    let mut w = world();
    w.notice = w
        .notice
        .replace("`jtty_mod.f90`", "`jtty_mod.f90`, `jtty_spec.f90`");
    assert!(fires(
        &check(&w),
        "NOTICE's entry names `jtty_spec.f90`, which is not vendored"
    ));
    // a test file is vendored too, so the entry may name it
    let mut w = world();
    w.notice = w
        .notice
        .replace("its test", "its test, `test_jtty_pack.f90`");
    assert_eq!(check(&w), Vec::<String>::new());

    // 6. the disclaimer, and the pre-release sentence on a pre-release and only on one
    let mut w = world();
    w.notice = w.notice.replace("affiliated with", "x");
    assert!(fires(&check(&w), "NOTICE's entry lacks the disclaimer"));
    let mut w = world();
    w.notice = w.notice.replace("general release carries JTTY", "x");
    assert!(fires(
        &check(&w),
        "v3.3.0-beta4 is a pre-release, but NOTICE's entry does not say"
    ));
    let mut w = world();
    w.tag = "v3.4.0";
    w.notice = w.notice.replace("v3.3.0-beta4", "v3.4.0");
    w.vendor_readme = w.vendor_readme.replace("v3.3.0-beta4", "v3.4.0");
    let p = check(&w);
    assert!(
        fires(
            &p,
            "v3.4.0 is a general release, but NOTICE's entry still says"
        ),
        "{p:?}"
    );
    w.notice = w.notice.replace(&format!("{PRE_RELEASE}."), "");
    w.readme = w.readme.replace(", from a WSJT-X pre-release", "");
    assert_eq!(check(&w), Vec::<String>::new());

    // 7. the vendor README's section: missing, or missing the tag or the commit; either list
    //    missing; a vendored file not listed as vendored, or listed as not vendored; a file
    //    listed as vendored that is not
    let mut w = world();
    w.vendor_readme = w.vendor_readme.replace(VENDOR_README_HEADING, "## JTTY");
    assert!(fires(&check(&w), "the vendor README has no"));
    for (part, what) in [
        ("`v3.3.0-beta4`", "the tag"),
        (UPSTREAM_COMMIT, "the commit"),
    ] {
        let mut w = world();
        w.vendor_readme = w.vendor_readme.replace(part, "x");
        let p = check(&w);
        assert!(
            fires(
                &p,
                &format!("the vendor README's section does not name {what}")
            ),
            "{part}: {p:?}"
        );
    }
    for list in [VENDORED_LIST, NOT_VENDORED_LIST] {
        let mut w = world();
        w.vendor_readme = w.vendor_readme.replace(&format!("{list}\n"), "Files:\n");
        let p = check(&w);
        assert!(
            fires(&p, &format!("the vendor README has no `{list}` list")),
            "{list}: {p:?}"
        );
    }
    let mut w = world();
    w.vendor_readme = w
        .vendor_readme
        .replace("- `tests/unit/jtty/test_jtty_pack.f90`\n", "");
    assert!(fires(
        &check(&w),
        "list does not name `tests/unit/jtty/test_jtty_pack.f90`"
    ));
    let mut w = world();
    w.vendor_readme = w.vendor_readme.replace(
        "- `lib/jtty/jtty_mod.f90`\n",
        "- `lib/jtty/jtty_mod.f90`\n- `lib/jtty/jtty_spec.f90`\n",
    );
    assert!(fires(
        &check(&w),
        "the vendor README lists `lib/jtty/jtty_spec.f90` as vendored, but VENDORED does not"
    ));
    let mut w = world();
    w.vendor_readme = w
        .vendor_readme
        .replace("`lib/jtty/jtty.f90`", "`lib/jtty/jtty_mod.f90`");
    assert!(fires(
        &check(&w),
        "the vendor README lists `lib/jtty/jtty_mod.f90` as not vendored, but it is"
    ));
    for (extra, name) in [
        ("lib/jtty/jtty.f90", "`lib/jtty/jtty.f90`"),
        ("lib/jtty/wava/rcu_bound_4fsk.f90", "`lib/jtty/wava/`"),
    ] {
        let mut w = world();
        w.files.insert(extra.into(), b"x".to_vec());
        let p = check(&w);
        assert!(
            fires(
                &p,
                &format!("the vendor README lists {name} as not vendored, but it is")
            ),
            "{extra}: {p:?}"
        );
    }

    // 8. the README credit: missing, or missing a name, or wrong about the pre-release
    for gone in ["WSJT-X", "JTTY", DIR] {
        let mut w = world();
        w.readme = w.readme.replace(gone, "x");
        let p = check(&w);
        assert!(
            fires(&p, "README's credits have no bullet naming WSJT-X, JTTY"),
            "{gone}: {p:?}"
        );
    }
    let mut w = world();
    w.readme = w.readme.replace(README_HEADING, "## Credits");
    assert!(fires(&check(&w), "README has no"));
    let mut w = world();
    w.readme = w.readme.replace(", from a WSJT-X pre-release", "");
    assert!(fires(
        &check(&w),
        "README's JTTY credit and the tag v3.3.0-beta4 disagree"
    ));

    // 9. each entry and section ends at its next heading: a name in the NEXT one is not this
    //    one's, either way round
    let p = check(&world());
    assert!(!fires(&p, "other.f90"), "{p:?}");
    let mut w = world();
    w.vendor_readme = w
        .vendor_readme
        .replace("- `tests/unit/jtty/test_jtty_pack.f90`\n", "")
        .replace(
            "`lib/jtty/later.f90`",
            "`tests/unit/jtty/test_jtty_pack.f90`",
        );
    assert!(fires(
        &check(&w),
        "list does not name `tests/unit/jtty/test_jtty_pack.f90`"
    ));
    let mut w = world();
    let credit = "- **WSJT-X** — the modem, and the JTTY modem sources\n  (`libtempo/vendor/wsjtx/\
                  lib/jtty/`, from a WSJT-X pre-release).\n";
    w.readme = w.readme.replace(credit, "") + credit;
    assert!(fires(
        &check(&w),
        "README's credits have no bullet naming WSJT-X, JTTY"
    ));
}
