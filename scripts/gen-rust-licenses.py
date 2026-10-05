#!/usr/bin/env python3
"""scripts/gen-rust-licenses.py — the license texts of the Rust crates the desktop app is built from.

WHAT. Writes licenses/rust/THIRD-PARTY.txt. Every installer carries it as
resources/rust/THIRD-PARTY.txt (src-tauri/tauri.conf.json), and Settings > Licenses shows it.
CI's `deny` job runs this with --check and fails when the committed file is behind the lockfile.

WHICH CRATES. Cargo's own build graph for src-tauri, from its Cargo.lock, with the features every
installer is built with, on every target one is built for: `cargo tree -e normal` per target, so
proc-macro crates and their dependencies are listed although they only run while the app compiles.
Cargo resolves those for the machine it runs on; CI's, and the reference, is Linux x86_64. Build
and dev dependencies are not listed. This repository's own crates are not third-party.

WHICH TEXT. The license files at the root of each crate's published package (and in a REUSE-style
LICENSES/ directory), less the texts of licenses Nexus does not take. A file that is no license
text at all (a copyright statement, an SPDX record) stays. Where a crate offers a choice, Nexus
takes an alternative deny.toml allows, since that list is the GPL-3.0-only compatibility decision:
the first by PREFERENCE whose text the package carries. A package that carries none takes the
first allowed alternative and the reviewed texts pinned for that crate version in
licenses/rust/upstream.json: its upstream repository's license file at the commit the package was
published from, or, where the repository has none either, the license as its steward publishes
it. Anything else stops the run, naming the crate.

usage: scripts/gen-rust-licenses.py [--check]
exit:  0 written, or current with --check · 1 stale with --check · 2 the generator could not run
"""
import difflib
import hashlib
import json
import os
import re
import subprocess
import sys
import tomllib

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
OUTPUT = os.path.join(ROOT, "licenses", "rust", "THIRD-PARTY.txt")
UPSTREAM = os.path.join(ROOT, "licenses", "rust", "upstream.json")
UPSTREAM_TEXTS = os.path.join(ROOT, "licenses", "rust", "upstream")
MANIFEST = os.path.join(ROOT, "src-tauri", "Cargo.toml")
# What every installer is built with: scripts/build-windows-cross.sh, scripts/build-linux.sh (Linux
# and the Raspberry Pi) and release.yml's macOS job all run `cargo tauri build --features
# radio,custom-protocol`, for these targets.
FEATURES = "radio,custom-protocol"
TARGETS = ["x86_64-pc-windows-gnu", "aarch64-apple-darwin", "x86_64-unknown-linux-gnu",
           "aarch64-unknown-linux-gnu"]
# The choice licenses/remote and licenses/stream already make: MIT over Apache-2.0, BSD-3-Clause
# over Apache-2.0. Licenses deny.toml allows that are not named here rank after these, in its order.
PREFERENCE = ["MIT", "BSD-3-Clause", "BSD-2-Clause", "ISC", "Zlib", "Apache-2.0"]
LICENSE_FILE = re.compile(r"^(licen[cs]e|copying|unlicense)([._-].*)?$", re.I)
NOTICE_FILE = re.compile(r"^notice([._-].*)?$", re.I)
# Phrases only a full license text carries, matched with case ignored and whitespace collapsed. A
# file is that license's text if it has all of them, so a pointer to a license is never one. The
# last four are never taken; they are here so that their texts are known as texts and left out.
FINGERPRINTS = {
    "MIT": [r"permission is hereby granted, free of charge, to any person obtaining a copy",
            r"the above copyright notice and this permission notice shall be included"],
    "Apache-2.0": [r"apache license,? version 2\.0",
                   r"terms and conditions for use, reproduction,? and distribution"],
    "BSD-3-Clause": [r"redistribution and use in source and binary forms",
                     r"endorse or promote products derived from this software"],
    "BSD-2-Clause": [r"redistribution and use in source and binary forms",
                     r"this list of conditions and the following disclaimer"],
    "ISC": [r"permission to use, copy, modify, and(/or)? distribute this software for any purpose "
            r"with or without fee is hereby granted, provided that the above copyright notice and "
            r"this permission notice appear in all copies"],
    "Zlib": [r"altered source versions must be plainly marked as such"],
    "Unicode-3.0": [r"unicode license v3"],
    "MPL-2.0": [r"mozilla public license version 2\.0"],
    "CDLA-Permissive-2.0": [r"community data license agreement . permissive . version 2\.0"],
    "Unlicense": [r"this is free and unencumbered software released into the public domain"],
    "0BSD": [r"distribute this software for any purpose with or without fee is hereby granted\. "
             r"the software is provided"],
    "BSL-1.0": [r"boost software license . version 1\.0"],
    "CC0-1.0": [r"creative commons legal code cc0 1\.0 universal"],
    "GPL-3.0": [r"gnu general public license version 3, 29 june 2007"],
}
RULE = "=" * 72


