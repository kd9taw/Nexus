// @vitest-environment jsdom
//
// ⊞ PANELS — AN ENTRY THAT CAN CHANGE NOTHING IS NOT CHECKABLE (operator, 2026-08-03:
// "what do the Panels selection / deselection of Rig Scope Controls and TX meters do on
// the Phone tab? I don't see them anywhere on my screen whether enabled or disabled").
//
// Both entries he named were live checkboxes that could not move anything on his station:
//   - Rig Scope Controls / CW's Scope Controls mount ONLY while the radio's own
//     panadapter streams (native Icom CI-V or FlexRadio). On the audio bandscope the
//     pane cannot exist, so the tick was inert.
//   - TX Meters gate correctly, but the unpinned variant renders nothing on receive —
//     the only moment the box has a visible effect is mid-over, when nobody is in a menu.
//
// The same shape survived on five more entries in the same two cockpits, and they are
// covered here too: Phone's DSP Functions / RX DSP Levels and CW's DSP Toggles / RX DSP
// Levels are capability-gated on what the rig reports over CAT, and CW's Sent Echo holds
// this SESSION's transmissions — so at every session start it is empty and its tick moves
// nothing, which is the operator's complaint verbatim in another cockpit.
//
// So this suite RENDERS the real cockpits (the wiring, not a props fixture) in BOTH states
// and asserts the affordance itself: reachable-but-inert plus the reason when the pane
// cannot mount, checkable with no reason when it can, and the standing "when" note on TX
// Meters. A presence test on a source string would pass against a menu that never received
// the reason at all.
//
// The unavailable entry is `aria-disabled`, never `disabled`: a disabled control leaves the
// tab order, so the operator who most needs the reason — keyboard-only, screen reader — is
// exactly the one who would never reach it. The reachability pin is its own describe below.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, act } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import { CwCockpit } from './CwCockpit'
import { TX_METERS_WHEN } from './TxMeters'
import { NO_NATIVE_SCOPE_REASON } from '../waterfall'
import {
  NO_DSP_FUNCS_REASON,
  NO_DSP_LEVELS_REASON,
  NOTHING_SENT_REASON,
} from '../features/panelHost'
import type { AppSnapshot } from '../types'
import type { CwPanelId, PanelLayoutApi, PhonePanelId } from '../features/panelState'

/** What the stubbed engine reports as this session's sent CW — '' at session start is the
 *  state the Sent Echo entry is dead in, and the whole point of that entry's reason. */
let cwSentLines: string[] = []

// Every engine call either cockpit's subtree makes on mount, stubbed harmlessly (the
// union of the two structure suites' lists).
vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 } })),
  setSettings: vi.fn(async () => ({})),
  getMeters: vi.fn(async () => ({ rxLevel: 0, smeterDb: null })),
  setPtt: vi.fn(async () => {}),
  setRfPower: vi.fn(async () => {}),
  setMicGain: vi.fn(async () => {}),
  setNrLevel: vi.fn(async () => {}),
  setAgc: vi.fn(async () => ({})),
  setScopeSpan: vi.fn(async () => ({})),
  setScopeRef: vi.fn(async () => {}),
  setFlexPanSpan: vi.fn(async () => ({})),
  setFlexPanRef: vi.fn(async () => ({})),
  startQsoRecording: vi.fn(async () => ({})),
  stopQsoRecording: vi.fn(async () => ({})),
  setTune: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  setSplit: vi.fn(async () => ({})),
  setRigFunc: vi.fn(async () => ({})),
  setSidebandOverride: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
  sendCw: vi.fn(async () => {}),
  setCwKeyer: vi.fn(async () => null),
  setCwWpm: vi.fn(async () => {}),
  stopCw: vi.fn(async () => {}),
  cwDecode: vi.fn(async () => ({
    text: '',
    wpm: 22,
    sent: cwSentLines,
    keyerError: null,
    candidates: [],
    state: 'listening',
    headline: '',
    prompt: '',
    recommended: null,
    workedCall: null,
    rst: null,
    name: null,
  })),
  cwClear: vi.fn(async () => {}),
  setAiCw: vi.fn(async () => {}),
  selectPeer: vi.fn(async () => null),
  previewCw: vi.fn(async (t: string) => t),
  pointRotatorAtCall: vi.fn(async () => 0),
}))

