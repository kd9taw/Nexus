// @vitest-environment jsdom
//
// ⊞ PANELS — AN ENTRY THAT CAN CHANGE NOTHING IS NOT CHECKABLE (operator, 2026-08-03:
// "what do the Panels selection / deselection of Rig Scope Controls and TX meters do on
// the Phone tab? I don't see them anywhere on my screen whether enabled or disabled").
//
// Both entries were live checkboxes that could not move anything on his station:
//   - Rig Scope Controls / CW's Scope Controls mount ONLY while the radio's own
//     panadapter streams (native Icom CI-V or FlexRadio). On the audio bandscope the
//     pane cannot exist, so the tick was inert.
//   - TX Meters gate correctly, but the unpinned variant renders nothing on receive —
//     the only moment the box has a visible effect is mid-over, when nobody is in a menu.
//
// So this suite RENDERS the real cockpits (the wiring, not a props fixture) in BOTH
// states and asserts the affordance itself: disabled + the reason when no native scope
// is streaming, checkable with no reason when one is, and the standing "when" note on
// TX Meters. A presence test on a source string would pass against a menu that never
// received the reason at all.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, act } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import { CwCockpit } from './CwCockpit'
import { TX_METERS_WHEN } from './TxMeters'
import { NO_NATIVE_SCOPE_REASON } from '../waterfall'
import type { AppSnapshot } from '../types'
import type { CwPanelId, PanelLayoutApi, PhonePanelId } from '../features/panelState'

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
    sent: [] as string[],
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
  cleanup()
})

function phoneSnap(): AppSnapshot {
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
    },
  } as unknown as AppSnapshot
}

function cwSnap(): AppSnapshot {
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
  const view = render(node)
  // Let the mount-time promises (getSettings / cwDecode) and the scope's feed report land.
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  fireEvent.click(screen.getByRole('button', { name: /panels/i }))
  return view
}

const openPhone = () =>
  openMenu(
    <PhoneCockpit
      snap={phoneSnap()}
      theme="dark"
      onWorkSpot={() => {}}
      spots={[]}
      panels={fakePanels<PhonePanelId>()}
    />,
  )

const openCw = () =>
  openMenu(
    <CwCockpit
      snap={cwSnap()}
      theme="dark"
      onWorkSpot={() => {}}
      spots={[]}
      panels={fakePanels<CwPanelId>()}
    />,
  )

/** The checkbox for one menu entry (its accessible name is the entry label alone — the
 *  reason lives outside the <label> so it never joins the name a screen reader speaks). */
const entry = (label: string) => screen.getByLabelText(label) as HTMLInputElement

describe('⊞ Panels — Rig Scope Controls follows the scope that is actually streaming', () => {
  it('audio bandscope: listed, NOT checkable, and the entry says what would bring it back', async () => {
    await openPhone()
    const box = entry('Rig Scope Controls')
    expect(box.disabled, 'the entry is checkable while its pane cannot mount').toBe(true)
    // The reason is ON the entry, and wired as the checkbox's description so it is
    // spoken with it rather than sitting as loose text nearby.
    const why = screen.getByText(NO_NATIVE_SCOPE_REASON)
    expect(box.getAttribute('aria-describedby')).toBe(why.id)
    expect(why.id).not.toBe('')
    // Still ticked: it is switched on, there is just nothing streaming for it to show.
    expect(box.checked).toBe(true)
    // The pane genuinely cannot render — that is what makes the tick inert.
    expect(document.querySelector('[data-pane="rigscope"]')).toBeNull()
  })

  it('native panadapter streaming: checkable again, with no reason on it', async () => {
    feedSource = 'civ'
    await openPhone()
    const box = entry('Rig Scope Controls')
    expect(box.disabled, 'still disabled with the rig streaming its own scope').toBe(false)
    expect(screen.queryByText(NO_NATIVE_SCOPE_REASON)).toBeNull()
    expect(box.getAttribute('aria-describedby')).toBeNull()
    // …and now the pane it names really is on screen, so the tick moves something.
    expect(document.querySelector('[data-pane="rigscope"]')).not.toBeNull()
  })

  it('CW carries the same rule for its Scope Controls entry', async () => {
    await openCw()
    expect(entry('Scope Controls').disabled).toBe(true)
    expect(screen.getByText(NO_NATIVE_SCOPE_REASON)).toBeTruthy()
    cleanup()

    feedSource = 'flex'
    await openCw()
    expect(entry('Scope Controls').disabled).toBe(false)
    expect(screen.queryByText(NO_NATIVE_SCOPE_REASON)).toBeNull()
  })
})

describe('⊞ Panels — TX Meters say when they have anything to show', () => {
  it('Phone: checkable (the gate works), with the standing "on transmit" note', async () => {
    await openPhone()
    const box = entry('TX Meters')
    expect(box.disabled, 'TX Meters is a working gate — it must stay checkable').toBe(false)
    const note = screen.getByText(TX_METERS_WHEN)
    expect(box.getAttribute('aria-describedby')).toBe(note.id)
    // On receive the panel itself renders nothing, which is exactly why the entry says so.
    expect(document.querySelector('.ph-txmeters')).toBeNull()
  })

  it('CW: the same id, the same note', async () => {
    await openCw()
    const box = entry('TX Meters')
    expect(box.disabled).toBe(false)
    expect(box.getAttribute('aria-describedby')).toBe(screen.getByText(TX_METERS_WHEN).id)
  })
})
