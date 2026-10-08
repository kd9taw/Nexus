// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest'
import { act, renderHook } from '@testing-library/react'
// Namespace import as well as the named ones: the stop-line guard below scans the module's
// own exports for vocabularies, so it cannot be narrowed by editing an import list.
import * as panelState from './panelState'
import {
  ALL_PANEL_VOCABULARIES,
  BOX_IDS,
  STOP_CONTROL_WORDS,
  MIN_SHARE,
  OPERATE_PANELS,
  WATERFALL_DETACHED_KEY,
  coercePanelLayout,
  loadPanelLayout,
  panelStateIn,
  panelStorageKey,
  redockAllStalePopouts,
  redockStalePopouts,
  savePanelLayout,
  seamShares,
  usePanelLayout,
  SSTV_PANELS,
  PHONE_PANELS,
  CW_PANELS,
  RTTY_PANELS,
  type PanelLayout,
  type PanelVocabulary,
  type OperatePanelId,
} from './panelState'
import { scopedKey, windowInstance } from './windowScope'
import { SLOT_IDS } from './connectConfig'

const KEY = panelStorageKey('operate')

beforeEach(() => {
  localStorage.clear()
})

describe('panel storage key', () => {
  it('is surface-scoped: nexus.panels.<view>.<instance>', () => {
    // jsdom has no ?panel= in the URL, so this window is the main surface.
    expect(windowInstance()).toBe('main')
    expect(KEY).toBe('nexus.panels.operate.main')
    // A torn-off surface gets its OWN record — the docked/popped collision the
    // app-global nexus.waterfall.detached flag used to have.
    expect(panelStorageKey('operate', 'w1')).toBe('nexus.panels.operate.w1')
    expect(scopedKey('nexus.panels.operate', 'global', 'w1')).toBe('nexus.panels.operate')
  })
})

describe('coercePanelLayout', () => {
  it('treats an absent panel as docked (a panel added later ships visible, unless its vocabulary ships it hidden)', () => {
    const l = loadPanelLayout(OPERATE_PANELS)
    expect(l.state.waterfall).toBeUndefined()
    expect(l.state.bandActivity).toBeUndefined()
  })

  it('coerces junk to the stock layout instead of throwing', () => {
    for (const junk of [null, 42, 'nope', [], { state: 7, share: 'x' }]) {
      expect(coercePanelLayout(OPERATE_PANELS, junk)).toEqual({ v: 2, state: {}, share: {} })
    }
  })

  it('recovers the stock layout from an unparseable stored record', () => {
    localStorage.setItem(KEY, '{not json')
    expect(loadPanelLayout(OPERATE_PANELS)).toEqual({ v: 2, state: {}, share: {} })
  })

  it('drops unknown panel ids, unknown states, and non-positive shares', () => {
    const l = coercePanelLayout(OPERATE_PANELS, {
      v: 1,
      state: { waterfall: 'removed', stopTx: 'removed', bandActivity: 'gone' },
      share: { waterfall: 0.4, rxfreq: -1, stations: 'big', callRoster: Infinity },
    })
    expect(l.state).toEqual({ waterfall: 'removed' })
    expect(l.share).toEqual({ waterfall: 0.4 })
    // The whitelist is the vocabulary, so a hand-edited store cannot introduce an id
    // for a TX control that has no panel entry.
    expect('stopTx' in l.state).toBe(false)
  })

  it('clamps a loaded share into [MIN_SHARE, 2 − MIN_SHARE] — the range the writers enforce', () => {
    // setShare/setShares floor at MIN_SHARE and seamShares caps at 2 − MIN_SHARE, but
    // load accepted any v > 0 — so a hand-edited/foreign 1e-9 collapsed a pane to ~0
    // height on the one path the setters cannot guard.
    const l = coercePanelLayout(OPERATE_PANELS, {
      v: 1,
      state: {},
      share: { waterfall: 1e-9, bandActivity: 50, callRoster: 1.2 },
    })
    expect(l.share.waterfall).toBe(MIN_SHARE)
    expect(l.share.bandActivity).toBe(2 - MIN_SHARE)
    expect(l.share.callRoster).toBe(1.2)
  })
})

describe('persistence', () => {
  it('an explicit removal survives a reload', () => {
    const stored: PanelLayout<OperatePanelId> = {
      v: 1,
      state: { waterfall: 'removed' },
      share: {},
    }
    savePanelLayout(KEY, stored)
    expect(loadPanelLayout(OPERATE_PANELS).state.waterfall).toBe('removed')
  })

  it('a surface with no record of its own inherits another surface’s, and writes only its own', () => {
    // The surfaceGet contract, for the panel record: a torn-off copy opens on the operator's
    // layout instead of first-run defaults, and its first change makes the record its own
    // without touching the window it inherited from.
    savePanelLayout(panelStorageKey('operate', 'main'), { v: 1, state: { waterfall: 'removed' }, share: {} })
    expect(loadPanelLayout(OPERATE_PANELS, 'w1').state.waterfall, 'control: no inheritance unless asked').toBeUndefined()
    expect(loadPanelLayout(OPERATE_PANELS, 'w1', 'main').state.waterfall).toBe('removed')

    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS, 'w1', 'main'))
    expect(result.current.stateOf('waterfall')).toBe('removed')
    act(() => result.current.setPanelState('stations', 'removed'))
    expect(JSON.parse(localStorage.getItem(panelStorageKey('operate', 'w1'))!).state).toEqual({
      waterfall: 'removed',
      stations: 'removed',
    })
    expect(JSON.parse(localStorage.getItem(panelStorageKey('operate', 'main'))!).state).toEqual({ waterfall: 'removed' })

    // Once it has its own, the other surface no longer speaks for it.
    savePanelLayout(panelStorageKey('operate', 'main'), { v: 1, state: {}, share: {} })
    expect(loadPanelLayout(OPERATE_PANELS, 'w1', 'main').state.stations).toBe('removed')
  })
})

