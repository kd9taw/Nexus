// @vitest-environment jsdom
//
// ⊞ PANELS — AN ENTRY THAT SHOWS NOTHING SAYS WHY, AND STAYS THE OPERATOR'S (operator,
// 2026-08-03: "what do the Panels selection / deselection of Rig Scope Controls and TX
// meters do on the Phone tab? I don't see them anywhere on my screen whether enabled or
// disabled").
//
// Eight entries across Phone and CW offer a pane that can be empty or absent through no
// fault of the tick:
//   - Rig Scope Controls / CW's Scope Controls mount ONLY while the radio's own
//     panadapter streams (native Icom CI-V or FlexRadio).
//   - Phone's DSP Functions / RX DSP Levels and CW's DSP Toggles / RX DSP Levels are
//     capability-gated on what the rig reports over CAT.
//   - CW's Sent Echo holds this SESSION's transmissions, so it is empty at every start-up.
//   - TX Meters gate correctly, but the unpinned variant renders nothing on receive.
//
// TWO IDEAS, NOT ONE. The REASON explains why nothing is on screen right now. The
// CHECKBOX is the operator's PREFERENCE about that panel. Merging them — refusing the
// tick while the station cannot feed the pane — took his control away in the state he is
// most likely to want it: CW's Sent Echo is empty at EVERY session start, and unticking it
// there is exactly how an operator who does not want the echo gets rid of it for the
// session. It also dimmed the focus ring of the one entry that most needs to be reachable.
// So every one of these entries is a plain, operable checkbox that carries its reason.
//
// This suite RENDERS the real cockpits (the wiring, not a props fixture) in BOTH states and
// asserts both halves: the reason arrives as the checkbox's accessible description when the
// pane has nothing to show, no reason when it has, and the tick RECORDS — and keeps — the
// operator's preference across the moment the station starts feeding it. A presence test on
// a source string would pass against a menu that never received the reason at all.
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
import type { CwPanelId, PanelLayoutApi, PanelState, PhonePanelId } from '../features/panelState'

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

/** @param state what the operator has already chosen for a panel ('removed' = he unticked
 *  it). Everything else is docked, which is the stock layout. */