class Stop(Exception):
    pass


def cargo(*args):
    run = subprocess.run(["cargo", *args, "--manifest-path", MANIFEST, "--locked"],
                         capture_output=True, text=True)
    if run.returncode:
        raise Stop(f"cargo {args[0]} failed:\n{run.stderr.strip()}")
    return run.stdout


def identify(text):
    # A license written as a comment block (untrusted's) reads the same without its markers.
    flat = " ".join(re.sub(r"(?m)^\s*(//+|#+|/?\*+/?)", " ", text).lower().split())
    found = {lic for lic, phrases in FINGERPRINTS.items() if all(re.search(p, flat) for p in phrases)}
    if "BSD-3-Clause" in found:
        found.discard("BSD-2-Clause")
    return found


def alternatives(expression):
    """The license expression as its alternatives, each the set of licenses it requires together."""
    tokens = re.findall(r"[()]|[^\s()]+", expression.replace("/", " OR "))
    at = 0

    def term():
        nonlocal at
        token = tokens[at]
        at += 1
        if token == "(":
            inner = either()
            if tokens[at:at + 1] != [")"]:
                raise Stop(f"cannot read the license expression {expression!r}")
            at += 1
            return inner
        if tokens[at:at + 1] == ["WITH"]:
            token = f"{token} WITH {tokens[at + 1]}"
            at += 2
        return [frozenset([token])]

    def both():
        nonlocal at
        found = term()
        while tokens[at:at + 1] == ["AND"]:
            at += 1
            right = term()
            found = [a | b for a in found for b in right]
        return found

    def either():
        nonlocal at
        found = both()
        while tokens[at:at + 1] == ["OR"]:
            at += 1
            found += both()
        return found

    try:
        found = either()
    except IndexError:
        found = None
    if found is None or at != len(tokens):
        raise Stop(f"cannot read the license expression {expression!r}")
    return found


def read(path):
    with open(path, encoding="utf-8") as f:
        text = f.read()
    return text.lstrip("\ufeff").replace("\r\n", "\n").replace("\r", "\n").rstrip() + "\n"


def block(text):
    """A text as one block of an entry, which separates its blocks by exactly one blank line."""
    return text.rstrip("\n")


def version_key(version):
    return [(0, int(p), "") if p.isdigit() else (1, 0, p) for p in re.split(r"[.+-]", version)]


def shipped_crates():
    found = set()
    for target in TARGETS:
        tree = cargo("tree", "--features", FEATURES, "-e", "normal", "--target", target,
                     "--prefix", "none", "--format", "{p}")
        found |= {(m[1], m[2]) for m in re.finditer(r"(?m)^(\S+) v(\S+)", tree)}
    return found


def entry(package, allowed, rank, upstream, used):
    name, version, expression = package["name"], package["version"], package["license"]
    key = f"{name} {version}"
    if not expression:
        raise Stop(f"{key} declares no license expression")
    root = os.path.dirname(package["manifest_path"])
    texts = {f: read(os.path.join(root, f)) for f in sorted(os.listdir(root))
             if LICENSE_FILE.match(f) and os.path.isfile(os.path.join(root, f))}
    # A REUSE-style package keeps its license texts in LICENSES/ (crc-catalog's).
    if os.path.isdir(os.path.join(root, "LICENSES")):
        texts |= {f"LICENSES/{f}": read(os.path.join(root, "LICENSES", f))
                  for f in sorted(os.listdir(os.path.join(root, "LICENSES")))}
    carried = {f: identify(t) for f, t in texts.items()}
    every = alternatives(expression)
    options = sorted((alt for alt in every if alt <= allowed),
                     key=lambda alt: sorted(rank[lic] for lic in alt))
    if not options:
        raise Stop(f"{key} is licensed {expression}, and deny.toml allows none of it: NOT "
                   "compatible with GPL-3.0-only as deny.toml decides it")
    known = set().union(*carried.values())
    taken = next((alt for alt in options if alt <= known), None)
    pin = None
    if taken is None:
        taken = options[0]
        pin = pinned(package, key, taken, root, upstream, used)
    elected = " AND ".join(sorted(taken, key=rank.get))
    lines = [RULE, f"{key} — {expression}" + (f" ({elected} elected)" if len(every) > 1 else ""),
             package.get("repository") or f"https://crates.io/crates/{name}/{version}"]
    for f, ids in carried.items():
        if not ids or ids & taken:
            lines += ["", f, "", block(texts[f])]
    if pin:
        lines += ["", f"The published package carries no {elected} text. Reviewed in its place, each from "
                  "the address", "above it:"]
        for source, text in pin:
            lines += ["", source, "", block(text)]
    if "Apache-2.0" in taken:
        for f in sorted(os.listdir(root)):
            if NOTICE_FILE.match(f):
                lines += ["", f, "", block(read(os.path.join(root, f)))]
    return "\n".join(lines) + "\n"


