//! The Engine lock is taken through `EngineGuard` everywhere outside tests — the scan that keeps
//! the logbook fence's count honest (SPEC-1 C11).
//!
//! `tempo_core::logbook::io_fence` proves the logbook's disk work never runs under the Engine
//! lock by counting the `EngineGuard`s alive on a thread. A raw `Mutex::lock()` or `try_lock()`
//! on the Engine mutex takes the same lock and is invisible to that count, so disk work under it
//! would pass the fence. Production code therefore takes the lock through `engine_lock`,
//! `engine_lock_result` or `engine_try_lock`, and this scan fails on anything else.
//!
//! # What it reads
//!
//! Every `.rs` file under the `src/` of tempo-app, tempo-audio and src-tauri — the crates that
//! can name the Engine mutex — minus test code: the files test modules are made of (`tests.rs`,
//! `*_tests.rs`, `*_test.rs`, anything under a `tests/` folder) and every item under
//! `#[cfg(test)]`. Test code may take the lock raw: its setup and inspection are not the app's
//! locking, and the fence still counts every acquisition the production code a test drives makes.
//!
//! # How it knows the Engine mutex, from text
//!
//! By type: a name declared with a type naming `SharedEngine` or a `Mutex<…Engine>` (a
//! parameter, a `let`, a closure parameter, a struct field); a name bound from one of those by a
//! move, a borrow, a `clone()` or an `Arc::clone`; and an expression that names `SharedEngine`
//! itself (`app.state::<SharedEngine>()`). Comments and literals are blanked first, so a doc line
//! quoting `engine.lock()` is not a finding. The three guard constructors are the one place a
//! raw lock belongs.
//!
//! Checked when it was written against a compiler-level census — every call to
//! `Mutex::<Engine>::lock` / `try_lock` in the three crates' MIR: on the tree before the
//! guard it named the same 53 production acquisitions the census did, and nothing else. Run over
//! the test code as well (which it does not scan), it named 923 of the census's 964 with no false
//! finding; the 41 it missed hold a handle unpacked from a tuple a test scene builder returns
//! (`let (engine, pool, ports) = three_radio_pool()`), a shape no production code uses.

use std::path::{Path, PathBuf};

/// `text` with every comment, string literal and character literal blanked to spaces, newlines
/// kept, so offsets and line numbers still point into the original.
fn code_only(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = b.to_vec();
    let blank = |out: &mut Vec<u8>, from: usize, to: usize| {
        for c in &mut out[from..to.min(b.len())] {
            if *c != b'\n' {
                *c = b' ';
            }
        }
    };
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                let end = b[i..]
                    .iter()
                    .position(|&c| c == b'\n')
                    .map_or(b.len(), |p| i + p);
                blank(&mut out, i, end);
                i = end;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let (mut depth, mut j) = (0, i);
                while j < b.len() {
                    if b[j] == b'/' && b.get(j + 1) == Some(&b'*') {
                        depth += 1;
                        j += 2;
                    } else if b[j] == b'*' && b.get(j + 1) == Some(&b'/') {
                        depth -= 1;
                        j += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        j += 1;
                    }
                }
                blank(&mut out, i, j);
                i = j;
            }
            // A raw string, `r"…"` / `r#"…"#` (or `br…`): no escapes inside, so it ends only
            // at a quote followed by as many hashes as opened it.
            b'r' if matches!(b.get(i + 1), Some(b'"' | b'#'))
                && (i == 0
                    || !ident(b[i - 1])
                    || (b[i - 1] == b'b' && (i < 2 || !ident(b[i - 2])))) =>
            {
                let mut j = i + 1;
                let mut hashes = 0;
                while b.get(j) == Some(&b'#') {
                    hashes += 1;
                    j += 1;
                }
                if b.get(j) != Some(&b'"') {
                    i += 1;
                    continue;
                }
                j += 1;
                while j < b.len() {
                    if b[j] == b'"'
                        && b[j + 1..].iter().take_while(|&&c| c == b'#').count() >= hashes
                    {
                        j += 1 + hashes;
                        break;
                    }
                    j += 1;
                }
                blank(&mut out, i, j);
                i = j;
            }
            b'"' => {
                let mut j = i + 1;
                while j < b.len() && b[j] != b'"' {
                    if b[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                blank(&mut out, i, j + 1);
                i = j + 1;
            }
            // A character literal is blanked; a lifetime (`'a`) is left alone.
            b'\'' => match char_literal_len(b, i) {
                Some(len) => {
                    blank(&mut out, i, i + len);
                    i += len;
                }
                None => i += 1,
            },
            _ => i += 1,
        }
    }
    String::from_utf8(out).expect("blanking replaces whole literals with ASCII")
}