function fakePanels<P extends string>(state: Partial<Record<P, PanelState>> = {}): PanelLayoutApi<P> {
  return {
    layout: { v: 1, state, share: {} },
    stateOf: (id: P) => state[id] ?? 'docked',
    setPanelState: vi.fn(),
    shareOf: () => 1,
    setShare: vi.fn(),
    setShares: vi.fn(),
    undo: vi.fn(),
    canUndo: false,
    undoRemoves: [],
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

/** Open Phone with its ⊞ menu up; returns the panel API so a test can read what moved. */
async function openPhone(radio: Record<string, unknown> = {}, state: Partial<Record<PhonePanelId, PanelState>> = {}) {
  const panels = fakePanels<PhonePanelId>(state)
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

async function openCw(radio: Record<string, unknown> = {}, state: Partial<Record<CwPanelId, PanelState>> = {}) {
  const panels = fakePanels<CwPanelId>(state)
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
 * EVERY entry, explained or not, is a plain operable checkbox — no `disabled`, no
 * `aria-disabled`. Both take the operator's preference away, and either one drags the
 * greyed-out look along with it: dimming a focusable element composites its focus ring
 * too, so the entry a keyboard operator most needs to find is the one hardest to see.
 * Availability belongs in the reason line; the tick is his answer to a different question.
 */
function expectOperable(label: string): HTMLInputElement {
  const box = entry(label)
  expect(
    box.disabled,
    `${label}: the \`disabled\` ATTRIBUTE removes the entry from the tab order — a ` +
      'keyboard or screen-reader operator would never reach the reason.',
  ).toBe(false)
  expect(
    box.getAttribute('aria-disabled'),
    `${label}: aria-disabled. That refuses the operator a preference he is entitled to ` +
      'record now (an empty Sent Echo is exactly when he wants it gone), and the dimming ' +
      'that goes with it halves the focus ring against the popover backdrop.',
  ).toBeNull()
  return box
}

/**
 * An entry whose pane has nothing to show right now: listed, fully operable, carrying its
 * reason as the checkbox's accessible DESCRIPTION so it reaches the operator who cannot
 * see the line under it.
 */
function expectExplained(label: string, reason: string): HTMLInputElement {
  const box = expectOperable(label)
  // Resolve the description the way an assistive technology does: follow the id.
  const whyId = box.getAttribute('aria-describedby')
  expect(whyId, `${label}: no aria-describedby — the reason is loose text, not its description`)
    .toBeTruthy()
  expect(document.getElementById(whyId!)?.textContent, `${label}: wrong reason on the entry`).toBe(
    reason,
  )
  return box
}

/** An entry whose pane CAN show something: operable, with no reason hung on it. */
function expectUnexplained(label: string): HTMLInputElement {
  const box = expectOperable(label)
  expect(
    box.getAttribute('aria-describedby'),
    `${label}: carries a reason while its pane has something to show`,
  ).toBeNull()
  return box
}

const pane = (id: string) => document.querySelector(`[data-pane="${id}"]`)

describe('⊞ Panels — the rig-scope entry follows the scope that is actually streaming', () => {
  it('Phone, audio bandscope: listed, ticked, and it says what would bring it back', async () => {
    await openPhone()
    const box = expectExplained('Rig Scope Controls', NO_NATIVE_SCOPE_REASON)
    // Still ticked: it is switched on, there is just nothing streaming for it to show.
    expect(box.checked).toBe(true)
    // The pane genuinely cannot render — which is what the reason is there to explain.
    expect(pane('rigscope')).toBeNull()
  })

  // BOTH arms of `civScope || flexScope`, in BOTH cockpits. Covering one arm per cockpit
  // let a mutant that dropped the other arm live through the whole suite (Phone without
  // `flexScope`, CW without `civScope`) — a FlexRadio on Phone, or an Icom on CW, would
  // have kept the entry apologising with its own panadapter on screen.
  it.each(['civ', 'flex'])('Phone, %s panadapter streaming: the reason is gone', async (src) => {
    feedSource = src
    await openPhone()
    expectUnexplained('Rig Scope Controls')
    expect(screen.queryByText(NO_NATIVE_SCOPE_REASON)).toBeNull()
    // …and now the pane it names really is on screen, so the tick moves something.
    expect(pane('rigscope')).not.toBeNull()
  })

  it('CW, audio bandscope: the same rule for its Scope Controls entry', async () => {
    await openCw()
    expectExplained('Scope Controls', NO_NATIVE_SCOPE_REASON)
    expect(pane('scopeCtl')).toBeNull()
  })

  it.each(['civ', 'flex'])('CW, %s panadapter streaming: the reason is gone', async (src) => {
    feedSource = src
    await openCw()
    expectUnexplained('Scope Controls')
    expect(screen.queryByText(NO_NATIVE_SCOPE_REASON)).toBeNull()
    expect(pane('scopeCtl')).not.toBeNull()
  })
})

describe('⊞ Panels — the DSP entries follow what the rig reports over CAT', () => {
  it('Phone, a rig that reports no DSP functions: both DSP entries carry the reason', async () => {
    await openPhone(BARE_RIG)
    expectExplained('DSP Functions', NO_DSP_FUNCS_REASON)
    expectExplained('RX DSP Levels', NO_DSP_LEVELS_REASON)
    expect(pane('dsp')).toBeNull()
    expect(pane('dspLevels')).toBeNull()
  })

  it('Phone, a rig that reports them: no reason on either, and both panes mount', async () => {
    await openPhone()
    expectUnexplained('DSP Functions')
    expectUnexplained('RX DSP Levels')
    expect(pane('dsp')).not.toBeNull()
    expect(pane('dspLevels')).not.toBeNull()
  })

  it('CW, a rig that reports no DSP functions: both DSP entries carry the reason', async () => {
    await openCw(BARE_RIG)
    expectExplained('DSP Toggles', NO_DSP_FUNCS_REASON)
    expectExplained('RX DSP Levels', NO_DSP_LEVELS_REASON)
    expect(pane('dsp')).toBeNull()
    expect(pane('rxdsp')).toBeNull()
  })

  it('CW, a rig that reports them: no reason on either, and both panes mount', async () => {
    await openCw()
    expectUnexplained('DSP Toggles')
    expectUnexplained('RX DSP Levels')
    expect(pane('dsp')).not.toBeNull()
    expect(pane('rxdsp')).not.toBeNull()
  })

  it('Phone: the two DSP entries are gated INDEPENDENTLY', async () => {
    // A rig with NB/NR but no readable NR level or AGC (common over Hamlib): the toggles
    // pane mounts, the levels pane cannot. One entry each way in ONE render is what stops
    // a future "any DSP at all" shortcut from apologising for a pane the operator can see.
    await openPhone({ nrLevel: null, agc: null })
    expectUnexplained('DSP Functions')
    expectExplained('RX DSP Levels', NO_DSP_LEVELS_REASON)
    expect(pane('dsp')).not.toBeNull()
    expect(pane('dspLevels')).toBeNull()
  })
})

describe('⊞ Panels — CW Sent Echo is empty until the first over', () => {
  it('session start: nothing has been sent, and the entry says so', async () => {
    // The operator's exact complaint in another cockpit: tick or untick, nothing moves,
    // because `sent` is empty on every fresh session.
    await openCw()
    expectExplained('Sent Echo', NOTHING_SENT_REASON)
    expect(pane('sent')).toBeNull()
  })

  it('after the first transmission: no reason, and the pane is really there', async () => {
    cwSentLines = ['CQ CQ DE KD9TAW K']
    await openCw()
    expectUnexplained('Sent Echo')
    expect(screen.queryByText(NOTHING_SENT_REASON)).toBeNull()
    expect(pane('sent')).not.toBeNull()
  })
})

describe('⊞ Panels — TX Meters say when they have anything to show', () => {
  it('Phone: operable (the gate works), with the standing "on transmit" note', async () => {
    await openPhone()
    const box = expectOperable('TX Meters')
    const note = screen.getByText(TX_METERS_WHEN)
    expect(box.getAttribute('aria-describedby')).toBe(note.id)
    // On receive the panel itself renders nothing, which is exactly why the entry says so.
    expect(document.querySelector('.ph-txmeters')).toBeNull()
  })

  it('CW: the same id, the same note', async () => {
    await openCw()
    const box = expectOperable('TX Meters')
    expect(box.getAttribute('aria-describedby')).toBe(screen.getByText(TX_METERS_WHEN).id)
  })
})

describe('⊞ Panels — the reason explains the screen; the tick stays the operator\'s', () => {
  // A reason is an EXPLANATION, never a refusal. Accessibility here is always-on, so the
  // entry has to be a real tab stop with its reason attached — and that is the same entry
  // whose box must still answer to him, because "I do not want this panel" is a preference
  // he holds independently of whether his rig can feed it this minute.
  it('it is a focus stop, and focusing it carries the reason with it', async () => {
    await openPhone()
    const box = expectExplained('Rig Scope Controls', NO_NATIVE_SCOPE_REASON)
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

  it('unticking an entry that has nothing to show RECORDS the preference', async () => {
    // CW's Sent Echo at session start is the case that cost him control: it is empty at
    // every start-up, and unticking it right there is how an operator who does not want
    // the echo this session gets rid of it. Refusing the tick until his first over meant
    // waiting to transmit before he could hide a panel.
    const panels = await openCw()
    const box = expectExplained('Sent Echo', NOTHING_SENT_REASON)
    fireEvent.click(box)
    expect(
      panels.setPanelState,
      'the untick was swallowed — the operator cannot say "not this session" until the ' +
        'station happens to be able to feed a panel he does not want',
    ).toHaveBeenCalledWith('sent', 'removed')
  })

  it('…and the pane stays away once the station CAN feed it', async () => {
    // The other half, and the reason recording it is honest: what he chose while the echo
    // was empty still holds after his first over. Availability decides whether the pane
    // COULD mount; his tick decides whether it does.
    cwSentLines = ['CQ CQ DE KD9TAW K']
    await openCw({}, { sent: 'removed' })
    expect(expectUnexplained('Sent Echo').checked).toBe(false)
    expect(
      pane('sent'),
      'the pane came back by itself — the untick he made while it was empty was thrown ' +
        'away, and availability is overriding his preference',
    ).toBeNull()
    // Not a dead render: the panes he did not untick are on screen.
    expect(pane('decode')).not.toBeNull()
  })

  it('an entry with nothing to explain acts the same way', async () => {
    const panels = await openPhone()
    fireEvent.click(expectUnexplained('Band Activity'))
    expect(panels.setPanelState).toHaveBeenCalledWith('bandActivity', 'removed')
  })
})

describe('⊞ Panels — a line the operator has already answered is not left standing', () => {
  it('an UNTICKED entry drops its reason, which has stopped being why the screen is empty', async () => {
    // CW's Sent Echo, unticked, at session start: the station reason ("nothing has been
    // sent this session") is still TRUE of the station and no longer the explanation of
    // anything he is looking at — he hid the pane, that is why it is not there. Leaving the
    // line under an unchecked box reads as though the station were keeping his panel away.
    cwSentLines = []
    await openCw({}, { sent: 'removed' })
    const box = expectOperable('Sent Echo')
    expect(box.checked).toBe(false)
    expect(
      box.getAttribute('aria-describedby'),
      'an unticked entry still explains the screen with a station reason that is not why ' +
        'the pane is gone',
    ).toBeNull()
    expect(
      screen.queryByText(NOTHING_SENT_REASON),
      'the reason line is still rendered under an entry the operator already answered',
    ).toBeNull()
  })

  it('…and it is back in the same render as the re-tick', async () => {
    // The suppression must be a function of the current state, not a latch: re-ticking has
    // to bring the explanation back, or an operator who restores a panel that still cannot
    // show anything is back to the dead-checkbox complaint this whole affordance answers.
    cwSentLines = []
    await openCw()
    expectExplained('Sent Echo', NOTHING_SENT_REASON)
  })

  it('a consequence note goes too — it warns about an act already taken', async () => {
    // Phone's Voice Keyer note reads "hiding this stops a voice message that is playing…".
    // On an entry that IS hidden there is nothing left to stop; the sentence describes the
    // past. Ticking it back is not a hide, so nothing there needs a warning either.
    await openPhone({}, { voiceKeyer: 'removed' })
    const box = expectOperable('Voice Keyer')
    expect(box.checked).toBe(false)
    expect(
      box.getAttribute('aria-describedby'),
      'a hidden Voice Keyer still warns that hiding it will stop a message',
    ).toBeNull()
  })
})

describe('⊞ Panels — keyboard and screen-reader mechanics', () => {
  it('the "popped out" tag annotates the entry instead of renaming it', async () => {
    // The tag used to sit INSIDE the <label>, so it joined the checkbox's accessible NAME
    // with no separator: "Voice Keyerpopped out". Outside the label and hung off
    // aria-describedby, the name is the panel's name and the state still reaches a screen
    // reader — which is what the operator needs from it.
    await openPhone({}, { voiceKeyer: 'popped' })
    const box = screen.getByLabelText('Voice Keyer') as HTMLInputElement
    expect(
      box.getAttribute('aria-label') ?? box.labels?.[0]?.textContent,
      'the state tag is glued into the checkbox name',
    ).toBe('Voice Keyer')
    const ids = (box.getAttribute('aria-describedby') ?? '').split(/\s+/).filter(Boolean)
    const described = ids.map((i) => document.getElementById(i)?.textContent ?? '').join(' ')
    expect(described, 'a popped-out panel does not say so to a screen reader').toMatch(/popped out/)
  })

  it('Escape hands focus back to the ⊞ button, not to the document body', async () => {
    // Closing the popover destroys the focused element. The browser's fallback owner is
    // <body>, which restarts a keyboard operator's tab traversal at the top of the app —
    // dozens of stops from the cockpit he was working. Focus belongs on the control that
    // opened the thing he just closed.
    await openPhone()
    const box = entry('Band Activity')
    box.focus()
    fireEvent.keyDown(box, { key: 'Escape' })
    expect(screen.queryByLabelText('Band Activity'), 'Escape did not close the menu').toBeNull()
    expect(
      document.activeElement,
      'Escape dropped focus to the body instead of returning it to ⊞ Panels',
    ).toBe(screen.getByRole('button', { name: /panels/i }))
  })
})