// The header is stubbed down to the one thing under test — it hosts the ⊞ menu, which
// both cockpits hand it as `actions`.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ actions }: { actions?: unknown }) => (
    <header className="cockpit-header">{actions as never}</header>
  ),
}))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

/** Which scope feed the stubbed scope reports for the render under test — '' = the
 *  soundcard bandscope (no native panadapter), 'civ'/'flex' = the rig's own. This is the
 *  exact channel the real PhoneScope uses to tell a cockpit what is driving it. */
let feedSource = ''
vi.mock('./PhoneScope', async () => {
  const { useEffect, useRef } = await import('react')
  return {
    PhoneScope: ({ onFeed }: { onFeed?: (s: string, lo: number, hi: number) => void }) => {
      // Report ONCE, like the real scope, which only calls back when the feed changes.
      // The cockpit's onFeed is a fresh closure every render, so a callback-keyed effect
      // would report → re-render → report forever.
      const cb = useRef(onFeed)
      cb.current = onFeed
      useEffect(() => {
        if (feedSource) cb.current?.(feedSource, 14_070_000, 14_120_000)
      }, [])
      return <div data-testid="scope-stub" />
    },
  }
})

afterEach(() => {
  feedSource = ''
  cwSentLines = []
  cleanup()
})

/** A rig that reports NOTHING optional over CAT — no DSP functions, no NR level, no AGC.
 *  Every Hamlib backend without those capabilities looks like this, and it is the state in
 *  which the DSP entries can never mount whatever their box says. */
const BARE_RIG = {
  nb: null,
  nr: null,
  notch: null,
  comp: null,
  vox: null,
  nrLevel: null,
  agc: null,
}

function phoneSnap(radio: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.2,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      rigMode: 'USB',
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      nrLevel: 0.3,
      agc: 'fast',
      nb: true,
      nr: true,
      ...radio,
    },
  } as unknown as AppSnapshot
}

function cwSnap(radio: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.05,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      rigMode: 'CW',
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      cwWpm: 22,
      cwKeyer: 'cat',
      nrLevel: 0.3,
      agc: 'fast',
      nb: true,
      nr: true,
      filterWidthHz: 500,
      ...radio,
    },
  } as unknown as AppSnapshot
}

function fakePanels<P extends string>(): PanelLayoutApi<P> {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: () => 'docked',
    setPanelState: vi.fn(),
    shareOf: () => 1,
    setShare: vi.fn(),
    setShares: vi.fn(),
    undo: vi.fn(),
    canUndo: false,
    reset: vi.fn(),
  }
}

/** Render a cockpit and open its ⊞ Panels menu. */
async function openMenu(node: React.ReactElement) {
  render(node)
  // Let the mount-time promises (getSettings / cwDecode) and the scope's feed report land.
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  fireEvent.click(screen.getByRole('button', { name: /panels/i }))
}

/** Open Phone with its ⊞ menu up; returns the panel API so a test can prove nothing moved. */
async function openPhone(radio: Record<string, unknown> = {}) {
  const panels = fakePanels<PhonePanelId>()
  await openMenu(
    <PhoneCockpit
      snap={phoneSnap(radio)}
      theme="dark"
      onWorkSpot={() => {}}
      spots={[]}
      panels={panels}
    />,
  )
  return panels
}

async function openCw(radio: Record<string, unknown> = {}) {
  const panels = fakePanels<CwPanelId>()
  await openMenu(
    <CwCockpit
      snap={cwSnap(radio)}
      theme="dark"
      onWorkSpot={() => {}}
      spots={[]}
      panels={panels}
    />,
  )
  return panels
}

/** The checkbox for one menu entry (its accessible name is the entry label alone — the
 *  reason lives outside the <label> so it never joins the name a screen reader speaks). */
const entry = (label: string) => screen.getByLabelText(label) as HTMLInputElement

/**
 * An entry whose pane cannot mount: listed, carrying its reason as the checkbox's
 * accessible DESCRIPTION, marked unavailable — and still a real focus stop, because
 * `disabled` would delete it from the very navigation the reason is written for.
 * Returns the box so the caller can go on to prove it is inert.
 */