describe('nexus.waterfall.detached migration', () => {
  it('carries a popped-out waterfall into the record', () => {
    localStorage.setItem(WATERFALL_DETACHED_KEY, '1')
    expect(loadPanelLayout(OPERATE_PANELS).state.waterfall).toBe('popped')
    // …and persists it, so the bridge is not needed a second time.
    expect(JSON.parse(localStorage.getItem(KEY)!).state.waterfall).toBe('popped')
  })

  it('runs exactly once — a re-dock is never undone by the stale global flag', () => {
    localStorage.setItem(WATERFALL_DETACHED_KEY, '1')
    expect(loadPanelLayout(OPERATE_PANELS).state.waterfall).toBe('popped')
    // Operator re-docks (record back to stock) while the legacy flag is still '1'.
    localStorage.removeItem(KEY)
    expect(loadPanelLayout(OPERATE_PANELS).state.waterfall).toBeUndefined()
  })

  it('leaves the record alone when the flag was never set', () => {
    expect(loadPanelLayout(OPERATE_PANELS).state.waterfall).toBeUndefined()
    expect(localStorage.getItem(KEY)).toBeNull()
  })
})

describe('redockStalePopouts (fresh main-window boot)', () => {
  it('re-docks a stale pop-out but leaves an explicit removal alone', () => {
    savePanelLayout(KEY, {
      v: 1,
      state: { waterfall: 'popped', stations: 'removed' },
      share: {},
    } as PanelLayout<OperatePanelId>)
    redockStalePopouts(OPERATE_PANELS)
    const l = loadPanelLayout(OPERATE_PANELS)
    // No detached window survives a restart, so 'popped' would strand the operator on a
    // re-dock bar with nothing behind it.
    expect(l.state.waterfall).toBe('docked')
    expect(l.state.stations).toBe('removed')
  })

  it('does not write when there is nothing stale', () => {
    redockStalePopouts(OPERATE_PANELS)
    expect(localStorage.getItem(KEY)).toBeNull()
  })

  it('the boot clear covers EVERY vocabulary, not just the one with a re-dock bar', () => {
    // main.tsx used to run this on OPERATE_PANELS alone. Operate is the only cockpit with a
    // pop-out affordance AND a re-dock bar; in the other four a stored 'popped' renders the
    // pane DOCKED while its ⊞ entry reads "popped out", with no window to re-dock from and
    // nothing that ever clears it — so the record said it at every launch, forever.
    // Driven off ALL_PANEL_VOCABULARIES so a sixth cockpit is covered by being exported.
    for (const vocab of ALL_PANEL_VOCABULARIES) {
      const key = panelStorageKey(vocab.view)
      const id = vocab.panelIds[0]
      savePanelLayout(key, { v: 1, state: { [id]: 'popped' }, share: {} } as PanelLayout<string>)
    }
    redockAllStalePopouts()
    for (const vocab of ALL_PANEL_VOCABULARIES) {
      const id = vocab.panelIds[0]
      expect(
        loadPanelLayout(vocab).state[id],
        `"${vocab.view}" kept a stale pop-out across a boot — its ⊞ entry will read ` +
          '"popped out" over a pane rendering docked, at every launch',
      ).toBe('docked')
    }
  })
})

describe('usePanelLayout', () => {
  it('saves synchronously on change, so a remount keeps the removal', () => {
    const { result, unmount } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    expect(result.current.stateOf('waterfall')).toBe('docked')
    act(() => result.current.setPanelState('waterfall', 'removed'))
    // Written by the state updater itself — not by an effect that a remount could skip.
    expect(JSON.parse(localStorage.getItem(KEY)!).state.waterfall).toBe('removed')
    unmount()
    const again = renderHook(() => usePanelLayout(OPERATE_PANELS))
    expect(again.result.current.stateOf('waterfall')).toBe('removed')
  })

  it('undo restores the previous layout, once', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    expect(result.current.canUndo).toBe(false)
    act(() => result.current.setPanelState('rxfreq', 'removed'))
    expect(result.current.canUndo).toBe(true)
    act(() => result.current.undo())
    expect(result.current.stateOf('rxfreq')).toBe('docked')
    expect(result.current.canUndo).toBe(false)
  })

  it('reset puts every panel back and is itself undoable', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.setPanelState('waterfall', 'removed'))
    act(() => result.current.setPanelState('stations', 'removed'))
    act(() => result.current.reset())
    expect(result.current.stateOf('waterfall')).toBe('docked')
    expect(result.current.stateOf('stations')).toBe('docked')
    act(() => result.current.undo())
    expect(result.current.stateOf('waterfall')).toBe('removed')
    expect(result.current.stateOf('stations')).toBe('removed')
  })

  it('setLayout replaces the whole record in ONE undoable step, coerced like a load (a layout preset)', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.setPanelState('waterfall', 'removed'))
    act(() => result.current.setShare('rxfreq', 1.6))
    act(() =>
      result.current.setLayout({
        v: 1,
        state: { stations: 'removed', bogus: 'removed' } as PanelLayout<OperatePanelId>['state'],
        share: { bandActivity: 1e-9 },
      }),
    )
    // Replaced, not merged: the waterfall the old record hid is back, the old share is gone.
    expect(result.current.stateOf('waterfall')).toBe('docked')
    expect(result.current.stateOf('stations')).toBe('removed')
    expect(result.current.shareOf('rxfreq')).toBe(1)
    // Coerced: an unknown id is not written, a collapsing share is clamped to the floor.
    expect(result.current.shareOf('bandActivity')).toBe(MIN_SHARE)
    expect(JSON.parse(localStorage.getItem(KEY)!)).toEqual({ v: 2, state: { stations: 'removed' }, share: { bandActivity: MIN_SHARE } })
    // One Undo brings back the whole previous record.
    act(() => result.current.undo())
    expect(result.current.stateOf('waterfall')).toBe('removed')
    expect(result.current.stateOf('stations')).toBe('docked')
    expect(result.current.shareOf('rxfreq')).toBe(1.6)
  })
})

