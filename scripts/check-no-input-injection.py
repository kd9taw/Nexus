#!/usr/bin/env python3
"""scripts/check-no-input-injection.py — security test A2 for Remote as a stream: no OS input.

WHY. A streamed operator's clicks and keys reach the station's own Nexus webview as DOM events,
and nothing else on the station PC (S11). The Win32 calls that would turn them into operating-
system input, reaching whatever window has focus, are SendInput, keybd_event and mouse_event.
This check fails if anything compiled into the Windows build calls one of them, apart from the
reviewed callers named below.

WHY NOT A BINARY CHECK. The plan asked for "no SendInput in the binary", and that cannot hold:
Tauri's own windowing crates already call it, so Nexus.exe imports SendInput whatever Nexus
does (measured 2026-09-27, tao 0.35.3 and muda 0.19.2). An import table cannot tell their call
from a new one, so this reads the source of everything the Windows build compiles instead:
every source file in this repository that git does not ignore, and every package `cargo metadata` resolves for
x86_64-pc-windows-gnu from src-tauri.

REVIEWED CALLERS, each pinned to its file and its number of calls, so a new call anywhere, even
in one of these crates, fails the check and gets looked at:
  tao   src/platform_impl/windows/window.rs  force_window_active(): an Alt press and release so
        that a window Tauri creates can take the foreground. Not reachable from a stream.
  muda  src/platform_impl/windows/mod.rs     the predefined Edit-menu items send their own
        Ctrl shortcut (Copy, Paste, ...). A local menu click at the station, not a stream path.
BINDINGS, which declare the functions and call nothing: windows, windows-sys, winapi.

usage: scripts/check-no-input-injection.py [--self-test]
exit:  0 clean · 1 a call found · 2 the check could not run
"""
import json
import os
import re
import subprocess
import sys
import tempfile

CALL = re.compile(r"\b(SendInput|keybd_event|mouse_event)\s*\(|::\s*(SendInput|keybd_event|mouse_event)\b")
SOURCE = (".rs", ".c", ".cc", ".cpp", ".cxx", ".h", ".hpp")
TARGET = "x86_64-pc-windows-gnu"
BINDINGS = {"windows", "windows-sys", "winapi"}
REVIEWED = {
    "tao": {"src/platform_impl/windows/window.rs": 1},
    "muda": {"src/platform_impl/windows/mod.rs": 1},
}


def calls(path):
    """The lines of one file that call (or name, by path) an input-injection function."""
    try:
        with open(path, encoding="utf-8", errors="replace") as fh:
            lines = fh.read().splitlines()
    except OSError:
        return []
    found = []
    for number, line in enumerate(lines, 1):
        text = line.strip()
        # A comment that names the function is documentation, not a call.
        if text.startswith(("//", "/*", "*")):
            continue
        if CALL.search(line):
            found.append((number, text[:120]))
    return found


def scan_tree(root, files=None):
    """{relative path: [(line, text)]} for every source file under root (or the given list)."""
    hits = {}
    if files is None:
        files = []
        for directory, _, names in os.walk(root):
            files.extend(os.path.join(directory, n) for n in names)
    for path in files:
        if not path.endswith(SOURCE):
            continue
        found = calls(path)
        if found:
            hits[os.path.relpath(path, root)] = found
    return hits


def judge_package(name, hits):
    """Problems with one dependency's calls: none for a binding, or a reviewed caller as pinned."""
    if not hits or name in BINDINGS:
        return []
    pinned = REVIEWED.get(name)
    if pinned is None:
        return [f"{name}: {path}:{n}: {text}" for path, lines in hits.items() for n, text in lines]
    problems = []
    for path, lines in hits.items():
        if len(lines) != pinned.get(path, 0):
            problems.extend(f"{name}: {path}:{n}: {text} (reviewed: {pinned})" for n, text in lines)
    return problems


