//! The one way Nexus starts a child process.
//!
//! **Why this exists.** Nexus is a GUI program on Windows (`windows_subsystem = "windows"`), and a
//! GUI program that starts a CONSOLE program — `rigctld`, `tasklist`, `w32tm`, `reg`, PowerShell —
//! makes Windows open a fresh console window for it unless the spawn says `CREATE_NO_WINDOW`. The
//! window lives exactly as long as the child, so a quick query shows up as a command-prompt box
//! that flashes on screen and vanishes.
//!
//! That is the shape of the Windows reports: three or four such windows at a time. The clock
//! diagnosis added in 1.12.0 runs `tasklist`, `sc`, `w32tm` and `reg` twice, one after the other,
//! a few seconds after launch and again every ten minutes, and that call site had no flag. The
//! daemon spawns beside it all set the flag, each with its own copy of the same five lines.
//! Getting it right depended on every new call site remembering those lines. A comment in
//! `rigctld_proc` even claimed "every spawn already carries CREATE_NO_WINDOW — verified site by
//! site", and it stopped being true the day the clock code landed.
//!
//! So the flag lives HERE, once, and [`tests::no_raw_process_spawns_outside_this_module`] fails
//! the build for any `Command::new` in shipped source outside this file. Test code and `build.rs`
//! are exempt: neither runs inside the operator's GUI process.
//!
//! **That guard covers our half of the process, and the flashes did not stop.** The next one
//! came from `tract-linalg`, below the AI CW decoder, which probes the CPU cache by running
//! `wmic` — a dependency's spawn, which no scan of `crates/` could see and no flag of ours can
//! reach. [`tests::no_unreviewed_dependency_process_spawns`] runs the same detector over every
//! crate in the lockfiles and makes each spawn it finds a written-down verdict; that one's
//! mitigation is `deepcw::warm_cpu_cache_probe`.
//!
//! On a GUI-subsystem child (TQSL, Nexus relaunching itself) the flag is ignored by Windows, so
//! routing those through here changes nothing. Off Windows it is a plain `Command::new`.

use std::ffi::OsStr;
use std::process::Command;