function expectUnavailable(label: string, reason: string): HTMLInputElement {
  const box = entry(label)
  expect(
    box.disabled,
    `${label}: the \`disabled\` ATTRIBUTE removes the entry from the tab order — a ` +
      'keyboard or screen-reader operator would never reach the reason. Use aria-disabled.',
  ).toBe(false)
  expect(box.getAttribute('aria-disabled'), `${label}: not marked unavailable`).toBe('true')
  // Resolve the description the way an assistive technology does: follow the id.
  const whyId = box.getAttribute('aria-describedby')
  expect(whyId, `${label}: no aria-describedby — the reason is loose text, not its description`)
    .toBeTruthy()
  expect(document.getElementById(whyId!)?.textContent, `${label}: wrong reason on the entry`).toBe(
    reason,
  )
  return box
}

/** An entry whose pane CAN mount: checkable, with no reason hung on it. */
function expectAvailable(label: string): HTMLInputElement {
  const box = entry(label)
  expect(box.disabled, `${label}: disabled while its pane can mount`).toBe(false)
  expect(box.getAttribute('aria-disabled'), `${label}: marked unavailable while it can mount`)
    .toBeNull()
  return box
}

const pane = (id: string) => document.querySelector(`[data-pane="${id}"]`)

describe('⊞ Panels — the rig-scope entry follows the scope that is actually streaming', () => {
  it('Phone, audio bandscope: listed, NOT checkable, and it says what would bring it back', async () => {
    await openPhone()
    const box = expectUnavailable('Rig Scope Controls', NO_NATIVE_SCOPE_REASON)
    // Still ticked: it is switched on, there is just nothing streaming for it to show.
    expect(box.checked).toBe(true)
    // The pane genuinely cannot render — that is what makes the tick inert.
    expect(pane('rigscope')).toBeNull()
  })

  // BOTH arms of `civScope || flexScope`, in BOTH cockpits. Covering one arm per cockpit
  // let a mutant that dropped the other arm live through the whole suite (Phone without
  // `flexScope`, CW without `civScope`) — a FlexRadio on Phone, or an Icom on CW, would
  // have kept the entry greyed with its own panadapter on screen.
  it.each(['civ', 'flex'])('Phone, %s panadapter streaming: checkable again, no reason', async (src) => {
    feedSource = src
    await openPhone()
    expectAvailable('Rig Scope Controls')
    expect(screen.queryByText(NO_NATIVE_SCOPE_REASON)).toBeNull()
    // …and now the pane it names really is on screen, so the tick moves something.
    expect(pane('rigscope')).not.toBeNull()
  })

  it('CW, audio bandscope: the same rule for its Scope Controls entry', async () => {
    await openCw()
    expectUnavailable('Scope Controls', NO_NATIVE_SCOPE_REASON)
    expect(pane('scopeCtl')).toBeNull()
  })

  it.each(['civ', 'flex'])('CW, %s panadapter streaming: checkable again, no reason', async (src) => {
    feedSource = src
    await openCw()
    expectAvailable('Scope Controls')
    expect(screen.queryByText(NO_NATIVE_SCOPE_REASON)).toBeNull()
    expect(pane('scopeCtl')).not.toBeNull()
  })
})

describe('⊞ Panels — the DSP entries follow what the rig reports over CAT', () => {
  it('Phone, a rig that reports no DSP functions: both DSP entries are unavailable', async () => {
    await openPhone(BARE_RIG)
    expectUnavailable('DSP Functions', NO_DSP_FUNCS_REASON)
    expectUnavailable('RX DSP Levels', NO_DSP_LEVELS_REASON)
    expect(pane('dsp')).toBeNull()
    expect(pane('dspLevels')).toBeNull()
  })

  it('Phone, a rig that reports them: both go checkable and both panes mount', async () => {
    await openPhone()
    expectAvailable('DSP Functions')
    expectAvailable('RX DSP Levels')
    expect(pane('dsp')).not.toBeNull()
    expect(pane('dspLevels')).not.toBeNull()
  })

  it('CW, a rig that reports no DSP functions: both DSP entries are unavailable', async () => {
    await openCw(BARE_RIG)
    expectUnavailable('DSP Toggles', NO_DSP_FUNCS_REASON)
    expectUnavailable('RX DSP Levels', NO_DSP_LEVELS_REASON)
    expect(pane('dsp')).toBeNull()
    expect(pane('rxdsp')).toBeNull()
  })

  it('CW, a rig that reports them: both go checkable and both panes mount', async () => {
    await openCw()
    expectAvailable('DSP Toggles')
    expectAvailable('RX DSP Levels')
    expect(pane('dsp')).not.toBeNull()
    expect(pane('rxdsp')).not.toBeNull()
  })

  it('Phone: the two DSP entries are gated INDEPENDENTLY', async () => {
    // A rig with NB/NR but no readable NR level or AGC (common over Hamlib): the toggles
    // pane mounts, the levels pane cannot. One entry each way in ONE render is what stops
    // a future "any DSP at all" shortcut from re-greying a pane the operator can see.
    await openPhone({ nrLevel: null, agc: null })
    expectAvailable('DSP Functions')
    expectUnavailable('RX DSP Levels', NO_DSP_LEVELS_REASON)
    expect(pane('dsp')).not.toBeNull()
    expect(pane('dspLevels')).toBeNull()
  })
})

