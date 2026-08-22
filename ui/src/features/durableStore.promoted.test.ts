// What survives a reinstall, and what deliberately does not.
//
// Operator report, 2026-08-21, upgrading to 1.7.5: "the views seem to be reset (I had hidden
// things from the Panels selection but they all seem to be back). Additionally, I had turned
// off items I don't use (CW, SSTV, etc.) and those were re-enabled." His settings.json came
// through fine — because those two things never lived there. They lived in the WebView2
// localStorage, which this file's own header describes as lost by "a WebView2 data reset, an
// uninstall-then-reinstall rather than an upgrade in place, or a change of asset protocol".
//
// The #28 audit drew the line between "REAL OPERATOR DATA" and cosmetics and put both of these
// on the cosmetic side, calling localStorage "the right home for a collapsed-panel flag". That
// report is the evidence the line was in the wrong place: WHICH MODES YOU OPERATE and WHICH
// PANES YOU HAVE HIDDEN are a setup you spent time on, not a collapsed-panel flag.
//
// The distinction that remains, and it is the interesting one: the MAIN window's layout is
// durable, a DETACHED panel's is not. A popped-out panel really is per-surface chrome, and
// making it durable would put a second window's arrangement into a per-profile store where it
// would fight the first window's.
import { describe, it, expect } from 'vitest'
import { isDurable, DURABLE_KEYS } from './durableStore'
import { panelStorageKey } from './panelState'

describe('what a reinstall must not take away', () => {
  it('keeps which modes the operator turned off', () => {
    expect(isDurable('nexus.features.v1')).toBe(true)
  })

  it('keeps the MAIN window pane layout, for every cockpit', () => {
    // By prefix, not by an entry per view — a list would have to be edited each time a
    // cockpit is added, and the cockpit somebody forgets is the one whose operator loses
    // their layout.
    for (const view of ['operate', 'phone', 'cw', 'rtty', 'psk', 'sstv', 'aprs']) {
      expect(isDurable(panelStorageKey(view, 'main')), `${view} main`).toBe(true)
    }
  })

  it('does NOT keep a detached panel layout — that is per-surface chrome', () => {
    expect(isDurable(panelStorageKey('operate', 'w2'))).toBe(false)
    expect(isDurable(panelStorageKey('phone', 'panel-3'))).toBe(false)
  })

  it('control: an ordinary cosmetic key is still not durable', () => {
    // Without this the prefix rule could be matching everything and all four tests above
    // would pass on a function that returns true unconditionally.
    expect(isDurable('nexus.theme')).toBe(false)
    expect(isDurable('nexus.panels.operate')).toBe(false) // no instance — not a layout key
    expect(isDurable('')).toBe(false)
  })

  it('control: the pre-existing durable keys are untouched', () => {
    // The promotion must not have disturbed what the #28 audit already protected.
    for (const k of ['nexus.watchlist', 'nexus.profiles', 'nexus-ui-scale-cap']) {
      expect(DURABLE_KEYS).toContain(k)
      expect(isDurable(k)).toBe(true)
    }
  })
})