describe('share (seam resize)', () => {
  it('defaults to 1, setShare persists synchronously and clamps to MIN_SHARE', () => {
    const { result, unmount } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    expect(result.current.shareOf('rxfreq')).toBe(1)
    act(() => result.current.setShare('rxfreq', 1.6))
    expect(result.current.shareOf('rxfreq')).toBe(1.6)
    // Saved by the updater itself, so a remount keeps the size (the remount-loss bug class).
    expect(JSON.parse(localStorage.getItem(KEY)!).share.rxfreq).toBe(1.6)
    // A seam can never drive a pane below MIN_SHARE — removal is the only route to gone.
    act(() => result.current.setShare('rxfreq', 0))
    expect(result.current.shareOf('rxfreq')).toBe(MIN_SHARE)
    unmount()
    const again = renderHook(() => usePanelLayout(OPERATE_PANELS))
    expect(again.result.current.shareOf('rxfreq')).toBe(MIN_SHARE)
  })

  it('setShares redistributes two adjacent panes in ONE undoable step', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.setShares({ bandActivity: 1.4, rxfreq: 0.6 }))
    expect(result.current.shareOf('bandActivity')).toBe(1.4)
    expect(result.current.shareOf('rxfreq')).toBe(0.6)
    // A seam drag is a single history entry — one undo restores BOTH panes.
    act(() => result.current.undo())
    expect(result.current.shareOf('bandActivity')).toBe(1)
    expect(result.current.shareOf('rxfreq')).toBe(1)
  })

  it('setShares with null clears a pane back to its stock share, in the same ONE undoable step', () => {
    // A divider's reset: the stock split is a SHEET default the record never holds, so the only
    // way back to it is for the record to hold nothing for the pair.
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.setShares({ bandActivity: 1.4, rxfreq: 0.6, stations: 1.2 }))
    act(() => result.current.setShares({ bandActivity: null, rxfreq: null }))
    expect(result.current.layout.share).toEqual({ stations: 1.2 })
    expect(JSON.parse(localStorage.getItem(KEY)!).share).toEqual({ stations: 1.2 })
    act(() => result.current.undo())
    expect(result.current.layout.share).toEqual({ bandActivity: 1.4, rxfreq: 0.6, stations: 1.2 })
  })

  it('reset clears shares back to default', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.setShare('rxfreq', 1.8))
    act(() => result.current.reset())
    expect(result.current.shareOf('rxfreq')).toBe(1)
  })
})

describe('cols (the grid cockpits’ column dividers, layout L2)', () => {
  const PHONE_KEY = panelStorageKey('phone')
  const stored = () => JSON.parse(localStorage.getItem(PHONE_KEY)!)

  it('a v1 record — every record saved before the column dividers — loads on the default columns, its panes and shares as stored', () => {
    localStorage.setItem(PHONE_KEY, JSON.stringify({ v: 1, state: { spots: 'docked', receiver: 'removed' }, share: { spots: 1.3, needed: 0.7 } }))
    const l = loadPanelLayout(PHONE_PANELS)
    expect(l.cols, 'a record with no columns grew some').toBeUndefined()
    expect(l.state).toEqual({ spots: 'docked', receiver: 'removed' })
    expect(l.share).toEqual({ spots: 1.3, needed: 0.7 })
    expect(l.v).toBe(2)
  })

  it('a v2 record keeps its columns across a reload, next to its panes and shares', () => {
    const { result, unmount } = renderHook(() => usePanelLayout(PHONE_PANELS))
    act(() => result.current.setShares({ spots: 1.2, needed: 0.8 }))
    act(() => result.current.setCols!({ a: 1.3, b: 0.7, log: 560 }))
    // The columns sit in a key of their own, beside the two an older build reads.
    expect(stored()).toEqual({ v: 2, state: {}, share: { spots: 1.2, needed: 0.8 }, cols: { a: 1.3, b: 0.7, log: 560 } })
    unmount()
    const again = renderHook(() => usePanelLayout(PHONE_PANELS))
    expect(again.result.current.layout.cols).toEqual({ a: 1.3, b: 0.7, log: 560 })
    expect(again.result.current.layout.share).toEqual({ spots: 1.2, needed: 0.8 })
  })

  it('clamps stored columns on load: the fr pair into the writers’ range, the log width to whole px, junk dropped', () => {
    const l = coercePanelLayout(PHONE_PANELS, {
      v: 2,
      state: {},
      share: {},
      cols: { a: 1e-9, b: 50, log: 612.6, bogus: 3 },
    })
    expect(l.cols).toEqual({ a: MIN_SHARE, b: 2 - MIN_SHARE, log: 613 })
    for (const junk of [{ a: -1, b: 'wide', log: Infinity }, { log: 0 }, 'cols', null]) {
      expect(coercePanelLayout(PHONE_PANELS, { v: 2, state: {}, share: {}, cols: junk }).cols, JSON.stringify(junk)).toBeUndefined()
    }
  })

  it('setCols is ONE undoable step; null puts one column back to the default', () => {
    const { result } = renderHook(() => usePanelLayout(PHONE_PANELS))
    act(() => result.current.setCols!({ a: 1.4, b: 0.6 }))
    act(() => result.current.setCols!({ log: 500 }))
    expect(result.current.layout.cols).toEqual({ a: 1.4, b: 0.6, log: 500 })
    // A divider's release is a single history entry: one Undo takes back exactly that release.
    act(() => result.current.undo())
    expect(result.current.layout.cols).toEqual({ a: 1.4, b: 0.6 })
    act(() => result.current.setCols!({ a: null, b: null }))
    expect(result.current.layout.cols, 'a reset leaves no column of its own behind').toBeUndefined()
    expect(stored().cols).toBeUndefined()
    // A write that could not have come from a divider changes nothing.
    act(() => result.current.setCols!({ log: 480 }))
    act(() => result.current.setCols!({ log: Number.NaN }))
    expect(result.current.layout.cols).toEqual({ log: 480 })
  })

  it('⊞ Reset puts the default columns back and Undo restores them (with everything else Reset took)', () => {
    const { result } = renderHook(() => usePanelLayout(PHONE_PANELS))
    act(() => result.current.setPanelState('receiver', 'removed'))
    act(() => result.current.setCols!({ a: 1.5, b: 0.5, log: 640 }))
    act(() => result.current.reset())
    expect(result.current.layout.cols).toBeUndefined()
    expect(stored().cols).toBeUndefined()
    act(() => result.current.undo())
    expect(result.current.layout.cols).toEqual({ a: 1.5, b: 0.5, log: 640 })
    expect(result.current.stateOf('receiver')).toBe('removed')
    expect(stored().cols).toEqual({ a: 1.5, b: 0.5, log: 640 })
  })
})