def repository_problems(repo):
    tracked = subprocess.run(
        # Tracked files, and new ones not yet added (a local run before a commit); never ignored ones.
        ["git", "-C", repo, "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        check=True, capture_output=True,
    ).stdout.decode().split("\0")
    this = os.path.relpath(os.path.abspath(__file__), repo)
    files = [os.path.join(repo, f) for f in tracked if f and f != this]
    hits = scan_tree(repo, files)
    read = sum(1 for f in files if f.endswith(SOURCE))
    return [f"{path}:{n}: {text}" for path, lines in hits.items() for n, text in lines], read


def dependency_problems(repo):
    meta = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked",
         "--manifest-path", os.path.join(repo, "src-tauri", "Cargo.toml"),
         "--filter-platform", TARGET],
        check=True, capture_output=True,
    ).stdout
    data = json.loads(meta)
    packages = {p["id"]: p for p in data["packages"]}
    problems, scanned = [], 0
    for node in data["resolve"]["nodes"]:
        package = packages[node["id"]]
        if package["source"] is None:
            continue  # this repository's own crates: covered by the tracked-file scan
        scanned += 1
        root = os.path.dirname(package["manifest_path"])
        problems.extend(judge_package(package["name"], scan_tree(root)))
    return problems, scanned


def self_test():
    """The check must find a planted call, in Nexus's code and in a dependency, and must not
    mistake a comment for one."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        planted = {
            "planted.rs": "use windows::Win32::UI::Input::KeyboardAndMouse::SendInput;\n"
                          "fn inject(i: &[INPUT]) { unsafe { SendInput(i, 40) }; }\n",
            "planted.c": "void click(void) { mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0); }\n",
            "planted.cpp": "void key() { keybd_event(VK_RETURN, 0, 0, 0); }\n",
            "quiet.rs": "// This path never calls SendInput(...) or mouse_event(...).\n"
                        "/// Nothing here reaches `keybd_event`.\nfn quiet() {}\n",
        }
        for name, text in planted.items():
            with open(os.path.join(tmp, name), "w") as fh:
                fh.write(text)
        hits = scan_tree(tmp)
        for name in ("planted.rs", "planted.c", "planted.cpp"):
            if name not in hits:
                failures.append(f"missed the planted call in {name}")
        if len(hits.get("planted.rs", [])) != 2:
            failures.append(f"planted.rs: expected the import and the call, got {hits.get('planted.rs')}")
        if "quiet.rs" in hits:
            failures.append(f"took a comment for a call: {hits['quiet.rs']}")
        # A dependency: an unknown crate calling it fails; a binding and the pinned callers pass;
        # a second call in a reviewed crate fails.
        one = {"src/platform_impl/windows/window.rs": [(1524, "SendInput(&inputs, 40);")]}
        if not judge_package("evil-crate", one):
            failures.append("an unknown crate's call passed")
        if judge_package("windows", one) or judge_package("tao", one):
            failures.append("a binding or the reviewed tao call failed")
        two = {"src/platform_impl/windows/window.rs": [(1, "SendInput(a)"), (2, "SendInput(b)")]}
        if not judge_package("tao", two):
            failures.append("a second call in tao passed")
    return failures


def main(argv):
    repo = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
    if argv[1:] == ["--self-test"]:
        failures = self_test()
        for f in failures:
            print(f"SELF-TEST FAILED: {f}")
        if not failures:
            print("self-test: planted calls found in Rust, C and C++, a comment passed, an "
                  "unknown crate's call refused, a second call in a reviewed crate refused")
        return 1 if failures else 0
    if argv[1:]:
        print(__doc__.split("\n\n")[-1])
        return 2
    try:
        problems, read = repository_problems(repo)
        dependency, scanned = dependency_problems(repo)
    except (OSError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        print(f"could not run the check: {error}")
        return 2
    problems += dependency
    for p in problems:
        print(f"INPUT INJECTION: {p}")
    if problems:
        print(f"{len(problems)} call(s) that would inject operating-system input. A streamed "
              "operator's input must reach Nexus's own webview only (S11).")
        return 1
    print(f"no input-injection call in this repository's {read} source files or in the "
          f"{scanned} dependencies of the {TARGET} build, beyond the reviewed tao and muda calls")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