/// The length of the character literal starting at the quote `b[i]`, or `None` for a lifetime.
fn char_literal_len(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i + 1) == Some(&b'\\') {
        // `'\''`: the escaped quote is not the closing one.
        let from = if b.get(i + 2) == Some(&b'\'') {
            i + 3
        } else {
            i + 2
        };
        let close = from + b[from..].iter().take(12).position(|&c| c == b'\'')?;
        return Some(close - i + 1);
    }
    let first = *b.get(i + 1)?;
    let len = match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    };
    (b.get(i + 1 + len) == Some(&b'\'')).then_some(len + 2)
}

/// `code` with every item under `#[cfg(test)]` (or `#[cfg(all(test, …))]`) blanked, newlines kept.
fn without_test_items(code: &str) -> String {
    let mut out = code.as_bytes().to_vec();
    let mut at = 0;
    for line in code.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("#[cfg(test)]") || t.starts_with("#[cfg(all(test") {
            // The item runs from the attribute to its `;` or the close of its first `{`.
            let b = code.as_bytes();
            let mut j = at + line.find('#').unwrap_or(0) + 1;
            // Past this attribute's own closing bracket.
            let mut sq = 0;
            while j < b.len() {
                match b[j] {
                    b'[' => sq += 1,
                    b']' => {
                        sq -= 1;
                        if sq == 0 {
                            j += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            let (mut depth, mut end) = (0i32, b.len());
            let mut k = j;
            while k < b.len() {
                match b[k] {
                    b';' if depth == 0 => {
                        end = k + 1;
                        break;
                    }
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = k + 1;
                            break;
                        }
                    }
                    _ => {}
                }
                k += 1;
            }
            let from = at + line.len() - t.len();
            for c in &mut out[from..end] {
                if *c != b'\n' {
                    *c = b' ';
                }
            }
        }
        at += line.len();
    }
    String::from_utf8(out).expect("blanking keeps UTF-8")
}

/// A fn in the blanked source: its name, the byte range from `fn` to the close of its body, and
/// where the body opens — so `range.start..body` is the signature.
struct Item {
    name: String,
    range: std::ops::Range<usize>,
    body: usize,
}

/// Every fn in `code`, nested ones included.
fn fns(code: &str) -> Vec<Item> {
    let b = code.as_bytes();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = code[i..].find("fn") {
        let at = i + p;
        i = at + 2;
        if (at > 0 && ident(b[at - 1])) || !b.get(at + 2).is_some_and(u8::is_ascii_whitespace) {
            continue;
        }
        let name_at = at + 2 + code[at + 2..].len() - code[at + 2..].trim_start().len();
        let name: String = code[name_at..]
            .chars()
            .take_while(|&c| c.is_alphanumeric() || c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        // The body opens at the first `{` outside the parameter list; a `;` first means none.
        let (mut paren, mut open) = (0i32, None);
        for (k, &c) in b.iter().enumerate().skip(name_at) {
            match c {
                b'(' => paren += 1,
                b')' => paren -= 1,
                b';' if paren == 0 => break,
                b'{' if paren == 0 => {
                    open = Some(k);
                    break;
                }
                _ => {}
            }
        }
        let Some(open) = open else { continue };
        let mut depth = 0i32;
        let mut end = b.len();
        for (k, &c) in b.iter().enumerate().skip(open) {
            match c {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = k + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        out.push(Item {
            name,
            range: at..end,
            body: open,
        });
    }
    out
}

/// `ty` without whitespace, references, and the `Arc` / `Rc` / Tauri `State` around it — the
/// type the value actually is.
fn outermost(ty: &str) -> String {
    let mut t: String = ty.chars().filter(|c| !c.is_whitespace()).collect();
    loop {
        let s = t.trim_start_matches('&');
        let s = s.strip_prefix("mut").unwrap_or(s);
        let s = s.strip_prefix("'static").unwrap_or(s);
        let mut next = s.to_string();
        for w in [
            "std::sync::Arc<",
            "Arc<",
            "std::rc::Rc<",
            "Rc<",
            "tauri::State<",
            "State<",
        ] {
            if let Some(inner) = s.strip_prefix(w).and_then(|r| r.strip_suffix('>')) {
                // `State<'_, T>`: the type is the last argument.
                next = inner.rsplit(',').next().unwrap_or(inner).to_string();
                break;
            }
        }
        if next == t {
            return t;
        }
        t = next;
    }
}

/// Whether a value of type `ty` IS the Engine mutex: `SharedEngine`, or a `Mutex<…Engine>`. A
/// mutex HOLDING a handle (`Mutex<Option<SharedEngine>>`) is a different lock, and is not.
fn is_engine_mutex(ty: &str) -> bool {
    let t = outermost(ty);
    // `SharedEngine` by any path — `crate::SharedEngine`, `super::SharedEngine`.
    if t == "SharedEngine" || t.ends_with("::SharedEngine") {
        return true;
    }
    let t = t.strip_prefix("std::sync::").unwrap_or(&t);
    t.strip_prefix("Mutex<")
        .and_then(|r| r.strip_suffix('>'))
        .is_some_and(is_engine)
}

/// Whether `ty` is the Engine itself — what a `Mutex::new` makes the Engine mutex of.
fn is_engine(ty: &str) -> bool {
    let t = outermost(ty);
    t == "Engine" || (t.ends_with("::Engine") && !t.contains('<'))
}

/// The names `code[range]` declares with a type `pred` accepts: `name: Type` wherever it
/// appears — parameters, typed `let`s, closure parameters, struct fields.
fn declared(code: &str, range: std::ops::Range<usize>, pred: fn(&str) -> bool) -> Vec<String> {
    let b = code.as_bytes();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut names = Vec::new();
    for k in range.clone() {
        if b[k] != b':' || b.get(k + 1) == Some(&b':') || (k > 0 && b[k - 1] == b':') {
            continue;
        }
        let mut s = k;
        while s > range.start && b[s - 1].is_ascii_whitespace() {
            s -= 1;
        }
        let e = s;
        while s > range.start && ident(b[s - 1]) {
            s -= 1;
        }
        if s == e || (s > 0 && b[s - 1] == b'\'') {
            continue; // no name, or a loop label
        }
        // The type runs to the first `,` `)` `=` `;` `{` `|` outside its own brackets.
        let (mut depth, mut t) = (0i32, k + 1);
        while t < range.end {
            match b[t] {
                b'<' | b'(' | b'[' => depth += 1,
                b'>' if b[t - 1] == b'-' => {}
                b'>' | b')' | b']' => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                b',' | b'=' | b';' | b'{' | b'|' if depth == 0 => break,
                _ => {}
            }
            t += 1;
        }
        if pred(&code[k + 1..t]) {
            names.push(code[s..e].to_string());
        }
    }
    names
}

/// The fns in `code` whose return type `pred` accepts — `fn engine() -> SharedEngine` for the
/// Engine mutex, `fn engine_on_store(..) -> Engine` for an Engine.
fn makers(code: &str, pred: fn(&str) -> bool) -> Vec<String> {
    fns(code)
        .into_iter()
        .filter(|f| {
            let sig = &code[f.range.start..f.body];
            sig.find("->").is_some_and(|at| {
                let ret = &sig[at + 2..];
                pred(&ret[..ret.find("where").unwrap_or(ret.len())])
            })
        })
        .map(|f| f.name)
        .collect()
}

/// The fns across the scanned files that return the Engine mutex, and those that return an
/// Engine for a `Mutex::new` to wrap.
struct Makers {
    handles: Vec<String>,
    engines: Vec<String>,
}

/// The names in `item` that hold the Engine mutex: those declared with its type, and those a
/// `let` binds to it — a move, borrow, `clone()` or `Arc::clone` of one of them or of an
/// Engine-mutex field (`fields`), a `Mutex::new` of an Engine, a call to a fn that returns one
/// (`makers`), or an expression naming `SharedEngine` that is not a lock.
fn engine_handles(code: &str, item: &Item, fields: &[String], makers: &Makers) -> Vec<String> {
    let mut names = declared(code, item.range.clone(), is_engine_mutex);
    let mut values = declared(code, item.range.clone(), is_engine);
    let body = &code[item.range.clone()];
    let squeeze = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    loop {
        let mut grew = false;
        for (at, _) in body.match_indices("let ") {
            if at > 0 && body.as_bytes()[at - 1].is_ascii_alphanumeric() {
                continue;
            }
            let rest = body[at + 4..].trim_start();
            let rest = rest.strip_prefix("mut ").unwrap_or(rest).trim_start();
            let name: String = rest
                .chars()
                .take_while(|&c| c.is_alphanumeric() || c == '_')
                .collect();
            let after = rest[name.len()..].trim_start();
            if name.is_empty() || names.contains(&name) || !after.starts_with('=') {
                continue;
            }
            let init = &after[1..];
            let init = squeeze(&init[..init.find(';').unwrap_or(init.len())]);
            // An Engine itself, for a `Mutex::new` further down to wrap.
            let engine_path = |s: &str| {
                [
                    "Engine::",
                    "engine::Engine::",
                    "tempo_app::engine::Engine::",
                    "crate::engine::Engine::",
                ]
                .iter()
                .any(|p| s.starts_with(p))
            };
            if engine_path(&init) && !values.contains(&name) {
                values.push(name.clone());
            }
            let callee_of = |s: &str| -> String {
                s[..s.find('(').unwrap_or(0)]
                    .rsplit("::")
                    .next()
                    .unwrap_or("")
                    .to_string()
            };
            let wraps = init.match_indices("Mutex::new(").any(|(m, pat)| {
                let arg = &init[m + pat.len()..];
                engine_path(arg)
                    || values.iter().any(|v| arg.starts_with(&format!("{v})")))
                    || makers.engines.contains(&callee_of(arg))
            });
            let mut core = init.clone();
            for t in [
                "std::sync::Arc::clone(",
                "Arc::clone(",
                ".inner()",
                ".clone()",
                "&",
                "*",
                "(",
                ")",
            ] {
                core = core.replace(t, "");
            }
            let last = core.rsplit('.').next().unwrap_or(&core).to_string();
            let callee = callee_of(&init);
            let handle = (init.contains("SharedEngine") && !init.contains("lock("))
                || wraps
                || names.contains(&core)
                || (core.contains('.') && fields.contains(&last))
                || (!callee.is_empty() && makers.handles.contains(&callee));
            if handle {
                names.push(name);
                grew = true;
            }
        }
        if !grew {
            return names;
        }
    }
}
/// The receiver of the method call whose `.` is at `dot`: the postfix chain before it, with any
/// whitespace a line break put before a `.`.
fn receiver(code: &str, dot: usize) -> (usize, &str) {
    let b = code.as_bytes();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = dot;
    loop {
        let mut j = i;
        while j > 0 && b[j - 1].is_ascii_whitespace() {
            j -= 1;
        }
        let end = j;
        // Trailing groups — a call's `(…)`, an index, a turbofish's `<…>`.
        while j > 0 && matches!(b[j - 1], b')' | b']' | b'>') {
            let close = b[j - 1];
            let open = match close {
                b')' => b'(',
                b']' => b'[',
                _ => b'<',
            };
            let mut depth = 0;
            while j > 0 {
                j -= 1;
                if b[j] == close {
                    depth += 1;
                } else if b[j] == open {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
        }
        while j > 0 && ident(b[j - 1]) {
            j -= 1;
        }
        if j == end {
            break;
        }
        i = j;
        let mut k = i;
        while k > 0 && b[k - 1].is_ascii_whitespace() {
            k -= 1;
        }
        if k > 0 && b[k - 1] == b'.' {
            i = k - 1;
        } else if k > 1 && b[k - 1] == b':' && b[k - 2] == b':' {
            i = k - 2;
        } else {
            break;
        }
    }
    (i, code[i..dot].trim())
}

/// The guard constructors, the one place a raw lock on the Engine belongs.
const GUARD_CONSTRUCTORS: [&str; 3] = ["engine_lock", "engine_lock_result", "engine_try_lock"];

/// Every raw `.lock()` / `.try_lock()` on the Engine mutex in `files` (path, text), as
/// `path:line: receiver.lock()`, outside test code (unless `tests_too`) and outside the guard
/// constructors in `engine.rs` (unless `constructors_too`).
fn raw_engine_locks(
    files: &[(String, String)],
    constructors_too: bool,
    tests_too: bool,
) -> Vec<String> {
    let prepared: Vec<(&str, &str, String)> = files
        .iter()
        .filter(|(path, _)| tests_too || !test_file(path))
        .map(|(path, text)| {
            let code = code_only(text);
            let code = if tests_too {
                code
            } else {
                without_test_items(&code)
            };
            (path.as_str(), text.as_str(), code)
        })
        .collect();
    // What every file can see: the Engine-mutex fields of any struct (the declarations outside
    // every fn body), so `self.engine` is known wherever it is read, and the fns that return
    // the Engine mutex.
    let mut fields = Vec::new();
    let mut made = Makers {
        handles: Vec::new(),
        engines: Vec::new(),
    };
    for (_, _, code) in &prepared {
        let mut from = 0;
        for f in &fns(code) {
            if f.range.start > from {
                fields.extend(declared(code, from..f.range.start, is_engine_mutex));
            }
            from = from.max(f.range.end);
        }
        fields.extend(declared(code, from..code.len(), is_engine_mutex));
        made.handles.extend(makers(code, is_engine_mutex));
        made.engines.extend(makers(code, is_engine));
    }
    let mut out: Vec<(&str, usize, String)> = Vec::new();
    for (path, text, code) in &prepared {
        let items = fns(code);
        for pat in [".lock()", ".try_lock()"] {
            for (dot, _) in code.match_indices(pat) {
                let (start, recv) = receiver(code, dot);
                let Some(item) = items
                    .iter()
                    .filter(|f| f.range.contains(&dot))
                    .min_by_key(|f| f.range.len())
                else {
                    continue;
                };
                if !constructors_too
                    && path.ends_with("tempo-app/src/engine.rs")
                    && GUARD_CONSTRUCTORS.contains(&item.name.as_str())
                {
                    continue;
                }
                let r: String = recv.chars().filter(|c| !c.is_whitespace()).collect();
                let mut r = r.trim_matches(['&', '*', '(', ')']).to_string();
                while let Some(stripped) = r.strip_suffix(".inner()").or(r.strip_suffix(".clone()"))
                {
                    r = stripped.to_string();
                }
                let last = r.rsplit('.').next().unwrap_or(&r).to_string();
                let names = engine_handles(code, item, &fields, &made);
                let engine = r.contains("SharedEngine>()")
                    || (!r.contains('.') && names.contains(&r))
                    || (r.contains('.') && (fields.contains(&last) || names.contains(&last)));
                if engine {
                    let line = text[..start].matches('\n').count() + 1;
                    out.push((
                        *path,
                        line,
                        format!(
                            "{recv}{pat}",
                            recv = recv.split_whitespace().collect::<String>()
                        ),
                    ));
                }
            }
        }
    }
    out.sort();
    out.into_iter()
        .map(|(path, line, call)| format!("{path}:{line}: {call}"))
        .collect()
}

/// A file a test module is made of — never production code.
fn test_file(path: &str) -> bool {
    let p = Path::new(path);
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    p.components().any(|c| c.as_os_str() == "tests")
        || name == "tests.rs"
        || name.ends_with("_tests.rs")
        || name.ends_with("_test.rs")
}

/// Every `.rs` file under `dir`, as (path relative to the repository, text).
fn sources(repo: &Path, dir: &str, out: &mut Vec<(String, String)>) {
    let mut stack = vec![repo.join(dir)];
    while let Some(d) = stack.pop() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&d)
            .unwrap_or_else(|e| panic!("read {}: {e}", d.display()))
            .map(|e| e.expect("entry").path())
            .collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let rel = p.strip_prefix(repo).expect("under the repo");
                let text = std::fs::read_to_string(&p).expect("source text");
                out.push((rel.to_string_lossy().replace('\\', "/"), text));
            }
        }
    }
}

/// The three crates' sources, read from this checkout.
fn production_sources() -> Vec<(String, String)> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for dir in [
        "crates/tempo-app/src",
        "crates/tempo-audio/src",
        "src-tauri/src",
    ] {
        sources(&repo, dir, &mut out);
    }
    out
}

#[test]
fn the_engine_lock_is_taken_through_engine_guard_everywhere_outside_tests() {
    let offenders = raw_engine_locks(&production_sources(), false, false);
    assert!(
        offenders.is_empty(),
        "these take the Engine lock without an EngineGuard, so the logbook fence cannot see them \
         (tempo_core::logbook::io_fence) — use engine_lock / engine_lock_result / \
         engine_try_lock, which take the same lock:\n{}",
        offenders.join("\n")
    );
}

/// The control on the real tree: with the guard constructors not excused, the scan finds the
/// raw locks inside them — so it read `engine.rs`, recognised the Engine mutex by its type, and
/// saw through to the calls. A scan that found nothing here would be reading nothing.
#[test]
fn the_scan_sees_the_raw_locks_inside_the_guard_constructors() {
    let found = raw_engine_locks(&production_sources(), true, false);
    let engine_rs: Vec<&String> = found
        .iter()
        .filter(|f| f.starts_with("crates/tempo-app/src/engine.rs:"))
        .collect();
    assert_eq!(
        engine_rs.len(),
        3,
        "one raw lock in each of engine_lock, engine_lock_result and engine_try_lock: {found:#?}"
    );
    let files = production_sources();
    for must in [
        "crates/tempo-app/src/engine.rs",
        "crates/tempo-audio/src/service.rs",
        "src-tauri/src/lib.rs",
        "src-tauri/src/remote_service/operations/logging.rs",
    ] {
        assert!(
            files.iter().any(|(p, _)| p == must),
            "the scan reads {must}"
        );
    }
}

/// The scan is only worth having if it can fail: every shape a raw Engine lock takes in this
/// codebase is named — a parameter, a command's `State`, an `Arc::clone`, Tauri's managed state,
/// a struct field, a `Mutex::new` of an Engine, a fn that returns the handle, a clone of a
/// field — and nothing that is not one: a lock inside a test item or a test file, a lock in a
/// comment or a string, a guard constructor, a different mutex, a mutex HOLDING a handle.
#[test]
fn the_scan_names_a_raw_engine_lock_in_every_shape_and_nothing_else() {
    // Built from quoted lines, so none of this file's own text looks like a raw lock.
    let lib = [
        "type SharedEngine = Arc<Mutex<Engine>>;",
        "struct Worker { engine: Arc<Mutex<Engine>>, spots: Arc<Mutex<Spots>> }",
        "fn param(engine: &SharedEngine) {",
        "    let e = engine.lock().unwrap();",
        "}",
        "fn command(state: State<'_, SharedEngine>) -> Result<(), String> {",
        "    state",
        "        .lock()",
        "        .map_err(|e| e.to_string())?",
        "        .set_skip_tx1(true);",
        "    Ok(())",
        "}",
        "fn cloned(engine: &Arc<Mutex<tempo_app::engine::Engine>>) {",
        "    let push_engine = Arc::clone(&engine);",
        "    std::thread::spawn(move || { let e = push_engine.try_lock(); });",
        "}",
        "fn managed(app: &AppHandle) {",
        "    if let Ok(e) = app.state::<SharedEngine>().inner().lock() {}",
        "}",
        "impl Worker {",
        "    fn tick(&self) { let e = self.engine.lock(); }",
        "    fn other(&self) { let s = self.spots.lock(); }",
        "}",
        "fn guarded(engine: &SharedEngine) {",
        "    let e = engine_lock(engine);",
        "    // engine.lock() in a comment",
        "    let s = \"engine.lock()\";",
        "}",
        "fn different(state: State<'_, SharedOtaSpots>) {",
        "    let c = state.lock();",
        "}",
        "fn holds_a_handle(local: Arc<Mutex<Option<SharedEngine>>>) {",
        "    let slot = local.lock().unwrap().take();",
        "}",
        "fn built() {",
        "    let engine = Arc::new(Mutex::new(Engine::new(\"K2DEF\", \"FN31\", 0)));",
        "    let e = engine.lock().unwrap();",
        "}",
        "fn fresh_engine() -> SharedEngine { unreachable!() }",
        "fn made() { let engine = fresh_engine(); let e = engine.try_lock(); }",
        "struct Deps { engine: SharedEngine }",
        "fn from_a_field(d: &Deps) {",
        "    let engine = d.engine.clone();",
        "    let e = engine.lock();",
        "}",
        "#[cfg(test)]",
        "mod tests {",
        "    fn setup(engine: &SharedEngine) { let e = engine.lock().unwrap(); }",
        "}",
    ]
    .join("\n");
    let test_file = "fn t(engine: &SharedEngine) { let e = engine.lock().unwrap(); }".to_string();
    let files = vec![
        ("src-tauri/src/lib.rs".to_string(), lib),
        (
            "src-tauri/src/remote_service/operations/x_tests.rs".to_string(),
            test_file,
        ),
    ];
    assert_eq!(
        raw_engine_locks(&files, false, false),
        [
            "src-tauri/src/lib.rs:4: engine.lock()",
            "src-tauri/src/lib.rs:7: state.lock()",
            "src-tauri/src/lib.rs:15: push_engine.try_lock()",
            "src-tauri/src/lib.rs:18: app.state::<SharedEngine>().inner().lock()",
            "src-tauri/src/lib.rs:21: self.engine.lock()",
            "src-tauri/src/lib.rs:37: engine.lock()",
            "src-tauri/src/lib.rs:40: engine.try_lock()",
            "src-tauri/src/lib.rs:44: engine.lock()",
        ]
    );
}

/// The guard constructors answer exactly as `Mutex::lock` / `try_lock` do — one lock, the same
/// poison, the same `WouldBlock` — and a guard is counted for the fence for exactly as long as
/// it lives (in a debug build; a release build counts nothing).
#[test]
fn the_guard_constructors_answer_as_the_mutex_does_and_count_while_held() {
    use std::sync::{Arc, Mutex, TryLockError};
    use tempo_app::engine::{engine_lock, engine_lock_result, engine_try_lock, Engine};
    use tempo_core::logbook::io_fence::engine_guards_held;

    let counted = u32::from(cfg!(debug_assertions));
    let m = Arc::new(Mutex::new(Engine::new("K2DEF", "FN31", 0)));
    assert_eq!(engine_guards_held(), 0);
    {
        let _held = engine_lock(&m);
        assert_eq!(engine_guards_held(), counted);
        let other = Arc::clone(&m);
        let busy = std::thread::spawn(move || {
            let answer = matches!(engine_try_lock(&other), Err(TryLockError::WouldBlock));
            (answer, engine_guards_held())
        })
        .join()
        .unwrap();
        assert_eq!(
            busy,
            (true, 0),
            "the same lock: busy to another thread, which holds no guard of its own"
        );
    }
    assert_eq!(engine_guards_held(), 0, "a dropped guard stops counting");
    {
        let _held = engine_try_lock(&m).expect("free");
        assert_eq!(engine_guards_held(), counted);
    }
    assert_eq!(engine_guards_held(), 0);

    // Poison, the real way: a thread panics holding the guard.
    let p = Arc::clone(&m);
    let _ = std::thread::spawn(move || {
        let _held = engine_lock(&p);
        panic!("poison the engine mutex for real");
    })
    .join();
    assert!(m.lock().is_err(), "precondition: the mutex is poisoned");
    let poisoned = engine_lock_result(&m)
        .err()
        .expect("reported, as Mutex::lock reports it");
    assert_eq!(
        engine_guards_held(),
        counted,
        "the guard inside the error is live"
    );
    drop(poisoned.into_inner());
    assert_eq!(engine_guards_held(), 0);
    assert!(matches!(
        engine_try_lock(&m),
        Err(TryLockError::Poisoned(_))
    ));
    assert_eq!(engine_guards_held(), 0);
    assert_eq!(
        engine_lock(&m).snapshot().mycall,
        "K2DEF",
        "engine_lock recovers, as it always has"
    );
}