describe('⊞ Panels — CW Sent Echo is dead until the first over', () => {
  it('session start: nothing has been sent, so the entry is unavailable and says so', async () => {
    // The operator's exact complaint in another cockpit: tick or untick, nothing moves,
    // because `sent` is empty on every fresh session.
    await openCw()
    expectUnavailable('Sent Echo', NOTHING_SENT_REASON)
    expect(pane('sent')).toBeNull()
  })

  it('after the first transmission: checkable, and the pane is really there', async () => {
    cwSentLines = ['CQ CQ DE KD9TAW K']
    await openCw()
    expectAvailable('Sent Echo')
    expect(screen.queryByText(NOTHING_SENT_REASON)).toBeNull()
    expect(pane('sent')).not.toBeNull()
  })
})

describe('⊞ Panels — TX Meters say when they have anything to show', () => {
  it('Phone: checkable (the gate works), with the standing "on transmit" note', async () => {
    await openPhone()
    const box = expectAvailable('TX Meters')
    const note = screen.getByText(TX_METERS_WHEN)
    expect(box.getAttribute('aria-describedby')).toBe(note.id)
    // On receive the panel itself renders nothing, which is exactly why the entry says so.
    expect(document.querySelector('.ph-txmeters')).toBeNull()
  })

  it('CW: the same id, the same note', async () => {
    await openCw()
    const box = expectAvailable('TX Meters')
    expect(box.getAttribute('aria-describedby')).toBe(screen.getByText(TX_METERS_WHEN).id)
  })
})

describe('⊞ Panels — an unavailable entry is REACHABLE and inert, never removed', () => {
  // The brief's own argument for keeping a dead entry listed was that a greyed entry with a
  // reason tells the operator more than a missing one. `disabled` would have re-created
  // "missing" on the accessibility surface: a disabled control is not a tab stop, so the
  // keyboard/screen-reader operator would skip straight past the only thing that explains
  // the pane they cannot find. Accessibility here is always-on, not a mode.
  it('it is a focus stop, and focusing it carries the reason with it', async () => {
    await openPhone()
    const box = expectUnavailable('Rig Scope Controls', NO_NATIVE_SCOPE_REASON)
    expect(box.tabIndex, 'pulled out of the tab order by a negative tabindex').toBeGreaterThanOrEqual(0)
    box.focus()
    expect(
      document.activeElement,
      'the entry cannot take focus — with `disabled` it never can, and the reason is ' +
        'unreachable for exactly the operator it was written for',
    ).toBe(box)
    // The description travels with focus: it is on the focused element, not beside it.
    const whyId = document.activeElement!.getAttribute('aria-describedby')
    expect(document.getElementById(whyId!)?.textContent).toBe(NO_NATIVE_SCOPE_REASON)
  })

  it('reachable does not mean actionable: clicking it changes nothing', async () => {
    const panels = await openPhone()
    const box = expectUnavailable('Rig Scope Controls', NO_NATIVE_SCOPE_REASON)
    fireEvent.click(box)
    expect(
      panels.setPanelState,
      'the entry acted — a reachable entry must still be unable to remove a pane that ' +
        'cannot mount, or the menu is lying in the other direction',
    ).not.toHaveBeenCalled()
    expect(box.checked, 'the box flipped, so it looks like it did something').toBe(true)
  })

  it('an entry that CAN act still acts (the affordance did not deaden the menu)', async () => {
    const panels = await openPhone()
    fireEvent.click(expectAvailable('Band Activity'))
    expect(panels.setPanelState).toHaveBeenCalledWith('bandActivity', 'removed')
  })
})