describe('cockpit vocabularies (TX-safety: the STOP line)', () => {
  // THE RULE (panelState.ts header): the operator must never be unable to stop a
  // transmission — mechanically, in every cockpit at least one control that stops one renders
  // OUTSIDE every ⊞-removable pane, so no menu entry, stored value or coercion rule can reach
  // it. Whether a pane can START one is not this guard's business, nor the rule's: six of the
  // entries below are senders and every one of them is hideable.
  //
  // NOR IS THIS GUARD ABOUT PANE-RESIDENT STOPS. Two panes host a stop control of their own
  // (`voiceKeyer`'s ■ Stop, `stream`'s "Auto on" toggle) and both are legitimate entries —
  // the control goes away with the pane, which is allowed because the ones outside every pane
  // do not. This guard only refuses an id NAMED for a stop control, which is a different and
  // much smaller claim.
  //
  // THIS IS THE NAME HALF OF THE ENFORCEMENT, AND IT IS ONLY THE NAME HALF. It reads ids.
  // It cannot see that a control is WIRED to an id, so a vocabulary id called `dsp` gating
  // the PTT row passes it untouched. The wiring half is components/stop-line.test.tsx
  // (Phone/CW/RTTY/SSTV, real headers, every id hidden) plus the same sweep for Operate in
  // OperateCockpit.structure.test.tsx. Both halves are required and neither is the rule.
  //
  // It also has to cover EVERY vocabulary, which is exactly what it did not do: the list of
  // cases named the four Phase-3 cockpits and never Operate, the first consumer — so
  // `'ptt'` in OPERATE_PANEL_IDS kept the whole 2081-test suite green (mutation, 2026-08-03)
  // while CLAUDE.md claimed the rule was "enforced by computation". Driving off
  // ALL_PANEL_VOCABULARIES fixes that case; the next test makes the ARRAY itself honest.
  it.each(ALL_PANEL_VOCABULARIES.map((v) => [v.view, v] as const))(
    '%s vocabulary has no id NAMED for a control that stops a transmission',
    (_view, vocab) => {
      const norm = (s: string) => s.toLowerCase().replace(/[^a-z]/g, '')
      for (const id of vocab.panelIds) {
        expect(
          (STOP_CONTROL_WORDS as readonly string[]).includes(norm(id)),
          `"${id}" is in the ${vocab.view} vocabulary — an id NAMED for a stop control reads ` +
            'as one of the controls that hold the stop line up, and those have no ⊞ entry. ' +
            '(A pane may host a stop of its own — two do — so this is a naming rule, not the ' +
            'falsified fourth wording that forbade any hideable stop at all.)',
        ).toBe(false)
      }
    },
  )

  // ⊞ ARRANGE (layout L3) can give a pane a PLACE, and that is one more thing an id can do. Every id
  // a placement can hold must be one of its vocabulary's own ids — so the name check above has
  // already read it — and none may be NAMED for a stop control, pinned or not. Checked against every
  // vocabulary that arranges, and the check itself is proven to fire on a planted spec.
  const arrangeNameProblems = (vocab: { view: string; panelIds: readonly string[]; arrange?: { columns: Record<string, readonly string[]>; pinned: readonly string[]; stockMerged?: readonly string[] } }) => {
    const norm = (s: string) => s.toLowerCase().replace(/[^a-z]/g, '')
    const a = vocab.arrange
    if (!a) return []
    const named = [...Object.values(a.columns).flat(), ...a.pinned, ...(a.stockMerged ?? [])]
    return named.flatMap((id) => [
      ...(vocab.panelIds.includes(id) ? [] : [`${vocab.view}: "${id}" can be placed but is not a vocabulary id`]),
      ...((STOP_CONTROL_WORDS as readonly string[]).includes(norm(id)) ? [`${vocab.view}: "${id}" is named for a stop control`] : []),
    ])
  }

  it('every id ⊞ Arrange can place is a vocabulary id, and none is named for a stop control (layout L3)', () => {
    const arranging = ALL_PANEL_VOCABULARIES.filter((v) => v.arrange)
    expect(arranging.map((v) => v.view), 'no vocabulary arranges: this guard would be reading nothing').toContain('phone')
    for (const vocab of arranging) expect(arrangeNameProblems(vocab)).toEqual([])
    // Control: a spec that could place a stop control, or an id its vocabulary does not have.
    expect(
      arrangeNameProblems({ view: 'planted', panelIds: ['decode'], arrange: { columns: { a: ['decode', 'ptt'], b: [], log: [] }, pinned: ['stopTx'] } }),
    ).toEqual([
      'planted: "ptt" can be placed but is not a vocabulary id',
      'planted: "ptt" is named for a stop control',
      'planted: "stopTx" can be placed but is not a vocabulary id',
      'planted: "stopTx" is named for a stop control',
    ])
  })

  it('ALL_PANEL_VOCABULARIES holds every vocabulary this module exports', () => {
    // The backstop above is only as wide as this array, and an array is a thing somebody
    // forgets. So do not trust it: find every export that IS a vocabulary and require it to
    // be in there. A sixth cockpit is name-guarded the moment it is exported, not when
    // somebody remembers to come back here.
    type Vocab = { view: string; panelIds: readonly string[] }
    const isVocab = (v: unknown): v is Vocab =>
      !!v &&
      typeof v === 'object' &&
      typeof (v as { view?: unknown }).view === 'string' &&
      Array.isArray((v as { panelIds?: unknown }).panelIds)
    // Collected with an `if` rather than `.filter(isVocab)`: the module namespace is a
    // union of every export (numbers, functions, consts), and `filter`'s narrowing overload
    // needs the guarded type to extend the element type, which it does not — so `filter`
    // silently returns the un-narrowed union and `v.view` below stops type-checking.
    const exported: Vocab[] = []
    for (const v of Object.values(panelState)) if (isVocab(v)) exported.push(v)
    // A floor, not an equality: the point is that a NEW vocabulary is caught by the loop
    // below with a message that names it, not by a count that says only "6 ≠ 5".
    expect(
      exported.length,
      'fewer vocabularies found than the five that exist — the scan itself has gone blind, ' +
        'and a blind scan passes every check under it',
    ).toBeGreaterThanOrEqual(5)
    for (const v of exported) {
      expect(
        ALL_PANEL_VOCABULARIES.includes(v as never),
        `the "${v.view}" vocabulary is exported but missing from ALL_PANEL_VOCABULARIES — ` +
          'the stop-line name backstop never looks at it',
      ).toBe(true)
    }
  })

  it('every stop control the cockpits actually render is in STOP_CONTROL_WORDS', () => {
    // The word list is the backstop's whole reach, so pin the names that exist today: PTT,
    // Stop TX, Tune, the TX-enable latch and the abort verbs. If a control is renamed in a
    // cockpit and not renamed here, the backstop quietly narrows — this fails first.
    for (const w of ['ptt', 'stoptx', 'stop', 'tune', 'halt', 'halttx', 'abort', 'enabletx']) {
      expect((STOP_CONTROL_WORDS as readonly string[]).includes(w), `${w} dropped`).toBe(true)
    }
  })

  it('Connect’s vocabulary is its slot list — visibility is per SLOT, placement stays in connectConfig', () => {
    expect([...panelState.CONNECT_PANELS.panelIds]).toEqual([...SLOT_IDS])
    expect(ALL_PANEL_VOCABULARIES).toContain(panelState.CONNECT_PANELS)
  })

  it('lists the expected content panels per cockpit', () => {
    expect([...SSTV_PANELS.panelIds]).toEqual(['scope', 'rfScope', 'txcompose', 'gallery'])
    expect([...PHONE_PANELS.panelIds]).toEqual([
      'scope', 'rigscope', 'txmeters', 'receiver', 'transmitter', 'bandActivity', 'voiceKeyer',
      'spots', 'needed', 'box1', 'box2', 'box3', 'box4', 'box5', 'box6',
    ])
    expect([...RTTY_PANELS.panelIds]).toEqual(['scope', 'rfScope', 'stream'])
    expect([...CW_PANELS.panelIds]).toEqual([
      'scope', 'scopeCtl', 'dsp', 'txmeters', 'rxdsp', 'bandActivity', 'copilot', 'decode', 'sent',
      'spots', 'needed', 'box1', 'box2', 'box3', 'box4', 'box5', 'box6',
    ])
  })

  it('every cockpit with a spectrum strip can hide it, and it is SHOWN until he says otherwise', () => {
    // The operator's ask (2026-08-16): "add the waterfall in each window as an option to
    // remove in the panels section — leave it ON by default, give me the option to turn it
    // off." Default-shown is not a separate mechanism: an absent state means 'docked'
    // (coercePanelLayout above), so a strip that has never been ticked renders, and only an
    // EXPLICIT removal hides it — which is what makes the hide survive a reload.
    //
    // Operate's entry is `waterfall`, not `scope`, and deliberately: that id shipped in
    // 0.15.0, carries the pop-out ('popped') state and is the one migrateWaterfallDetached
    // keys on. Renaming it would drop every stored preference and break the re-dock. The
    // four cockpits that gained a strip entry here share ONE id.
    for (const [vocab, id] of [
      [OPERATE_PANELS, 'waterfall'],
      [PHONE_PANELS, 'scope'],
      [CW_PANELS, 'scope'],
      [RTTY_PANELS, 'scope'],
      [SSTV_PANELS, 'scope'],
    ] as const) {
      expect(
        (vocab.panelIds as readonly string[]).includes(id),
        `the ${vocab.view} cockpit renders a spectrum strip with no ⊞ entry to hide it`,
      ).toBe(true)
      // Nothing stored ⇒ shown. The strip is display + click-to-tune and hosts no stop
      // control in any cockpit, which is what admits it under THE STOP LINE.
      expect(coercePanelLayout(vocab, {}).state[id]).toBeUndefined()
    }
  })

  it('Phone can hide the voice keyer — it starts overs, it is not the way you end one', () => {
    // The pane TRANSMITS (F1–F6 play a canned message with PTT keyed), which is why the
    // blunt rule kept it out. It is admissible under the narrowed one because hiding it
    // IS a stop: the unmount cleanup calls stopVoice, so the hide cannot strand you keyed.
    // The abort itself is proven at the real site in PhoneCockpit.keyerHide.test.tsx.
    expect((PHONE_PANELS.panelIds as readonly string[]).includes('voiceKeyer')).toBe(true)
  })
})

