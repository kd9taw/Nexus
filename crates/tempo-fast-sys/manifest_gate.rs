//! Build gate: every module-scope Fortran symbol and every static-storage C symbol in the
//! vendored modem must be classified in `libtempo/modem-state-manifest.toml`.
//!
//! # Why this exists
//!
//! Two radio chains decoding different bands in one process share every statically-allocated
//! Fortran symbol unless the manifest's class-1 set is swapped around each decode. An
//! *unclassified* symbol is, by default, a *shared* one — which is exactly the bug this
//! whole audit was written to prevent. The failure mode is not a crash: it is a CRC-valid,
//! syntactically perfect, WRONG decode that gets logged and uploaded.
//!
//! So a vendor refresh that quietly introduces new state must break the build, not ship.
//!
//! # What it does and does not catch
//!
//! In Fortran it scans for the *greppable* declaration forms — `save`, `data`, `common`, and
//! module-scope declarations between `module` and `contains`. In C it scans every `static`
//! (at any brace depth, since a function-local static is just as shared) plus file-scope
//! globals; see [`scan_c`] for why `const` is not treated the way Fortran's `parameter` is.
//! It deliberately does **not** try to re-derive
//! the full audit: ~160 of the manifest's 585 symbols are ordinary locals that gfortran spilled
//! into `.bss` for exceeding `-fmax-stack-var-size`, which is a property of the *compiler
//! invocation*, not the source, and no source scan can see them.
//!
//! That is the right trade. The gate's job is "did a refresh add state nobody classified",
//! and new state arrives as a visible declaration. Catching the compiler-spilled set would
//! need `nm` on a built object, which is a different (and much slower) check.
//!
//! # Keyed on (file, name), not (file, line)
//!
//! The manifest records `line` for humans to find the symbol. The gate ignores it: an
//! unrelated edit that shifts a declaration down three lines is not a finding, and a gate that
//! cries wolf on every whitespace change gets disabled. A genuinely NEW symbol changes the
//! (file, name) set, which is what we test.

use std::collections::HashSet;
use std::path::Path;

/// A symbol the scanner believes is module-scope state, keyed as the gate compares it.
#[derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key {
    pub file: String,
    pub name: String,
}

/// Pull `name` + `file` out of every `[[symbol]]` table. Deliberately a hand-rolled scan
/// rather than a toml dependency: build-dependencies are compiled for the HOST on every clean
/// build of every target, and this file's shape is fully under our control.
pub fn parse_manifest(text: &str) -> HashSet<Key> {
    let mut out = HashSet::new();
    let (mut name, mut file) = (None, None);
    for line in text.lines() {
        let t = line.trim();
        if t == "[[symbol]]" {
            name = None;
            file = None;
        } else if let Some(v) = t.strip_prefix("name = ") {
            name = Some(v.trim().trim_matches('"').to_string());
        } else if let Some(v) = t.strip_prefix("file = ") {
            file = Some(v.trim().trim_matches('"').to_string());
        }
        if let (Some(n), Some(f)) = (&name, &file) {
            out.insert(Key {
                file: f.clone(),
                name: n.clone(),
            });
            name = None;
            file = None;
        }
    }
    out
}

