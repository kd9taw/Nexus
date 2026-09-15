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

    /// The helper hands back an ordinary `Command` for the program it was given.
    #[test]
    fn command_keeps_the_program() {
        let cmd = super::command("rigctld");
        assert_eq!(cmd.get_program(), "rigctld");
        assert_eq!(cmd.get_args().count(), 0);
    }
}