describe('panes a vocabulary ships HIDDEN (#345: Phone Spots and Needed)', () => {
  // The operator's pick, verbatim: "Hidden, add via ⊞ Panels (Recommended)" — "Nobody's Phone
  // screen changes on update. Operators who want them tick Spots / Needed in ⊞ Panels, then size
  // them with the pane dividers." An absent state used to mean 'docked' for EVERY id, so a new id
  // shipped visible, including to every operator whose Phone record is already on disk. The
  // vocabulary's `defaultRemoved` is the answer, and each case below drives one reader of an
  // absent state: `stateOf`, `undoRemoves`, `reset`, and a record written by today's build.
  const PHONE_KEY = panelStorageKey('phone')
  // The two feeds, and the six boxes (2026-10-07), which ship hidden for the same reason.
  const HIDDEN = ['spots', 'needed', ...BOX_IDS] as const

  it('a FRESH record shows Spots, Needed and the boxes hidden, and every other Phone pane docked', () => {
    const { result } = renderHook(() => usePanelLayout(PHONE_PANELS))
    for (const id of HIDDEN) expect(result.current.stateOf(id), `${id} ships visible`).toBe('removed')
    for (const id of PHONE_PANELS.panelIds) {
      if ((HIDDEN as readonly string[]).includes(id)) continue
      expect(result.current.stateOf(id), `"${id}" changed its default`).toBe('docked')
    }
    // A READING of absence, not a migration: nothing is written to get there, so a later
    // release can still change its mind about an id nobody ticked.
    expect(localStorage.getItem(PHONE_KEY)).toBeNull()
  })

  it("a record stored by today's build (no entry for either) shows both hidden too", () => {
    savePanelLayout(PHONE_KEY, {
      v: 1,
      state: { receiver: 'removed', voiceKeyer: 'docked' },
      share: { receiver: 1.3 },
    } as PanelLayout<string>)
    const { result } = renderHook(() => usePanelLayout(PHONE_PANELS))
    for (const id of HIDDEN) expect(result.current.stateOf(id), `${id} appeared on an upgrade`).toBe('removed')
    // …and the operator's own choices in that record are exactly what they were.
    expect(result.current.stateOf('receiver')).toBe('removed')
    expect(result.current.stateOf('voiceKeyer')).toBe('docked')
    expect(result.current.stateOf('bandActivity')).toBe('docked')
    expect(result.current.shareOf('receiver')).toBe(1.3)
  })

  it('a tick docks it, and the tick is stored and survives a restart', () => {
    const { result, unmount } = renderHook(() => usePanelLayout(PHONE_PANELS))
    act(() => result.current.setPanelState('spots', 'docked'))
    expect(result.current.stateOf('spots')).toBe('docked')
    expect(result.current.stateOf('needed')).toBe('removed')
    expect(JSON.parse(localStorage.getItem(PHONE_KEY)!).state.spots).toBe('docked')
    unmount()
    const again = renderHook(() => usePanelLayout(PHONE_PANELS))
    expect(again.result.current.stateOf('spots')).toBe('docked')
    expect(again.result.current.stateOf('needed')).toBe('removed')
  })

  it('⊞ Reset returns them to hidden, and every other pane to docked', () => {
    const { result } = renderHook(() => usePanelLayout(PHONE_PANELS))
    act(() => result.current.setPanelState('spots', 'docked'))
    act(() => result.current.setPanelState('needed', 'docked'))
    act(() => result.current.setPanelState('receiver', 'removed'))
    act(() => result.current.reset())
    expect(result.current.stateOf('spots')).toBe('removed')
    expect(result.current.stateOf('needed')).toBe('removed')
    expect(result.current.stateOf('receiver')).toBe('docked')
    // Reset is undoable like any change: one Undo brings the ticks back.
    act(() => result.current.undo())
    expect(result.current.stateOf('spots')).toBe('docked')
    expect(result.current.stateOf('needed')).toBe('docked')
  })

  it('Undo takes a tick back, and `undoRemoves` knows that doing so hides the pane', () => {
    const { result } = renderHook(() => usePanelLayout(PHONE_PANELS))
    act(() => result.current.setPanelState('needed', 'docked'))
    // `undoRemoves` is what ⊞ Undo reads to warn before a hide. Read with the old default it
    // would say this undo removes nothing, when it takes the pane off the screen.
    expect(result.current.undoRemoves).toEqual(['needed'])
    act(() => result.current.undo())
    expect(result.current.stateOf('needed')).toBe('removed')
    // The undone record is the stock one: an absent entry, not an explicit 'removed'.
    expect(JSON.parse(localStorage.getItem(PHONE_KEY)!).state.needed).toBeUndefined()
  })

  it('Phone and CW ship their Spots and Needed hidden, the five digital cockpits their RF scope pane — and each hidden id is its own', () => {
    // ⚠️ `features/connectPresets.ts` reads Connect's record RAW (`state[s] === 'removed'`) and
    // writes it the same way, i.e. it takes absent to mean docked. That stays true only while
    // Connect lists nothing here: before a vocabulary gains a hidden pane, route every raw
    // reader of its record through `panelStateIn`. (CW's record has no raw reader: CwCockpit and
    // panelHost read it through `stateOf`; nor do the five digital cockpits' — App and
    // DetachedPanel build theirs with usePanelLayout, checked 2026-10-04.)
    const shipsHidden = ALL_PANEL_VOCABULARIES.filter((v) => (v.defaultRemoved ?? []).length > 0)
    expect(shipsHidden.map((v) => v.view)).toEqual(['operate', 'sstv', 'phone', 'cw', 'rtty', 'psk', 'js8'])
    expect([...(PHONE_PANELS.defaultRemoved ?? [])]).toEqual([...HIDDEN])
    expect([...(CW_PANELS.defaultRemoved ?? [])]).toEqual([...HIDDEN])
    for (const v of [SSTV_PANELS, RTTY_PANELS, panelState.PSK_PANELS]) {
      expect([...(v.defaultRemoved ?? [])], v.view).toEqual(['rfScope'])
    }
    // JS8 is a grid cockpit, and FT arranges its columns (2026-10-07): the six boxes of each ship hidden
    // beside its RF scope pane.
    expect([...(panelState.JS8_PANELS.defaultRemoved ?? [])]).toEqual(['rfScope', ...BOX_IDS])
    expect([...(OPERATE_PANELS.defaultRemoved ?? [])]).toEqual(['rfScope', ...BOX_IDS])
    for (const v of ALL_PANEL_VOCABULARIES) {
      for (const id of v.defaultRemoved ?? []) {
        expect(v.panelIds, `"${v.view}" hides "${id}", which is not in its vocabulary`).toContain(id)
      }
    }
  })

  it('panelStateIn is the one reading of an absent entry, per vocabulary', () => {
    const empty = { v: 1 as const, state: {}, share: {} }
    expect(panelStateIn(PHONE_PANELS, empty, 'spots')).toBe('removed')
    expect(panelStateIn(PHONE_PANELS, empty, 'bandActivity')).toBe('docked')
    expect(panelStateIn(OPERATE_PANELS, empty, 'waterfall')).toBe('docked')
    // A stored value always wins over the default, in both directions.
    expect(panelStateIn(PHONE_PANELS, { ...empty, state: { spots: 'docked' } }, 'spots')).toBe('docked')
    expect(panelStateIn(PHONE_PANELS, { ...empty, state: { receiver: 'removed' } }, 'receiver')).toBe('removed')
  })
})

