//! ⭐ **Every built-in CW macro expands with nothing in braces left for the air.** The CW
//! cockpit sends its keys' text to the engine, which fills the tokens (`cw::expand` behind
//! `preview_cw`/`send_cw`); a token the expander does not know is left as written, so a built-in
//! naming one would key the braces' contents as a bare word. The texts are read out of the shipped
//! sets — the everyday and POTA sets in `components/CwCockpit.tsx`, the contest and Field Day
//! layouts in `features/esmRoles.ts` — so a new key or token is checked the day it ships.
//!
//! RTTY and PSK expand in the UI (`expandRttyMacro`, `expandPskMacro`), and their built-ins are
//! checked the same way in `rttyMacros.test.ts` and `pskMacros.test.ts`.

use tempo_app::engine::Engine;

const COCKPIT: &str = include_str!("../../../ui/src/components/CwCockpit.tsx");
const LAYOUTS: &str = include_str!("../../../ui/src/features/esmRoles.ts");

/// Every `text: '…'` in `src` between `from` and `to`.
fn texts(src: &str, from: &str, to: &str) -> Vec<String> {
    let at = src.find(from).unwrap_or_else(|| panic!("no {from}"));
    let body = &src[at..];
    let body = &body[..body.find(to).unwrap_or_else(|| panic!("no {to}"))];
    body.lines()
        .filter_map(|l| l.split("text: '").nth(1))
        .map(|t| t[..t.find('\'').expect("the text closes on its line")].to_string())
        .collect()
}

#[test]
fn every_built_in_cw_macro_expands_with_no_token_left() {
    let mut all = texts(
        COCKPIT,
        "const DEFAULT_MACROS: CwMacro[] = [",
        "export const CW_CONTEST_SETS",
    );
    let everyday_and_pota = all.len();
    // The contest set with a report, the one without, and Field Day's.
    all.extend(texts(
        LAYOUTS,
        "export const CW_CONTEST_LAYOUT",
        "export function cwBuiltInRoles",
    ));
    assert!(
        everyday_and_pota >= 16,
        "the everyday and POTA sets: {all:?}"
    );
    assert_eq!(
        all.len(),
        everyday_and_pota + 24,
        "the three contest layouts: {all:?}"
    );
    assert!(
        all.iter().any(|t| t.contains("{RST}")),
        "the parser reads the report keys"
    );

    let mut e = Engine::new("W9XYZ", "EN52", 0);
    e.select_peer("K9AAA");
    for rst in ["", "549"] {
        e.set_cw_rst_sent(rst.into());
        for text in &all {
            let keyed = e.preview_cw(text);
            assert!(
                !keyed.contains(['{', '}']),
                "{text:?} keys {keyed:?} with the report box at {rst:?}"
            );
        }
    }
}