def pinned(package, key, taken, root, upstream, used):
    pin = upstream["crates"].get(key)
    vcs = os.path.join(root, ".cargo_vcs_info.json")
    commit = json.load(open(vcs))["git"]["sha1"] if os.path.exists(vcs) else None
    repository = package.get("repository") or "its repository"
    if not pin or set(pin["license"].split(" AND ")) != set(taken):
        raise Stop(f"{key} ({package['license']}) carries no text of {' AND '.join(sorted(taken))}: "
                   f"review {repository}'s license file at commit {commit or '(unrecorded)'} and pin "
                   "it in licenses/rust/upstream.json")
    used.add(key)
    slug = re.sub(r"^https?://[^/]+/|\.git$|/$", "", package.get("repository") or "")
    found, texts = set(), []
    for f, source in pin["texts"]:
        text = read(os.path.join(UPSTREAM_TEXTS, f))
        if hashlib.sha256(text.encode()).hexdigest() != upstream["sha256"].get(f):
            raise Stop(f"the reviewed text {f} changed, or has no sha256 in upstream.json")
        if commit and slug and slug in source and f"/{commit}/" not in source:
            raise Stop(f"{key}: {source} is not at the commit the package was published from, {commit}")
        found |= identify(text)
        texts.append((source, text))
    if not set(taken) <= found:
        raise Stop(f"the reviewed texts for {key} carry no text of {pin['license']}")
    return texts


def generate():
    with open(os.path.join(ROOT, "deny.toml"), "rb") as f:
        allowed = tomllib.load(f)["licenses"]["allow"]
    rank = {lic: i for i, lic in enumerate(PREFERENCE + [a for a in allowed if a not in PREFERENCE])}
    with open(UPSTREAM) as f:
        upstream = json.load(f)
    metadata = json.loads(cargo("metadata", "--format-version", "1", "--features", FEATURES))
    packages = {(p["name"], p["version"]): p for p in metadata["packages"]}
    shipped = shipped_crates()
    unknown = sorted(f"{n} {v}" for n, v in shipped - set(packages))
    if unknown:
        raise Stop(f"cargo tree lists crates cargo metadata does not: {', '.join(unknown)}")
    # A package with no source is one of this repository's own crates.
    crates = sorted((packages[c] for c in shipped if packages[c]["source"]),
                    key=lambda p: (p["name"], version_key(p["version"])))
    used, entries, problems = set(), [], []
    for package in crates:
        try:
            entries.append(entry(package, set(allowed), rank, upstream, used))
        except Stop as problem:
            problems.append(str(problem))
    unused = sorted(set(upstream["crates"]) - used)
    if unused:
        problems.append(f"licenses/rust/upstream.json pins crates that no longer need it: "
                        f"{', '.join(unused)}")
    if problems:
        raise Stop("\n".join(problems))
    head = [
        "Nexus — third-party notices for the app's Rust crates",
        "",
        f"The desktop app is built from the {len(crates)} Rust crates below: every crate cargo compiles for it on",
        "Windows, macOS, Linux and the Raspberry Pi, including the proc-macro crates that run only while",
        "it compiles. Each is followed by the license files its published package carries, less the",
        "texts of licenses Nexus does not take. Where a crate offers a choice, the entry names the",
        "license elected: the first of MIT, BSD-3-Clause, BSD-2-Clause, ISC, Zlib and Apache-2.0 whose",
        "text the package carries. Every license taken is compatible with GPL-3.0-only. A package that",
        "carries no text is followed by reviewed texts, each under the address it came from.",
        "These notices supplement Nexus COPYING and NOTICE.",
        "",
    ]
    return "\n".join(head) + "\n" + "\n".join(entries)


def main(argv):
    if argv[1:] not in ([], ["--check"]):
        print(__doc__.split("\n\n")[-1])
        return 2
    try:
        text = generate()
    except (Stop, OSError, UnicodeDecodeError, ValueError, KeyError) as error:
        print(f"gen-rust-licenses: {error}")
        return 2
    relative = os.path.relpath(OUTPUT, ROOT)
    if argv[1:] == ["--check"]:
        with open(OUTPUT, encoding="utf-8", newline="") as f:
            committed = f.read()
        if committed == text:
            print(f"{relative} is current")
            return 0
        sys.stdout.writelines(difflib.unified_diff(committed.splitlines(True), text.splitlines(True),
                                                   f"{relative} (committed)", f"{relative} (generated)"))
        print(f"\n{relative} is stale: run scripts/gen-rust-licenses.py and commit the result")
        return 1
    with open(OUTPUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    print(f"wrote {relative}: {len(text.encode())} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