describe('the RF scope pane ships hidden in the five digital cockpits (operator, 2026-10-03)', () => {
  // The pick, verbatim: "Yes, opt-in pane (Recommended)" — an RF pan pane, off by default, in
  // FT/JS8/RTTY/PSK/SSTV, with the audio waterfall staying the default. Nobody's screen changes on
  // the update that adds it: a fresh record and one stored by an older build both read it hidden,
  // every other pane keeps its default, a tick docks it, and Reset hides it again.
  const DIGITAL: readonly PanelVocabulary<string>[] = [OPERATE_PANELS, panelState.JS8_PANELS, RTTY_PANELS, panelState.PSK_PANELS, SSTV_PANELS]

  it('a FRESH record shows it hidden, and every other pane docked, in all five', () => {
    for (const v of DIGITAL) {
      const { result, unmount } = renderHook(() => usePanelLayout(v))
      expect(result.current.stateOf('rfScope'), `${v.view}: it ships visible`).toBe('removed')
      for (const id of v.panelIds) {
        if (id === 'rfScope') continue
        // JS8's boxes ship hidden too (panelState.boxes.test.ts reads them).
        if ((BOX_IDS as readonly string[]).includes(id)) {
          expect(result.current.stateOf(id), `${v.view}: "${id}" ships visible`).toBe('removed')
          continue
        }
        expect(result.current.stateOf(id), `${v.view}: "${id}" changed its default`).toBe('docked')
      }
      unmount()
    }
  })

  it('a record stored before it existed reads it hidden, and keeps every choice in it', () => {
    savePanelLayout(panelStorageKey('rtty'), { v: 1, state: { stream: 'removed' }, share: { stream: 1.2 } } as PanelLayout<string>)
    const { result } = renderHook(() => usePanelLayout(RTTY_PANELS))
    expect(result.current.stateOf('rfScope')).toBe('removed')
    expect(result.current.stateOf('stream')).toBe('removed')
    expect(result.current.stateOf('scope')).toBe('docked')
    expect(result.current.shareOf('stream')).toBe(1.2)
  })

  it('a tick docks it and is stored; Reset hides it again', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.setPanelState('rfScope', 'docked'))
    expect(result.current.stateOf('rfScope')).toBe('docked')
    expect(JSON.parse(localStorage.getItem(panelStorageKey('operate'))!).state.rfScope).toBe('docked')
    act(() => result.current.reset())
    expect(result.current.stateOf('rfScope')).toBe('removed')
    expect(result.current.stateOf('waterfall'), 'control: Reset leaves the waterfall docked').toBe('docked')
  })

  it('it is no stop control by name, and JS8 can place it (it heads the leading column)', () => {
    expect((STOP_CONTROL_WORDS as readonly string[]).includes('rfscope')).toBe(false)
    expect(panelState.JS8_PANELS.arrange?.columns.a[0]).toBe('rfScope')
  })
})

