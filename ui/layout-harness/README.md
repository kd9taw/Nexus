# Layout harness — real-geometry gate for top-bar changes

Six top-bar rounds in one night (2026-08-09) shipped DOM-test-green layouts that were
wrong ON SCREEN, because jsdom computes no flex layout: it cannot see two auto margins
splitting a row's slack, a tail group wrapping to a lone left-aligned line, or a chip
rendered in an unreadable corner. This harness is the check that ends that class.

**Use it before shipping any change to `.topbar*` CSS or the TopBar row structure:**

1. `cp ../src/styles.css .` (the harness links the real sheet — keep it current)
2. Mirror any DOM-structure change into `topbar.html` (it carries the real classes)
3. `python3 -m http.server 8799` here, open `http://localhost:8799/topbar.html`
4. In the console: `measure()` at several container widths
   (`document.querySelector('.app').style.width='1024px'` … `'3436px'`)

The assertion that must hold (adversarial-review verdict, wf_c2df16d3):
- `sameLine` true at ≥1366 (chips on the pills line)
- `chipsToTxGap` == resolved `--space-3` (chips joined to the Tx cluster, never floating)
- `txFlushRight` == 0 (the control run ends at the row edge)
- exactly ONE slack region, between the mode pills and the right block
- at 1024 the right block wraps as ONE unit, still flush right
