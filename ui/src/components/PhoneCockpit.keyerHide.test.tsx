// @vitest-environment jsdom
//
// HIDING THE VOICE KEYER IS A STOP, AND THE OPERATOR IS TOLD BOTH HALVES OF IT FIRST.
//
// THE STOP LINE (features/panelState.ts) admits a transmitting pane to a ⊞ vocabulary on
// two conditions, and this file computes both at the real site:
//   (b1) the pane's HIDE PATH IS ITSELF A STOP — unmounting it ends what it started;
//   (b2) its ⊞ entry SAYS SO before the tick.
//
// (b1) is why the suite renders the REAL VoiceKeyer (every other Phone suite stubs it)
// inside the REAL cockpit and hides it through the panel record the ⊞ menu writes. A test
// against a stub would prove the pane disappears and nothing about the transmitter.
//
// (b2) used to be an assertion about ONE id, which is recitation dressed as enforcement: a
// second pane admitted the same way would have shipped noteless. It is computed here
// instead, and the two clauses are wired to each other — the suite drives EVERY id in the
// Phone vocabulary through the hide path, watches which ones fire a stop at the wire, and
// requires exactly those to carry a note. No declaration list to keep in step: an id that
// stops something on hide and says nothing fails, and so does a scare note on an id that
// stops nothing.
//
// The keyer's hide fires TWO stops, and they are different acts. stopVoice ends a message —
// that is the stop the pane was admitted for. cancelVoiceRecording DISCARDS a capture in
// progress, which is destruction, not safety, and is not implied by "hide a pane". It is
// named in the note before the tick and announced by a toast when it happens; both are
// pinned below.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, act, within } from '@testing-library/react'
import { PhoneCockpit, VOICE_KEYER_STOPS_ON_HIDE } from './PhoneCockpit'
import type { AppSnapshot } from '../types'
import { PHONE_PANEL_IDS } from '../features/panelState'
import type { PanelLayoutApi, PhonePanelId } from '../features/panelState'

// vi.hoisted, not a bare const: the vi.mock factory below is hoisted above every
// top-level binding, so a plain const would be in its temporal dead zone when it runs.
// These are the WIRE calls that end or destroy something — the sweep below treats a hide
// that touches any of them as a hide with a consequence.
const { stopVoice, cancelVoiceRecording, startVoiceRecording, pushToast } = vi.hoisted(() => ({
  stopVoice: vi.fn(async () => ({})),
  cancelVoiceRecording: vi.fn(async () => ({})),
  startVoiceRecording: vi.fn(async () => ({})),
  pushToast: vi.fn(),
}))

vi.mock('../api', () => ({
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
  // The voice keyer's own wire. `stopVoice` and `cancelVoiceRecording` are the two under
  // test — the sweep below asks the wire, not the component, what a hide did.
  getVoiceMessages: vi.fn(async () => [{ slot: 1, label: 'CQ', file: '/tmp/cq.wav' }]),
  playVoiceMessage: vi.fn(async () => ({})),
  stopVoice,
  startVoiceRecording,
  stopVoiceRecording: vi.fn(async () => []),
  cancelVoiceRecording,
  clearVoiceMessage: vi.fn(async () => []),
  importVoiceMessage: vi.fn(async () => []),
}))
vi.mock('../toast', () => ({
  pushToast,
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

// The header is stubbed down to the one thing under test — it hosts the ⊞ menu, which the
// cockpit hands it as `actions`.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ actions }: { actions?: unknown }) => (
    <header className="cockpit-header">{actions as never}</header>
  ),
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))
// VoiceKeyer is deliberately NOT mocked — its unmount cleanup is the behaviour under test.

afterEach(() => {
  stopVoice.mockClear()
  cancelVoiceRecording.mockClear()
  startVoiceRecording.mockClear()
  pushToast.mockClear()
  cleanup()
})