describe("CW ships Phone's two feeds hidden too (plan H8)", () => {
  // The operator's pick: "CW gets Phone's Spots/Needed panes", hidden by default exactly as in Phone.
  // Each case drives one reader of an absent state, as Phone's do, against CW's own record.
  const CW_KEY = panelStorageKey('cw')

  it('a FRESH CW record shows Spots, Needed and the boxes hidden, and every other CW pane docked', () => {
    const { result } = renderHook(() => usePanelLayout(CW_PANELS))
    for (const id of ['spots', 'needed', ...BOX_IDS] as const) expect(result.current.stateOf(id), `${id} ships visible`).toBe('removed')
    for (const id of CW_PANELS.panelIds) {
      if (id === 'spots' || id === 'needed' || (BOX_IDS as readonly string[]).includes(id)) continue
      expect(result.current.stateOf(id), `"${id}" changed its default`).toBe('docked')
    }
    expect(localStorage.getItem(CW_KEY)).toBeNull()
  })

  it('an existing CW layout loads unchanged: its choices, shares and places, and neither feed', () => {
    savePanelLayout(CW_KEY, {
      v: 2,
      state: { copilot: 'removed', sent: 'docked' },
      share: { decode: 1.6 },
      place: { bandActivity: { col: 'a', order: 2 }, decode: { col: 'a', order: 0 }, sent: { col: 'a', order: 1 } },
    } as PanelLayout<string>)
    const { result } = renderHook(() => usePanelLayout(CW_PANELS))
    expect(result.current.stateOf('spots'), 'Spots appeared on an upgrade').toBe('removed')
    expect(result.current.stateOf('needed'), 'Needed appeared on an upgrade').toBe('removed')
    expect(result.current.stateOf('copilot')).toBe('removed')
    expect(result.current.stateOf('sent')).toBe('docked')
    expect(result.current.stateOf('decode')).toBe('docked')
    expect(result.current.shareOf('decode')).toBe(1.6)
    // The operator's placement is kept as stored; the two new ids are simply not in it, so each
    // stands at the foot of its stock column (panelPlace) — shown only once it is ticked.
    expect(result.current.layout.place).toEqual({
      decode: { col: 'a', order: 0 },
      sent: { col: 'a', order: 1 },
      bandActivity: { col: 'a', order: 2 },
    })
  })

  it('a tick docks one and is stored; ⊞ Reset hides both again and Undo brings them back', () => {
    const { result } = renderHook(() => usePanelLayout(CW_PANELS))
    act(() => result.current.setPanelState('spots', 'docked'))
    act(() => result.current.setPanelState('needed', 'docked'))
    expect(JSON.parse(localStorage.getItem(CW_KEY)!).state).toMatchObject({ spots: 'docked', needed: 'docked' })
    act(() => result.current.reset())
    expect(result.current.stateOf('spots')).toBe('removed')
    expect(result.current.stateOf('needed')).toBe('removed')
    act(() => result.current.undo())
    expect(result.current.stateOf('spots')).toBe('docked')
    expect(result.current.stateOf('needed')).toBe('docked')
  })

  it("the feeds take Phone's places: Spots under Decode's column, Needed under the middle one, both last below three tracks", () => {
    expect(CW_PANELS.arrange!.columns.a).toEqual(['decode', 'sent', 'spots', ...BOX_IDS])
    expect(CW_PANELS.arrange!.columns.b).toEqual(['bandActivity', 'copilot', 'needed'])
    expect(CW_PANELS.arrange!.pinned).toEqual([])
    expect(CW_PANELS.arrange!.stockMerged).toEqual(['decode', 'sent', 'bandActivity', 'copilot', 'spots', 'needed'])
  })
})

describe('seamShares', () => {
  it('centres to the stock [1, 1]', () => {
    expect(seamShares(0.5)).toEqual([1, 1])
  })

  it('is monotonic — dragging down grows the pane above', () => {
    const [aboveLo] = seamShares(0.3)
    const [aboveHi] = seamShares(0.7)
    expect(aboveHi).toBeGreaterThan(aboveLo)
  })

  it('always sums to 2 (relative flex, so the region stays full)', () => {
    for (const f of [0, 0.1, 0.42, 0.5, 0.88, 1]) {
      const [a, b] = seamShares(f)
      expect(a + b).toBeCloseTo(2, 10)
    }
  })

  it('clamps both extremes so neither pane drops below MIN_SHARE', () => {
    const [aTop, bTop] = seamShares(0) // dragged fully up
    expect(aTop).toBeGreaterThanOrEqual(MIN_SHARE)
    expect(bTop).toBeGreaterThanOrEqual(MIN_SHARE)
    const [aBot, bBot] = seamShares(1) // dragged fully down
    expect(aBot).toBeGreaterThanOrEqual(MIN_SHARE)
    expect(bBot).toBeGreaterThanOrEqual(MIN_SHARE)
  })
})