/// Windows `CREATE_NO_WINDOW` process-creation flag: run a console program with no console.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// `std::process::Command::new(program)`, with no console window on Windows.
///
/// Everything else — args, env, cwd, stdio — is left to the caller exactly as `Command::new`
/// leaves it. A caller that needs more creation flags must OR them with `CREATE_NO_WINDOW`,
/// because `creation_flags` replaces the value rather than adding to it.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)] // mutated only on Windows
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    /// Blank every comment, string literal and char literal in `src` to spaces, keeping newlines,
    /// so a scan sees only code and line numbers still line up. `Command::new` inside a doc
    /// comment (there is one in `rigctld_proc`) or a string is not a spawn.
    fn code_only(src: &str) -> String {
        let c: Vec<char> = src.chars().collect();
        let mut out: Vec<char> = c.clone();
        let blank = |out: &mut Vec<char>, from: usize, to: usize| {
            for ch in out.iter_mut().take(to).skip(from) {
                if *ch != '\n' {
                    *ch = ' ';
                }
            }
        };
        let ident = |ch: char| ch.is_alphanumeric() || ch == '_';
        let mut i = 0;
        while i < c.len() {
            let prev_ident = i > 0 && ident(c[i - 1]);
            if c[i] == '/' && c.get(i + 1) == Some(&'/') {
                let end = c[i..]
                    .iter()
                    .position(|&x| x == '\n')
                    .map_or(c.len(), |p| i + p);
                blank(&mut out, i, end);
                i = end;
            } else if c[i] == '/' && c.get(i + 1) == Some(&'*') {
                let (mut depth, mut j) = (1, i + 2);
                while j < c.len() && depth > 0 {
                    if c[j] == '/' && c.get(j + 1) == Some(&'*') {
                        depth += 1;
                        j += 2;
                    } else if c[j] == '*' && c.get(j + 1) == Some(&'/') {
                        depth -= 1;
                        j += 2;
                    } else {
                        j += 1;
                    }
                }
                blank(&mut out, i, j);
                i = j;
            } else if c[i] == 'r'
                && (!prev_ident || (c[i - 1] == 'b' && !(i > 1 && ident(c[i - 2]))))
                && matches!(c.get(i + 1), Some('"') | Some('#'))
            {
                // Raw string r"…" / r#"…"# / br"…".
                let hashes = c[i + 1..].iter().take_while(|&&x| x == '#').count();
                if c.get(i + 1 + hashes) != Some(&'"') {
                    i += 1;
                    continue;
                }
                let close: String = std::iter::once('"')
                    .chain(std::iter::repeat_n('#', hashes))
                    .collect();
                let body_start = i + 2 + hashes;
                let rest: String = c[body_start..].iter().collect();
                let end = rest.find(&close).map_or(c.len(), |p| {
                    body_start + rest[..p].chars().count() + close.len()
                });
                blank(&mut out, i, end);
                i = end;
            } else if c[i] == '"' {
                let mut j = i + 1;
                while j < c.len() && c[j] != '"' {
                    j += if c[j] == '\\' { 2 } else { 1 };
                }
                blank(&mut out, i, (j + 1).min(c.len()));
                i = j + 1;
            } else if c[i] == '\'' {
                // A char literal ('x', '\n', '\u{1F4E1}'), or a lifetime ('a) which is left alone.
                if c.get(i + 1) == Some(&'\\') && i + 3 <= c.len() {
                    // Search past the escaped character itself, which may be a `'`.
                    let end = c[i + 3..]
                        .iter()
                        .position(|&x| x == '\'')
                        .map_or(c.len(), |p| i + 3 + p + 1);
                    blank(&mut out, i, end);
                    i = end;
                } else if c.get(i + 2) == Some(&'\'') {
                    blank(&mut out, i, i + 3);
                    i += 3;
                } else {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
        out.into_iter().collect()
    }

    /// Does this attribute body (`cfg(test)`, `cfg(all(test, unix))`, …) compile only under test?
    fn is_test_cfg(attr: &str) -> bool {
        let a: String = attr.chars().filter(|ch| !ch.is_whitespace()).collect();
        a == "cfg(test)"
            || (a.starts_with("cfg(all(")
                && a.split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                    .any(|tok| tok == "test")
                && !a.contains("not("))
    }

    /// Blank every item carrying a test-only `#[cfg(…)]` in already-[`code_only`] text: the
    /// attribute through the end of the item it gates (`mod tests { … }`, `fn …{ … }`, `mod x;`,
    /// a `field,`). Stops at the first `;` or `,` at bracket depth 0, at the close of a `{` block
    /// opened at depth 0, or at an unmatched closer (the end of the enclosing block).
    fn strip_test_items(code: &str) -> String {
        let c: Vec<char> = code.chars().collect();
        let mut out = c.clone();
        let mut i = 0;
        while i + 1 < c.len() {
            if !(c[i] == '#' && c[i + 1] == '[') {
                i += 1;
                continue;
            }
            // The attribute body, up to its matching `]`.
            let (mut depth, mut j) = (1, i + 2);
            while j < c.len() && depth > 0 {
                match c[j] {
                    '[' => depth += 1,
                    ']' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            let body: String = c[i + 2..j.saturating_sub(1)].iter().collect();
            if !is_test_cfg(&body) {
                i = j;
                continue;
            }
            // The gated item.
            let mut depth = 0i32;
            let mut k = j;
            while k < c.len() {
                match c[k] {
                    '(' | '[' => depth += 1,
                    ')' | ']' if depth > 0 => depth -= 1,
                    '{' => {
                        depth += 1;
                    }
                    '}' if depth == 1 => {
                        // A brace block opened at item level just closed: the item is done
                        // unless brackets opened before it are still open (they are not, since
                        // `(`/`[` must balance before a body brace).
                        k += 1;
                        break;
                    }
                    '}' if depth > 1 => depth -= 1,
                    ')' | ']' | '}' => break, // unmatched: the enclosing block ends here
                    ';' | ',' if depth == 0 => {
                        k += 1;
                        break;
                    }
                    _ => {}
                }
                k += 1;
            }
            for ch in out.iter_mut().take(k).skip(i) {
                if *ch != '\n' {
                    *ch = ' ';
                }
            }
            i = k;
        }
        out.into_iter().collect()
    }

    /// The 1-based line of every raw process spawn in shipped `src`: `Command::new` (std's or
    /// tokio's, however it is qualified) or a renamed `Command` import that would hide one.
    fn raw_spawn_lines(src: &str) -> Vec<usize> {
        let code = strip_test_items(&code_only(src));
        // Squash ALL whitespace, newlines included, so `Command\n    ::new` is still caught; keep
        // each surviving char's line so the report points at where the match starts.
        let mut squashed = String::new();
        let mut line_of = Vec::new();
        let mut line = 1;
        for ch in code.chars() {
            if ch == '\n' {
                line += 1;
            } else if !ch.is_whitespace() {
                squashed.push(ch);
                line_of.push(line);
            }
        }
        let chars: Vec<char> = squashed.chars().collect();
        let mut lines = Vec::new();
        for pat in ["Command::new", "process::Commandas"] {
            let p: Vec<char> = pat.chars().collect();
            for start in 0..chars.len().saturating_sub(p.len() - 1) {
                if chars[start..start + p.len()] == p[..] {
                    lines.push(line_of[start]);
                }
            }
        }
        lines.sort_unstable();
        lines.dedup();
        lines
    }

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                rust_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }

    /// Every shipped source file: each workspace crate's `src/`, and `src-tauri/src/` (which is
    /// not a workspace member, so nothing else would scan it). `build.rs`, `tests/`, `examples/`
    /// and `benches/` sit outside `src/` and are exempt by construction.
    fn shipped_sources(repo: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        if let Ok(crates) = std::fs::read_dir(repo.join("crates")) {
            for krate in crates.flatten() {
                rust_files(&krate.path().join("src"), &mut files);
            }
        }
        rust_files(&repo.join("src-tauri").join("src"), &mut files);
        files
    }

    /// THE GUARD. A raw `Command::new` in shipped code is how the 1.12.0 console flash shipped:
    /// the flag was right at five call sites and missing at the sixth.
    #[test]
    fn no_raw_process_spawns_outside_this_module() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let this = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/process.rs");
        let files = shipped_sources(&repo);
        // A scan that found no files would pass while checking nothing. Prove it reached both
        // trees, including the one outside the workspace.
        let names: Vec<String> = files
            .iter()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        for must in [
            "src-tauri/src/lib.rs",
            "crates/tempo-audio/src/clockdiag.rs",
            "crates/tempo-audio/src/rigctld_proc.rs",
        ] {
            assert!(
                names.iter().any(|n| n.ends_with(must)),
                "the spawn guard did not scan {must} — it is checking the wrong tree"
            );
        }
        let this = this.canonicalize().expect("process.rs");
        let mut offenders = Vec::new();
        for f in &files {
            if f.canonicalize().ok().as_deref() == Some(this.as_path()) {
                continue;
            }
            let src = std::fs::read_to_string(f).expect("read source");
            for line in raw_spawn_lines(&src) {
                offenders.push(format!("{}:{line}", f.display()));
            }
        }
        assert!(
            offenders.is_empty(),
            "raw process spawn(s) in shipped code — on Windows each one flashes a console window. \
             Use `tempo_core::process::command(program)` instead of `Command::new(program)`:\n  {}",
            offenders.join("\n  ")
        );
    }

    /// The guard's scanner fires on a raw spawn and ignores the places a spawn is allowed or is
    /// not a spawn at all. One direction alone would be half a test.
    #[test]
    fn the_scanner_fires_on_raw_spawns_and_only_on_those() {
        // Fires: every spelling that creates a process without the helper.
        let planted = r#####"
use std::process::Command;
fn a() { let _ = Command::new("tasklist.exe").output(); }
fn b() { let _ = std::process::Command::new("w32tm.exe").status(); }
async fn c() { let _ = tokio::process::Command::new("reg.exe").output().await; }
use std::process::Command as Spawn;
fn d() { let _ = std::process::Command
    ::new("sc.exe"); }
"#####;
        assert_eq!(raw_spawn_lines(planted), vec![3, 4, 5, 6, 7]);

        // Silent: test-gated code, comments, strings, and the helper itself.
        let allowed = r#####"
/// `Command::new("rigctld")` from inside Nexus fails — a doc comment, not a spawn.
fn shipped() {
    let _ = tempo_core::process::command("rigctld");
    let s = "Command::new(x)"; /* Command::new */ let r = r#"Command::new("y")"#;
    let ch = '"'; let q = '\''; let _ = (s, r, ch, q);
    let life: &'static str = "x";
}
#[cfg(test)]
fn helper_for_tests() { let _ = std::process::Command::new("sh"); }
struct S { #[cfg(test)] probe: u32, kept: u32 }
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t() { let _ = std::process::Command::new("sh").args(["-c", "{"]).status(); }
}
#[cfg(all(test, unix))]
mod unix_tests { fn u() { let _ = Command::new("sleep"); } }
"#####;
        assert_eq!(raw_spawn_lines(allowed), Vec::<usize>::new());

        // Code AFTER a test module is scanned again: the skip ends where the module ends.
        let after = "#[cfg(test)]\nmod tests { fn t() {} }\nfn later() { Command::new(\"x\"); }\n";
        assert_eq!(raw_spawn_lines(after), vec![3]);
        // `not(test)` is shipped code.
        let not_test = "#[cfg(not(test))]\nfn f() { Command::new(\"x\"); }\n";
        assert_eq!(raw_spawn_lines(not_test), vec![2]);
    }

    // ── The dependency half of the same class ────────────────────────────────────────────
    //
    // `no_raw_process_spawns_outside_this_module` scans OUR source, and that is all it can
    // ever see. It was written after the 1.12.0 clock-diagnosis flash, it was right about
    // that flash, and it was still looking in the wrong half of the process when the next
    // report arrived: `tract-linalg`, three crates below the AI CW decoder, probes the CPU
    // cache by running `wmic` with no CREATE_NO_WINDOW. Nothing in `crates/` or
    // `src-tauri/src` says so, so nothing in the first guard could.
    //
    // A console window does not care whose code opened it. The scan below therefore runs
    // the same detector over every third-party crate in the lockfiles and demands that each
    // spawn it finds has been looked at by a person and written down here.

    /// Every child-process spawn in a locked dependency's shipped source, with the verdict
    /// that admitted it. Keyed by crate NAME and file path only: a version bump should not
    /// churn this list, but a crate that starts spawning from a new file must be reviewed.
    ///
    /// **Adding a line here is the review.** For each, answer: can this run inside the
    /// operator's GUI process, and if it can, is it a console program on Windows? If both,
    /// it is a flash and it needs a mitigation, not an entry.
    const REVIEWED_DEPENDENCY_SPAWNS: &[&str] = &[
        // ⚠️ THE ONE THAT REACHES AN OPERATOR: `wmic cpu get L2CacheSize,L3CacheSize` with no
        // CREATE_NO_WINDOW, to size matmul cache blocking. Still there in 0.23.7, the latest
        // published version. It runs on the AI CW decode thread — `tract` enters this tree
        // through `deepcw` and nothing else — which is why both console-flash reports named the
        // CW screen. Memoised in a `OnceLock`, so it is one flash per launch. NOT admitted:
        // mitigated by `deepcw::warm_cpu_cache_probe`, which settles the probe at startup with
        // the WBEM directory off `PATH`, so the spawn fails instead of opening a window. If this
        // line ever stops matching, upstream changed — re-read the probe, and delete the
        // mitigation with it.
        "tract-linalg src/cache.rs",
        // Windows, and harmless: `open` sets CREATE_NO_WINDOW on every command it builds.
        "open src/windows.rs",
        // Relaunches Nexus itself on an explicit restart. A GUI-subsystem child gets no console
        // either way, so the flag would be a no-op.
        "tauri src/process.rs",
        // Runtime process APIs whose program comes from the caller — they spawn nothing of their
        // own. Nexus makes no such call; the first guard in this file is what proves that.
        "async-process src/lib.rs",
        "tokio src/process/mod.rs",
        "zbus src/abstractions/process.rs",
        // Other targets: never compiled into a Windows build.
        "auto-launch src/macos.rs",
        "open src/haiku.rs",
        "open src/ios.rs",
        "open src/macos.rs",
        "open src/redox.rs",
        "open src/unix.rs",
        "open src/wsl.rs",
        "redox_users src/lib.rs",
        "tokio src/process/unix/pidfd_reaper.rs",
        // pkexec / sudo / zenity / kdialog, in the Linux+BSD install arm. The Windows arm ends
        // in `ShellExecuteW`, which is the thing src-tauri's updater notes already describe.
        "tauri-plugin-updater src/updater.rs",
        // Compile time only — build scripts, their helpers, and proc-macro crates. None of this
        // code is linked into the application binary.
        "autocfg src/rustc.rs",
        "bindgen features.rs",
        "bindgen lib.rs",
        "cargo_metadata src/lib.rs",
        "cc src/lib.rs",
        "cc src/tool.rs",
        "clang-sys src/support.rs",
        "cmake src/lib.rs",
        "embed-resource src/non_windows.rs",
        "embed-resource src/windows_msvc.rs",
        "embed-resource src/windows_not_msvc.rs",
        "find-msvc-tools src/find_tools.rs",
        "find-msvc-tools src/tool.rs",
        // A build script that is not named `build.rs`, so the directory filter cannot spot it.
        "libdbus-sys build_vendored.rs",
        "pkg-config src/lib.rs",
        // Included by its own build script.
        "portable-atomic version.rs",
        "proc-macro-crate src/lib.rs",
        "rustc_version src/lib.rs",
        "tauri-macros src/command/wrapper.rs",
        "version_check src/lib.rs",
        // Test-support code this scanner's `#[cfg(test)]` stripper cannot see, because the gate
        // is a directory layout or a cargo feature rather than an attribute. `nix` keeps its
        // integration tests in `test/`, not `tests/`.
        "native-tls src/test.rs",
        "nix test/test_kmod/mod.rs",
        "nix test/test_mount.rs",
        "nix test/test_unistd.rs",
        "rusty-fork src/fork.rs",
        "windows-implement src/tests.rs",
    ];

    /// `$CARGO_HOME/registry/src/<index>/` — where cargo extracts dependency sources.
    fn registry_src_roots() -> Vec<PathBuf> {
        let home = std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")));
        let Some(src) = home.map(|h| h.join("registry").join("src")) else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(&src) else {
            return Vec::new();
        };
        entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect()
    }

    /// `(name, version)` for every package in a `Cargo.lock`. Both lockfiles matter: the
    /// desktop shell is its own workspace, so the crates that only IT pulls in (tauri, the
    /// updater, the opener) appear in nothing the workspace lock lists.
    fn locked_packages(repo: &Path) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for lock in ["Cargo.lock", "src-tauri/Cargo.lock"] {
            let Ok(text) = std::fs::read_to_string(repo.join(lock)) else {
                continue;
            };
            let (mut name, mut version) = (None, None);
            for line in text.lines() {
                let value = |l: &str, key: &str| {
                    l.strip_prefix(key)
                        .map(|v| v.trim().trim_matches('"').to_string())
                };
                if line.starts_with("[[package]]") {
                    (name, version) = (None, None);
                } else if let Some(v) = value(line, "name =") {
                    name = Some(v);
                } else if let Some(v) = value(line, "version =") {
                    version = Some(v);
                }
                if let (Some(n), Some(v)) = (&name, &version) {
                    out.push((n.clone(), v.clone()));
                    (name, version) = (None, None);
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// Shipped `.rs` files of an extracted dependency: everything under the crate root
    /// except the places whose code cannot run inside the operator's process. `build.rs`
    /// and `build/` run at compile time; `tests/`, `benches/` and `examples/` are not
    /// compiled into the library at all.
    fn dependency_sources(krate: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(krate) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if p.is_dir() {
                if !matches!(name.as_str(), "tests" | "benches" | "examples" | "build") {
                    dependency_sources(&p, out);
                }
            } else if name.ends_with(".rs") && name != "build.rs" {
                out.push(p);
            }
        }
    }

    /// THE SECOND GUARD. A dependency that spawns a child process spawns it inside Nexus,
    /// and on Windows a console child with no `CREATE_NO_WINDOW` is a command-prompt window
    /// on the operator's screen. We cannot pass those spawns a flag, so the least we do is
    /// know about every one of them.
    #[test]
    fn no_unreviewed_dependency_process_spawns() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let roots = registry_src_roots();
        assert!(
            !roots.is_empty(),
            "no cargo registry source directory found (CARGO_HOME / HOME) — this guard \
             would pass while checking nothing"
        );
        let packages = locked_packages(&repo);
        assert!(
            packages.len() > 100,
            "read {} packages from the lockfiles; that is not a parse, it is a misread",
            packages.len()
        );

        let mut found: Vec<(String, String)> = Vec::new();
        let mut scanned: Vec<String> = Vec::new();
        for (name, version) in &packages {
            let dir = roots
                .iter()
                .map(|r| r.join(format!("{name}-{version}")))
                .find(|d| d.is_dir());
            let Some(dir) = dir else { continue };
            scanned.push(name.clone());
            let mut files = Vec::new();
            dependency_sources(&dir, &mut files);
            for f in files {
                let Ok(src) = std::fs::read_to_string(&f) else {
                    continue;
                };
                // Cheap gate first: the detector below builds char vectors, and 18 000 files
                // of dependency source is not something to do that to.
                if !src.contains("Command::new") || raw_spawn_lines(&src).is_empty() {
                    continue;
                }
                let rel = f
                    .strip_prefix(&dir)
                    .unwrap_or(&f)
                    .to_string_lossy()
                    .replace('\\', "/");
                found.push((name.clone(), rel));
            }
        }
        found.sort();
        found.dedup();

        // THE POSITIVE CONTROL, and it is a live one rather than a planted string:
        // `tract-linalg` really does spawn `wmic`, so a scan that cannot see it is broken or
        // reached nothing — the reassuring answer this guard exists to refuse.
        assert!(
            found
                .iter()
                .any(|(n, p)| n == "tract-linalg" && p == "src/cache.rs"),
            "the dependency scan did not find tract-linalg's known `wmic` spawn \
             ({} crates scanned). Either the sources are not extracted — build the \
             workspace first, `cargo test --workspace` — or the scanner is broken.",
            scanned.len()
        );

        let new: Vec<String> = found
            .iter()
            .map(|(n, p)| format!("{n} {p}"))
            .filter(|f| !REVIEWED_DEPENDENCY_SPAWNS.contains(&f.as_str()))
            .collect();
        let gone: Vec<String> = REVIEWED_DEPENDENCY_SPAWNS
            .iter()
            .filter(|r| {
                let (krate, path) = r.split_once(' ').expect("`<crate> <path>` per entry");
                // Silent about a crate whose source is not extracted: that is this machine
                // having built less, not upstream having changed. The control above is what
                // refuses a scan that reached nothing at all.
                scanned.iter().any(|s| s == krate)
                    && !found.iter().any(|(n, p)| n == krate && p == path)
            })
            .map(|r| r.to_string())
            .collect();
        assert!(
            new.is_empty(),
            "unreviewed child-process spawn(s) in dependency source. On Windows a console \
             child spawned without CREATE_NO_WINDOW flashes a command-prompt window inside \
             Nexus, and we cannot pass a dependency's spawn a flag. Decide for each whether it \
             can run in the GUI process, then add it to REVIEWED_DEPENDENCY_SPAWNS with the \
             verdict as a comment:\n  {}",
            new.join("\n  ")
        );
        assert!(
            gone.is_empty(),
            "reviewed dependency spawn(s) no longer exist — the upstream code changed, so \
             the verdict (and any mitigation written for it) is out of date:\n  {}",
            gone.join("\n  ")
        );
    }

    /// The helper hands back an ordinary `Command` for the program it was given.
    #[test]
    fn command_keeps_the_program() {
        let cmd = super::command("rigctld");
        assert_eq!(cmd.get_program(), "rigctld");
        assert_eq!(cmd.get_args().count(), 0);
    }
}