/// Symbol names declared at module scope in one Fortran source.
///
/// Intentionally over-inclusive on `save`/`data`/`common` and conservative on plain
/// declarations (module scope only — between `module` and the first `contains`), because a
/// false positive here costs one manifest row while a false negative costs a shared symbol.
pub fn scan_fortran(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_module = false;
    let mut past_contains = false;
    // Interface blocks declare PROCEDURES, not storage — and their bodies are full of `::`
    // result-type and dummy-argument declarations that look exactly like module state. The
    // whole crc.f90 bind(C) block was the scanner's false-positive population.
    let mut interface_depth = 0i32;
    // A derived TYPE block declares member layout, not storage. `harq_slot`'s members
    // (cd_rv0, freq, rv_count, …) are not symbols — the SLOT ARRAY declared of that type is,
    // and it is classified separately.
    let mut in_type_block = false;
    for raw in src.lines() {
        let line = raw.split('!').next().unwrap_or("").trim(); // strip comments
        let low = line.to_ascii_lowercase();
        if low.starts_with("module ") && !low.starts_with("module procedure") {
            in_module = true;
            past_contains = false;
            continue;
        }
        if low == "contains" {
            past_contains = true;
            continue;
        }
        if (low.starts_with("type ") || low.starts_with("type::") || low.starts_with("type,"))
            && line.contains("::")
            && !low.contains("(")
        {
            in_type_block = true;
            continue;
        }
        if low.starts_with("end type") {
            in_type_block = false;
            continue;
        }
        if in_type_block {
            continue;
        }
        if low.starts_with("interface") || low.starts_with("abstract interface") {
            interface_depth += 1;
            continue;
        }
        if low.starts_with("end interface") {
            interface_depth = (interface_depth - 1).max(0);
            continue;
        }
        if interface_depth > 0 {
            continue;
        }
        if low.starts_with("end module") {
            in_module = false;
            past_contains = false;
            continue;
        }
        // `save ::`, `data x/…/`, `common /blk/ a,b` carry state wherever they appear —
        // including inside subroutines, which is the classic SAVEd-local idiom.
        let explicit = !low.starts_with("use ")
            && (low.starts_with("save ")
                || low.starts_with("save::")
                || low.starts_with("data ")
                || low.starts_with("common ")
                || low.starts_with("common/"));
        // `use … :: …`, interfaces and procedure declarations also carry `::` but declare no
        // storage — they were the scanner's entire false-positive population on the real tree
        // (iso_c_binding imports, `procedure` names inside interface blocks).
        let is_decl_noise = low.starts_with("use ")
            || low.starts_with("use,")
            || low.starts_with("interface")
            || low.starts_with("end interface")
            || low.starts_with("procedure")
            || low.starts_with("module procedure")
            || low.starts_with("import")
            || low.starts_with("implicit")
            || low.starts_with("abstract")
            // Access-specifier statements (`public :: null_timer`) name procedures, not
            // storage. The storage is whatever variable is declared elsewhere.
            || low.starts_with("public")
            || low.starts_with("private")
            || low.starts_with("protected");
        // A module-scope declaration before `contains` is implicitly SAVEd.
        let module_decl = in_module && !past_contains && line.contains("::") && !is_decl_noise;
        if !(explicit || module_decl) {
            continue;
        }
        out.extend(names_in_decl(line));
    }
    out.sort();
    out.dedup();
    out
}

/// Identifier names from one declaration line, ignoring types, attributes, dimensions and
/// initializers.
fn names_in_decl(line: &str) -> Vec<String> {
    // Everything after `::` is the name list; without `::` (save/data/common) take the tail.
    let tail = match line.split_once("::") {
        Some((_, rhs)) => rhs,
        None => {
            let low = line.to_ascii_lowercase();
            let kw = ["save", "data", "common"]
                .iter()
                .find(|k| low.starts_with(*k))
                .copied()
                .unwrap_or("");
            &line[kw.len().min(line.len())..]
        }
    };
    let mut names = Vec::new();
    let mut depth = 0i32; // skip dimensions (…) and DATA initializers /…/
    let mut cur = String::new();
    let mut in_slash = false;
    for ch in tail.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            '/' => in_slash = !in_slash,
            _ if depth > 0 || in_slash => {}
            c if c.is_alphanumeric() || c == '_' => cur.push(c),
            _ => {
                push_name(&mut names, &mut cur);
            }
        }
        if depth > 0 || in_slash {
            push_name(&mut names, &mut cur);
        }
    }
    push_name(&mut names, &mut cur);
    names
}

