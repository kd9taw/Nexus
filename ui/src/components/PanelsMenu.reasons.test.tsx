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
import { render, screen, cleanup, fireEvent, act, within } from '@testing-library/react'
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
vi.mock('../api', async (importOriginal) => {
  // ⭐ DERIVED FROM THE REAL MODULE, not a hand-kept list. A hand-kept mock omits any export
  // added after it was written, and a component that calls one THROWS ON MOUNT — so the suite
  // goes red at a seam nothing in the diff explains, and the tempting fix is to make the test
  // pass rather than ask why. That cost five files one evening when a single API call was added
  // to the CW cockpit, this one among them.
  //
  // Every function the module exports is auto-stubbed here; the entries below override only the
  // ones this file's assertions actually depend on, so their shapes are unchanged.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    // Hand-kept mock: an export CwCockpit calls but this list omits makes it THROW ON MOUNT,
    // which reads as a behaviour regression rather than the stale mock it actually is.
    getCatCwUnprovenRigModels: vi.fn(async () => []),
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
  }
})

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
 *  reason lives outside the <label> so it never joins the name a screen reader speaks).
 *
 *  ⚠️ SCOPED TO THE POPOVER. A cockpit's pane frames carry their own titles as accessible
 *  names, so a bare `screen.getByLabelText` matches the entry AND the pane it names whenever
 *  the two read the same — which Phone's `Receiver`/`Transmitter` do, deliberately: an entry
 *  that called the pane something else would be a second name for one thing. The older
 *  entries escaped it only by the Title Case / sentence case accident (`DSP Functions` vs
 *  `DSP functions`), which is not a rule anybody agreed to and not one to lean on. */
const entry = (label: string) =>
  within(document.querySelector('.panels-menu-pop') as HTMLElement).getByLabelText(
    label,
  ) as HTMLInputElement

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

/** CW's THREE RIG-CONTROL ENTRIES NO LONGER OWN A FRAME EACH. Since the 2026-08-04 density
 *  pass the scope controls, the DSP toggles and the RX DSP levels are three groups inside ONE
 *  "Rig controls" frame — the ids, the menu entries and the reason notes are unchanged, only
 *  the boxes around them are. So "the pane this entry names is on screen" is located by the
 *  group's own accessible name, which is the thing the operator actually sees appear. Phone
 *  keeps a frame per entry and keeps using `pane()`. */
const CW_RIG_GROUP: Record<string, string> = {
  scopeCtl: '[aria-label="Rig scope control"], [aria-label="Flex panadapter control"]',
  dsp: '[aria-label="Rig DSP functions"]',
  rxdsp: '[aria-label="RX DSP levels"]',
}
const cwGroup = (id: keyof typeof CW_RIG_GROUP | string) =>
  document.querySelector(`[data-pane="rigctl"] ${CW_RIG_GROUP[id].split(', ').join(', [data-pane="rigctl"] ')}`)

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
    expect(cwGroup('scopeCtl')).toBeNull()
  })

  it.each(['civ', 'flex'])('CW, %s panadapter streaming: the reason is gone', async (src) => {
    feedSource = src
    await openCw()
    expectUnexplained('Scope Controls')
    expect(screen.queryByText(NO_NATIVE_SCOPE_REASON)).toBeNull()
    expect(cwGroup('scopeCtl')).not.toBeNull()
  })
})