function makeSnap(): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.2,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      sidebandOverride: null,
      rigMode: 'USB',
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      qsoRecording: false,
      rfPower: null,
      micGain: null,
      nrLevel: 0.3,
      agc: 'fast',
      nb: true,
      nr: true,
      notch: null,
      comp: null,
      vox: null,
      filterWidthHz: null,
      splitTxMhz: null,
      smeterDb: null,
      rxLevel: 0,
      phoneSegLo: null,
      phoneSegHi: null,
    },
  } as unknown as AppSnapshot
}

function fakePanels(removed: PhonePanelId[] = []): PanelLayoutApi<PhonePanelId> {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: (id) => (removed.includes(id) ? 'removed' : 'docked'),
    setPanelState: () => {},
    shareOf: () => 1,
    setShare: () => {},
    setShares: () => {},
    undo: () => {},
    canUndo: false,
    reset: () => {},
  }
}

const view = (removed: PhonePanelId[] = []) => (
  <PhoneCockpit
    snap={makeSnap()}
    theme="dark"
    onWorkSpot={() => {}}
    spots={[]}
    panels={fakePanels(removed)}
  />
)

describe('hiding the Phone voice keyer', () => {
  it('aborts the message in flight — the hide calls stopVoice, it does not leave you keyed', async () => {
    const r = render(view())
    await act(async () => {})
    // The real keyer is mounted (its slot grid rendered from the stubbed engine list).
    expect(document.querySelector('[data-pane="voiceKeyer"] .vk')).not.toBeNull()
    expect(stopVoice).not.toHaveBeenCalled()

    // The operator unticks Voice Keyer. This is the whole safety argument: the pane goes,
    // and the transmission goes with it.
    r.rerender(view(['voiceKeyer']))
    await act(async () => {})
    expect(document.querySelector('[data-pane="voiceKeyer"]')).toBeNull()
    expect(stopVoice, 'hiding the keyer did not abort playback — it stranded it').toHaveBeenCalled()

    // …and the way to stop a transmission the keyer did not start is exactly where it was.
    expect(document.querySelector('.cockpit-txdock .ph-ptt')).not.toBeNull()
  })

  it('hiding any OTHER panel leaves the keyer transmitting undisturbed', async () => {
    // The converse, and the reason the fiber-stability tests exist: a menu interaction
    // that merely reflows the region must not fire the keyer's cleanup. Only its own
    // entry may stop it.
    const r = render(view())
    await act(async () => {})
    r.rerender(view(['dsp']))
    await act(async () => {})
    expect(document.querySelector('[data-pane="voiceKeyer"] .vk')).not.toBeNull()
    expect(stopVoice, 'an unrelated ⊞ toggle aborted the voice keyer').not.toHaveBeenCalled()
  })

  it('the ⊞ entry says what unticking it will do, before the operator unticks it', async () => {
    render(view())
    await act(async () => {})
    fireEvent.click(screen.getByRole('button', { name: /Panels/ }))
    const box = screen.getByRole('checkbox', { name: /Voice Keyer/ }) as HTMLInputElement
    expect(box.checked).toBe(true)
    // Checkable (it is not an unavailable entry) and described by the consequence.
    expect(box.getAttribute('aria-disabled')).toBeNull()
    const whyId = box.getAttribute('aria-describedby')
    expect(whyId, 'the Voice Keyer entry carries no note').not.toBeNull()
    expect(document.getElementById(whyId!)?.textContent).toBe(VOICE_KEYER_STOPS_ON_HIDE)
  })

  it('the note names the RECORDING it destroys, not only the message it stops', async () => {
    // Two different acts ride on one tick and the note has to carry both. Stopping a
    // message is the stop the pane was admitted for; discarding a capture in progress is
    // destruction, and it was not part of that argument. Word-level, because "it says
    // something" is not the property — the operator has to be able to learn THIS from it.
    expect(VOICE_KEYER_STOPS_ON_HIDE).toMatch(/stops? a voice message/i)
    expect(
      VOICE_KEYER_STOPS_ON_HIDE,
      'the note promises a stop and does not mention that a recording is thrown away',
    ).toMatch(/recording/i)
  })

  it('says so when it actually discards the recording, not only in a menu he may never open', async () => {
    // The note is consent at the point of decision, and ⊞ Reset layout / walking off the
    // Phone screen reach the same teardown without passing a menu at all. So the discard
    // announces itself too. Red at the real site by dropping the pushToast from
    // VoiceKeyer's cleanup: "the recording was binned in silence".
    const r = render(view())
    await act(async () => {})
    // Start a recording in slot 1 the way the operator does — the ● button on the slot.
    fireEvent.click(screen.getByTitle('Record from your input device'))
    await act(async () => {})
    expect(startVoiceRecording, 'the harness never started a recording').toHaveBeenCalled()

    r.rerender(view(['voiceKeyer']))
    await act(async () => {})
    expect(cancelVoiceRecording, 'the recorder was left running behind a closed pane').toHaveBeenCalled()
    const said = pushToast.mock.calls.map((c) => String(c[0])).join(' | ')
    expect(said, 'the recording was binned in silence').toMatch(/discard/i)
    expect(said).toMatch(/F1/)
  })

  it('an id whose hide has a consequence at the WIRE carries a note; one that has none does not', async () => {
    // Clause (b2) of the stop line, computed rather than recited. Drive every id in the
    // real vocabulary through the real hide path, ask the WIRE what happened, and require
    // the ⊞ note to agree with it. A second sender admitted tomorrow is covered by this the
    // day it is added — no list to update, and no way to ship it silent.
    //
    // Red both ways at the real site: delete voiceKeyer from `notes` in PhoneCockpit and it
    // fails "stops something on hide and its ⊞ entry says nothing"; add a note to `dsp` and
    // it fails "carries a consequence note but hiding it stops nothing".
    const noteFor = async (id: PhonePanelId) => {
      render(view())
      await act(async () => {})
      fireEvent.click(screen.getByRole('button', { name: /Panels/ }))
      // Scoped to the popover: the cockpit has other checkboxes (the PTT row's hands-free
      // Lock), and a bare getAllByRole would index into them.
      const menu = screen.getByRole('group', { name: /panels on this screen/i })
      const boxes = within(menu).getAllByRole('checkbox') as HTMLInputElement[]
      // The menu is built from PHONE_PANEL_IDS in order, so the id's box is at its index —
      // asserted, not assumed, or a reordering would silently read the wrong entry's note
      // and this guard would go quietly vacuous.
      expect(boxes.length, 'the ⊞ menu no longer lists exactly the Phone vocabulary').toBe(
        PHONE_PANEL_IDS.length,
      )
      const box = boxes[PHONE_PANEL_IDS.indexOf(id)]
      const whyId = box.getAttribute('aria-describedby')
      const text = whyId ? (document.getElementById(whyId)?.textContent ?? '') : ''
      cleanup()
      return text
    }
    const stopsOnHide = async (id: PhonePanelId) => {
      const r = render(view())
      await act(async () => {})
      stopVoice.mockClear()
      cancelVoiceRecording.mockClear()
      r.rerender(view([id]))
      await act(async () => {})
      const fired = stopVoice.mock.calls.length + cancelVoiceRecording.mock.calls.length > 0
      cleanup()
      return fired
    }
    for (const id of PHONE_PANEL_IDS) {
      const stops = await stopsOnHide(id)
      const note = await noteFor(id)
      // "Consequence note" is specifically one that warns about hiding. The station-state
      // notes ("TX meters read on transmit") are a different thing and must not count, or
      // this would pass by accident on txmeters.
      const warns = /hiding this/i.test(note)
      if (stops) {
        expect(
          warns,
          `hiding "${id}" stops something at the wire and its ⊞ entry says nothing about it`,
        ).toBe(true)
      } else {
        expect(
          warns,
          `the "${id}" entry carries a consequence note but hiding it stops nothing — a ` +
            'warning the operator cannot act on teaches him to ignore the next one',
        ).toBe(false)
      }
    }
  })
})