/// Symbol names with static storage duration in one C source.
///
/// Q65 is the first vendored mode to bring substantial C, and until it did, this gate scanned
/// Fortran only — so `q65_subs.c`'s 344-byte mutable `codec` struct and its paired first-call
/// guards sat in `.bss` completely outside the audit while the build reported "all classified".
/// That is the exact false pass the manifest exists to prevent.
///
/// Two forms have static storage duration and both are process-global:
///
///   * `static` at **any** brace depth. A function-local `static int first=1;` is every bit as
///     shared between chains as a file-scope one; it just has a mangled name (`first.0`) in the
///     object file. Depth is therefore not a filter for these.
///   * a file-scope (depth 0) definition **without** `static`, which is an external-linkage
///     global — `const qracode qra_13_64_64_irr_e` is one.
///
/// # `const` is NOT excluded, and that is deliberate
///
/// The Fortran scanner drops `parameter`, which is safe there: a Fortran `parameter` is a
/// compile-time constant with no storage at all. C's `const` is a different thing — it is
/// storage that may not be written *through that lvalue*, which does not imply the linker put
/// it anywhere read-only. Measured on this very tree: `pd_uniform_tab` (pdmath.c:68) and
/// `qra_13_64_64_irr_e` (qra13_64_64_irr_e.c:495) are both `const`-qualified and both land in
/// writable `.data`, because they hold relocated pointers. Excluding `const` would have skipped
/// both. The `.rodata` ones cost one class-4 manifest row each, which is the cheap side of the
/// trade this gate has always made.
pub fn scan_c(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth: i32 = 0;
    // Parens must be tracked separately from braces. A multi-line function signature keeps
    // brace depth at 0, so without this every continuation line reads as a file-scope
    // declaration and each PARAMETER becomes a symbol: q65.c:529 alone contributed seven
    // phantoms (pAPMask, pAPSymbols, ...) before this was added.
    let mut paren: i32 = 0;
    let mut in_block_comment = false;

    for raw in src.lines() {
        let line = strip_c_comments(raw, &mut in_block_comment);
        let t = line.trim();

        // Preprocessor lines can carry braces inside macro bodies that never balance.
        if paren == 0 && !t.starts_with('#') && !t.is_empty() {
            let is_static = t.starts_with("static ") || t.starts_with("static\t");
            // A declaration is only a *definition* of storage at depth 0 when unqualified;
            // `static` counts anywhere. `extern` is a reference to storage defined elsewhere,
            // and `typedef` declares a type, so neither allocates.
            if (is_static || depth == 0)
                && !t.starts_with("extern")
                && !t.starts_with("typedef")
                && !is_c_function_decl(t)
            {
                for n in c_declarator_names(t) {
                    out.push(n);
                }
            }
        }

        depth += line.matches('{').count() as i32;
        depth -= line.matches('}').count() as i32;
        if depth < 0 {
            depth = 0;
        }
        paren += line.matches('(').count() as i32;
        paren -= line.matches(')').count() as i32;
        if paren < 0 {
            paren = 0;
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Blank out `//` and `/* */` comment text, carrying block state across lines.
fn strip_c_comments(line: &str, in_block: &mut bool) -> String {
    let b = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < b.len() {
        if *in_block {
            if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                *in_block = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            *in_block = true;
            i += 2;
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            break;
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}

/// True when the declarator is a function rather than storage.
///
/// The discriminator is a `(` in the declarator, i.e. before any `=` or `;`. That keeps
/// `static void np_fwht2(float*, float*);` out while keeping `static pnp_fwht np_fwht_tab[7]`
/// in — an array of function pointers behind a typedef has no parens of its own and IS
/// writable storage (npfwht.c:38 is exactly that, and it is not even `const`).
fn is_c_function_decl(t: &str) -> bool {
    let head = t.split(['=', ';']).next().unwrap_or(t);
    head.contains('(')
}

/// Declared names from one C declaration line, handling `static int a, b;`.
fn c_declarator_names(t: &str) -> Vec<String> {
    let head = t.split(['=', ';', '{']).next().unwrap_or(t);
    let mut out = Vec::new();
    for part in head.split(',') {
        // Everything before `[` is the declarator; the last identifier in it is the name.
        let decl = part.split('[').next().unwrap_or(part);
        let last = decl
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .rfind(|s| !s.is_empty());
        if let Some(name) = last {
            if name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                continue;
            }
            if C_NOISE.contains(&name) {
                continue;
            }
            out.push(name.to_string());
        }
    }
    out
}

/// Type and storage keywords that can end up last when a line declares no name — e.g. a bare
/// `static const int` continuation, or a struct tag with the declarator on the next line.
const C_NOISE: &[&str] = &[
    "static", "const", "volatile", "unsigned", "signed", "int", "char", "short", "long", "float",
    "double", "void", "struct", "union", "enum", "inline", "restrict", "register", "auto",
];

fn push_name(names: &mut Vec<String>, cur: &mut String) {
    if cur.is_empty() {
        return;
    }
    let s = std::mem::take(cur);
    // Type/attribute keywords that survive the `::` split on continuation lines, plus bare
    // numbers from dimensions.
    const NOISE: &[&str] = &[
        "integer",
        "real",
        "complex",
        "character",
        "logical",
        "double",
        "precision",
        "parameter",
        "save",
        "data",
        "common",
        "dimension",
        "allocatable",
        "pointer",
        "target",
        "intent",
        "in",
        "out",
        "inout",
        "kind",
        "len",
        "type",
        "class",
        "public",
        "private",
        "protected",
        "optional",
        "value",
        "external",
        "intrinsic",
        // Literals and intrinsics that survive DATA/initializer parsing.
        "true",
        "false",
        "reshape",
        "null",
        "none",
    ];
    let low = s.to_ascii_lowercase();
    if NOISE.contains(&low.as_str()) || s.chars().all(|c| c.is_ascii_digit()) {
        return;
    }
    names.push(s);
}

/// Every symbol the scanner finds that the manifest does not classify.
pub fn unclassified(lib_root: &Path, manifest_text: &str) -> Vec<Key> {
    let known = parse_manifest(manifest_text);
    let mut missing = Vec::new();
    let mut stack = vec![lib_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let ext = p.extension().and_then(|s| s.to_str());
            if !matches!(ext, Some("f90") | Some("c")) {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&p) else {
                continue;
            };
            let rel = p
                .strip_prefix(lib_root)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            let names = if ext == Some("c") {
                scan_c(&src)
            } else {
                scan_fortran(&src)
            };
            for name in names {
                let k = Key {
                    file: rel.clone(),
                    name,
                };
                if !known.contains(&k) {
                    missing.push(k);
                }
            }
        }
    }
    missing.sort();
    missing.dedup();
    missing
}

#[cfg(test)]
mod c_tests {
    use super::*;

    /// The bug that made the first version of this scanner useless. Parens do not change
    /// BRACE depth, so every continuation line of a multi-line function signature sits at
    /// depth 0 and reads as a file-scope declaration — turning each parameter into a phantom
    /// symbol. On the real tree this produced seven phantoms from `q65.c:529` alone and
    /// buried the two symbols that actually matter.
    #[test]
    fn multiline_signature_params_are_not_symbols() {
        let src = r#"
int q65_decode(const qracode *pcode,
	       const float *pIntrinsics, const int *pAPMask,
	       const int *pAPSymbols, const int maxiters)
{
	return 0;
}
"#;
        assert_eq!(scan_c(src), Vec::<String>::new());
    }

    /// A function-local `static` has static storage duration and is shared between chains
    /// exactly like a file-scope one — it just gets a mangled name (`first.0`) in the object.
    /// Brace depth must NOT filter these out.
    #[test]
    fn function_local_static_is_a_symbol() {
        let src = r#"
void q65_enc_(int *a)
{
  static int first=1;
  int scratch;
  if (first) { first=0; }
}
"#;
        let got = scan_c(src);
        assert!(got.contains(&"first".to_string()), "got {got:?}");
        assert!(!got.contains(&"scratch".to_string()), "stack local leaked: {got:?}");
    }

    /// `const` is not excluded the way Fortran's `parameter` is, because a C `const` object
    /// still has storage and the linker may put it somewhere writable. Both of these are
    /// `const`-qualified in the real tree and both land in `.data` because they hold
    /// relocated pointers.
    #[test]
    fn const_qualified_storage_is_still_a_symbol() {
        let src = r#"
static const ppd_uniform pd_uniform_tab[7] = {
	pd_uniform1, pd_uniform2
};
const qracode qra_13_64_64_irr_e = {
	13, 64
};
"#;
        let got = scan_c(src);
        assert!(got.contains(&"pd_uniform_tab".to_string()), "got {got:?}");
        assert!(got.contains(&"qra_13_64_64_irr_e".to_string()), "got {got:?}");
    }

    /// Function declarations are not storage; an array of function pointers behind a typedef
    /// is. The discriminator is a paren in the declarator, and `np_fwht_tab` (npfwht.c:38)
    /// has none — it is writable storage and is not even `const`.
    #[test]
    fn functions_out_function_pointer_tables_in() {
        let src = r#"
static void np_fwht2(float *dst, float *src);
static pnp_fwht np_fwht_tab[7] = {
	np_fwht1, np_fwht2
};
"#;
        assert_eq!(scan_c(src), vec!["np_fwht_tab".to_string()]);
    }

    /// `extern` refers to storage defined elsewhere and `typedef` defines a type; neither
    /// allocates, and counting them would demand a manifest row in every file that sees the
    /// header.
    #[test]
    fn extern_and_typedef_allocate_nothing() {
        let src = r#"
extern int q65_llh;
typedef void (*pnp_fwht)(float*, float*);
"#;
        assert_eq!(scan_c(src), Vec::<String>::new());
    }

    /// Comments must not contribute names. `q65_subs.c` carries the Fortran interface it
    /// implements inside a block comment, complete with declaration-shaped lines.
    #[test]
    fn comments_contribute_nothing() {
        let src = r#"
/*
   real s3prob(LL,NN)    !Symbol-value probabilities
   static int decoy;
*/
// static int decoy2;
static int real_one;
"#;
        assert_eq!(scan_c(src), vec!["real_one".to_string()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_symbol_tables() {
        let m = r#"
[[symbol]]
name = "msg0"
file = "ft8/ft8_a7.f90"
line = 11
class = 1

[[symbol]]
name = "jseq"
file = "ft8/ft8_a7.f90"
line = 14
class = 1
"#;
        let k = parse_manifest(m);
        assert_eq!(k.len(), 2);
        assert!(k.contains(&Key {
            file: "ft8/ft8_a7.f90".into(),
            name: "msg0".into()
        }));
    }

    #[test]
    fn finds_module_scope_declarations() {
        let src = "\
module ft8_a7
  parameter(MAXDEC=200)
  real dt0(MAXDEC,0:1,0:1)
  character*37 msg0(MAXDEC,0:1,0:1)
  integer :: jseq
contains
  subroutine foo()
    real scratch(100)
  end subroutine
end module
";
        let n = scan_fortran(src);
        assert!(n.contains(&"jseq".to_string()), "{n:?}");
        // A local INSIDE contains is not module-scope state — must not be swept in.
        assert!(!n.contains(&"scratch".to_string()), "{n:?}");
    }

    #[test]
    fn finds_saved_locals_inside_subroutines() {
        // The classic SAVEd-local idiom: state that a naive module-scope-only scan misses.
        let src = "\
subroutine bar()
  real x(10)
  save x
  data first/.true./
end subroutine
";
        let n = scan_fortran(src);
        assert!(n.contains(&"x".to_string()), "{n:?}");
        assert!(n.contains(&"first".to_string()), "{n:?}");
    }

    #[test]
    fn ignores_comments_and_type_keywords() {
        let src = "\
module m
  ! save this is a comment, not a declaration
  integer :: alpha
end module
";
        let n = scan_fortran(src);
        assert_eq!(n, vec!["alpha".to_string()], "{n:?}");
    }

    #[test]
    fn a_new_unclassified_symbol_is_reported() {
        // The whole point: a vendor refresh adds state, the manifest does not know it.
        let manifest = "[[symbol]]\nname = \"known\"\nfile = \"a.f90\"\nline = 1\nclass = 1\n";
        let known = parse_manifest(manifest);
        assert!(known.contains(&Key {
            file: "a.f90".into(),
            name: "known".into()
        }));
        assert!(!known.contains(&Key {
            file: "a.f90".into(),
            name: "brand_new".into()
        }));
    }
}