describe('⊞ Panels — the DSP entries follow what the rig reports over CAT', () => {
  // ⭐ PHONE'S TWO ENTRIES LEFT THIS RULE ON 2026-09-20, and the three cases below say so
  // rather than being deleted. They used to pin the apology — "your radio is not reporting
  // DSP functions over CAT" under a `DSP Functions` tick that hid an empty box — and that
  // was right while a pane could be empty. Phone's `receiver` and `transmitter` panes never
  // are: every control renders whatever the rig says, disabled and carrying its own ⊘ when
  // the rig cannot drive it. An availability note on either entry would now be FALSE, and a
  // reason the operator can see is wrong is worse than none.
  //
  // So the apology did not disappear, it got finer: one whole-pane note became one mark per
  // control, which says the same thing about strictly more of the radio. CW is unchanged and
  // its cases below are untouched — it still has the panes this rule was written for.
  it('Phone, a rig that reports nothing: the two chain entries carry NO reason', async () => {
    await openPhone(BARE_RIG)
    expectUnexplained('Receiver')
    expectUnexplained('Transmitter')
    // …because both panes really are on screen with something in them. That is the fact the
    // absent note is claiming, so it is the fact that gets checked.
    expect(pane('receiver')).not.toBeNull()
    expect(pane('transmitter')).not.toBeNull()
    // And the apology is where it moved to: ONE line at the pane's foot naming the controls
    // this radio does not have — not a note on the menu entry, and not a grey row each.
    const foot = document.querySelector('[data-pane="receiver"] .ph-chain-absent')?.textContent ?? ''
    expect(foot, 'the note left the menu and arrived nowhere').toContain('NB')
    expect(foot).toContain('AGC')
    expect(
      document.querySelector('[data-chain="NB"]'),
      'a control this radio lacks is still drawing a grey row',
    ).toBeNull()
  })

  it('Phone, a rig that reports them: the entries read exactly the same', async () => {
    // THE PAIR, and the point of it: this render and the one above must be INDISTINGUISHABLE
    // at the menu. An entry that quietly went back to apologising on a bare rig would pass
    // the case above on its own only if that case were the only one.
    await openPhone()
    expectUnexplained('Receiver')
    expectUnexplained('Transmitter')
    expect(pane('receiver')).not.toBeNull()
    expect(pane('transmitter')).not.toBeNull()
    // The difference between the two rigs shows HERE instead. This fixture reports NB/NR, an
    // NR level and AGC, so those four DRAW where the bare rig's were collapsed — asserted
    // per control rather than as a count, because the foot line still names the controls
    // this rig does not report either (RF gain, AF, squelch…), so two counts would both be
    // non-zero and prove nothing.
    const foot = document.querySelector('[data-pane="receiver"] .ph-chain-absent')?.textContent ?? ''
    for (const chain of ['NB', 'NR', 'NRLVL', 'AGC']) {
      expect(document.querySelector(`[data-chain="${chain}"]`), `${chain}: collapsed over a rig that reports it`).not.toBeNull()
    }
    expect(foot, 'a control this rig reports was listed as missing').not.toContain('AGC')
  })

  it('CW, a rig that reports no DSP functions: both DSP entries carry the reason', async () => {
    await openCw(BARE_RIG)
    expectExplained('DSP Toggles', NO_DSP_FUNCS_REASON)
    expectExplained('RX DSP Levels', NO_DSP_LEVELS_REASON)
    expect(cwGroup('dsp')).toBeNull()
    expect(cwGroup('rxdsp')).toBeNull()
  })

  it('CW, a rig that reports them: no reason on either, and both panes mount', async () => {
    await openCw()
    expectUnexplained('DSP Toggles')
    expectUnexplained('RX DSP Levels')
    expect(cwGroup('dsp')).not.toBeNull()
    expect(cwGroup('rxdsp')).not.toBeNull()
  })

  it('CW, a rig with no NR/AGC but a commandable Sub: RX DSP Levels carries NO reason — the SUB row is behind it', async () => {
    // The Sub's levels ride the RX DSP box (a dual-receiver IC-7610 on Nexus's own CI-V
    // control), so on this rig that box is NOT empty, and an entry saying "these appear on a rig
    // that does" would be false about what is on the screen.
    await openCw({
      ...BARE_RIG,
      receivers: {
        main: { id: 'main', stages: { frontEnd: 'own', dsp: 'own', audio: 'own' } },
        sub: { id: 'sub', stages: { frontEnd: 'own', dsp: 'unknown', audio: 'own' } },
        subCapability: 'present',
        subCommandable: true,
      },
    })
    expectUnexplained('RX DSP Levels')
    expect(document.querySelector('[data-receiver="sub"]'), 'the SUB row it vouches for').not.toBeNull()
    // …and the DSP Toggles entry, with nothing behind it, still says so.
    expectExplained('DSP Toggles', NO_DSP_FUNCS_REASON)
  })

  it('Phone: the CONTROLS are marked independently, one each way in one render', async () => {
    // The case this replaces proved the two Phone ENTRIES were gated independently — that a
    // future "any DSP at all" shortcut could not apologise for a pane the operator can see.
    // Nothing gates those entries any more, so the same hazard moved down a level: a
    // shortcut that marked a whole PANE unavailable because one of its controls is. The rig
    // here is the common Hamlib case — NB/NR readable, NR level and AGC not — and one
    // control each way in ONE render is what catches it.
    await openPhone({ nrLevel: null, agc: null })
    expectUnexplained('Receiver')
    const live = document.querySelector('[data-chain="NB"] .ph-dsp-btn') as HTMLButtonElement
    expect(live, 'no NB toggle at all').toBeTruthy()
    expect(live.disabled, 'NB is dead on a rig that reports it').toBe(false)
    // …and the two it cannot read are collapsed into the foot line, named, in the same
    // render. One each way is what catches a shortcut that judges the whole pane at once.
    const foot = document.querySelector('[data-pane="receiver"] .ph-chain-absent')?.textContent ?? ''
    expect(document.querySelector('[data-chain="NRLVL"]'), 'the unreadable NR level still draws a row').toBeNull()
    expect(document.querySelector('[data-chain="AGC"]'), 'the unreadable AGC still draws a row').toBeNull()
    expect(foot, 'the unreadable NR level is named nowhere').toContain('NR')
    expect(foot, 'the unreadable AGC is named nowhere').toContain('AGC')
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
    // On receive the panel is now PRESENT and says the same thing the entry does — it is
    // `pinned`, so before the first over it holds a one-line hint that interpolates this very
    // string (`meters.tx.idle` takes TX_METERS_WHEN). The entry and the panel therefore cannot
    // drift. Until 2026-09-20 the panel rendered nothing here, which is what the entry's note
    // was compensating for; the note stays, because it is still what the panel is waiting on.
    const panel = document.querySelector('.ph-txmeters')
    expect(panel, 'the pinned panel should hold its idle hint on receive').not.toBeNull()
    expect(panel!.textContent).toContain(TX_METERS_WHEN)
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
