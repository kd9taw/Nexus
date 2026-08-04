// @vitest-environment jsdom
//
// HIDING THE VOICE KEYER IS A STOP, AND THE OPERATOR IS TOLD BOTH HALVES OF IT FIRST.
//
// THIS FILE COMPUTES THE PRACTICE, NOT THE RULE. THE STOP LINE (features/panelState.ts) is
// the rule and it is one sentence: the operator must never be unable to stop a transmission —
// mechanically, in every cockpit at least one control that stops one renders OUTSIDE every
// ⊞-removable pane. That rule says nothing about this pane. The voice keyer transmits AND
// hosts a ■ Stop of its own, and neither fact bears on whether it may be hidden: Stop TX,
// Tune and PTT are not panels, so the menu cannot reach the operator's last resort whatever
// he ticks, and the keyer's own Stop is a convenience that may go away with it. FOUR earlier
// wordings turned on one of those two facts; all four were falsified (see panelState.ts for
// each and the shipped code that killed it).
//
// What IS true, and what this file pins, is THE PRACTICE: a hide that ENDS something already
// in flight should say so before the tick, because a stop the operator did not ask for reads
// as a dropout. Courtesy, not safety — but courtesy worth computing, because a warning that
// goes missing is invisible and one that cries wolf trains the operator to ignore the next.
// Two properties at the real site:
//   (1) the keyer's hide really does end things — unmounting it calls stopVoice, and also
//       cancels a recording in progress;
//   (2) its ⊞ entry says so before the tick, and an entry whose hide ends nothing does not.
//
// (1) is why the suite renders the REAL VoiceKeyer (every other Phone suite stubs it)
// inside the REAL cockpit and hides it through the panel record the ⊞ menu writes. A test
// against a stub would prove the pane disappears and nothing about the transmitter.
//
// (2) used to be an assertion about ONE id, which is recitation dressed as enforcement: a
// second such pane would have shipped noteless. It is computed here instead, and the two
// properties are wired to each other — the suite drives EVERY id in the Phone vocabulary
// through the hide path, watches which ones fire a stop at the wire, and requires exactly
// those to carry a note. No declaration list to keep in step: an id that stops something on
// hide and says nothing fails, and so does a scare note on an id that stops nothing. The
// five Phone panes whose hides end nothing must carry no warning, and that is asserted too.
//
// The keyer's hide fires TWO teardowns, and they are different acts. stopVoice ends a
// message — the one the note was written for. cancelVoiceRecording DISCARDS a capture in
// progress, which is destruction, not safety, and is not implied by "hide a pane". It is
// named in the note before the tick and announced by a toast when it happens; both are
// pinned below.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, act, within } from '@testing-library/react'
import { PhoneCockpit, VOICE_KEYER_STOPS_ON_HIDE, VOICE_KEYER_UNDO_ENDS } from './PhoneCockpit'
import type { AppSnapshot } from '../types'
import { PHONE_PANELS, PHONE_PANEL_IDS, usePanelLayout } from '../features/panelState'
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
  // cleanup() BEFORE the clears, and that ordering is load-bearing: unmounting the tree runs
  // the keyer's own teardown, so clearing first left a stopVoice call from THIS test sitting
  // in the counter for the next one. Any case that ended with the keyer still mounted was
  // handing its successor a phantom stop.
  cleanup()
  stopVoice.mockClear()
  cancelVoiceRecording.mockClear()
  cancelVoiceRecording.mockImplementation(async () => ({}))
  startVoiceRecording.mockClear()
  pushToast.mockClear()
  // The cases driven by the real panel record persist through localStorage; without this
  // each one would start from the previous one's layout.
  localStorage.clear()
})

function makeSnap(transmitting = false): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.2,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      sidebandOverride: null,
      rigMode: 'USB',
      transmitting,
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
    undoRemoves: [],
    reset: () => {},
  }
}

const view = (removed: PhonePanelId[] = [], transmitting = false) => (
  <PhoneCockpit
    snap={makeSnap(transmitting)}
    theme="dark"
    onWorkSpot={() => {}}
    spots={[]}
    panels={fakePanels(removed)}
  />
)

/**
 * The cockpit driven by the REAL panel record — `usePanelLayout(PHONE_PANELS)`, exactly as
 * App builds it. The fake above cannot serve for the ⊞ menu's own buttons: Undo and Reset
 * are history operations, and a stub with `undo: () => {}` makes every claim about them
 * vacuous. Storage is per-test (cleared in afterEach), so each case starts at stock.
 */
function LivePanels({ transmitting = false }: { transmitting?: boolean }) {
  const panels = usePanelLayout(PHONE_PANELS)
  return (
    <PhoneCockpit
      snap={makeSnap(transmitting)}
      theme="dark"
      onWorkSpot={() => {}}
      spots={[]}
      panels={panels}
    />
  )
}

/** Open ⊞ Panels (idempotent — the trigger TOGGLES) and return the popover. */
function openMenu() {
  const btn = screen.getByRole('button', { name: /Panels/ })
  if (btn.getAttribute('aria-expanded') !== 'true') fireEvent.click(btn)
  return screen.getByRole('group', { name: /panels on this screen/i })
}

/** The text a control's aria-describedby resolves to, or '' when it has none. */
function descriptionOf(el: Element): string {
  const id = el.getAttribute('aria-describedby')
  return id ? (document.getElementById(id)?.textContent ?? '') : ''
}

/** Tick or untick one entry by its label, through the real checkbox. */
function toggle(label: RegExp, on: boolean) {
  const box = within(openMenu()).getByRole('checkbox', { name: label }) as HTMLInputElement
  fireEvent.click(box)
  expect(box.checked).toBe(on)
}

describe('hiding the Phone voice keyer', () => {
  it('aborts the message in flight — the hide calls stopVoice, it does not leave you keyed', async () => {
    const r = render(view())
    await act(async () => {})
    // The real keyer is mounted (its slot grid rendered from the stubbed engine list).
    expect(document.querySelector('[data-pane="voiceKeyer"] .vk')).not.toBeNull()
    expect(stopVoice).not.toHaveBeenCalled()

    // The operator unticks Voice Keyer: the pane goes, and the transmission goes with it.
    // Not the safety argument — Stop TX and PTT are still there either way — but the
    // behaviour the ⊞ note promises, and the reason the note has to exist at all.
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
    // message is the one the note was written for; discarding a capture in progress is
    // destruction, and it was not part of that argument. Word-level, because "it says
    // something" is not the property — the operator has to be able to learn THIS from it.
    expect(VOICE_KEYER_STOPS_ON_HIDE).toMatch(/stops? a voice message/i)
    expect(
      VOICE_KEYER_STOPS_ON_HIDE,
      'the note promises a stop and does not mention that a recording is thrown away',
    ).toMatch(/recording/i)
  })

  it('says so when it actually discards the recording, not only in a menu he may never open', async () => {
    // The note is consent at the point of decision, and walking off the Phone screen reaches
    // the same teardown without passing a menu at all. So the discard announces itself too.
    // Red at the real site by dropping the pushToast from VoiceKeyer's cleanup: "the
    // recording was binned in silence".
    const r = render(view())
    await act(async () => {})
    // Start a recording in slot 1 the way the operator does — the ● button on the slot.
    // Matched on the title's opening rather than the whole string: since the 2026-08-04
    // density pass that title also carries the input-device warning the `.vk-note` paragraph
    // used to hold (PhoneCockpit.density.test.tsx owns that sentence). Slot 1 is this
    // fixture's only slot and it HAS a file, so the ● tool is the only Record here.
    fireEvent.click(screen.getByTitle(/^Record F1\./))
    await act(async () => {})
    expect(startVoiceRecording, 'the harness never started a recording').toHaveBeenCalled()

    r.rerender(view(['voiceKeyer']))
    await act(async () => {})
    expect(cancelVoiceRecording, 'the recorder was left running behind a closed pane').toHaveBeenCalled()
    const said = pushToast.mock.calls.map((c) => String(c[0])).join(' | ')
    expect(said, 'the recording was binned in silence').toMatch(/discard/i)
    expect(said).toMatch(/F1/)
  })

  it('the over it aborts is announced too, not only the recording it bins', async () => {
    // The argument that made the discard speak up ("a stop the operator did not ask for is
    // indistinguishable from a dropout") applies at least as hard to a transmission cut
    // short, and that one used to end in silence. The keyer knows a message is on the air
    // from two things at once: it started one, and the rig is keyed — so drive both.
    // Red at the real site by deleting the pushToast from VoiceKeyer's cleanup.
    const r = render(<LivePanels />)
    await act(async () => {})
    fireEvent.click(screen.getByTitle('Play F1 (CQ)'))
    await act(async () => {})
    // The rig keys — the snapshot lags the send by a poll, which is why the pane waits for it.
    r.rerender(<LivePanels transmitting />)
    await act(async () => {})

    toggle(/Voice Keyer/, false)
    await act(async () => {})
    expect(document.querySelector('[data-pane="voiceKeyer"]')).toBeNull()
    const said = pushToast.mock.calls.map((c) => String(c[0])).join(' | ')
    expect(said, 'the over was cut off in silence').toMatch(/on the air/i)
    expect(said).toMatch(/F1/)
  })

  it('says nothing about an over when the rig was never keyed', async () => {
    // The converse, and the reason the toast reads two signals rather than one: a message
    // the engine refused (no privileges, TX not allowed) leaves the pane's own "I started
    // one" flag set with nothing ever on the air. A notice about a transmission that never
    // happened teaches the operator to ignore the next one.
    render(<LivePanels />)
    await act(async () => {})
    fireEvent.click(screen.getByTitle('Play F1 (CQ)'))
    await act(async () => {})
    toggle(/Voice Keyer/, false)
    await act(async () => {})
    const said = pushToast.mock.calls.map((c) => String(c[0])).join(' | ')
    expect(said, 'claimed to have stopped an over that never keyed').not.toMatch(/on the air/i)
  })

  it('a cancel the backend REFUSED does not report the take as discarded', async () => {
    // The discard toast was unconditional while cancelVoiceRecording's rejection was
    // swallowed, so an IPC failure told the operator his recording was gone while the
    // backend recorder ran on — the one direction a safety notice must never be wrong in.
    cancelVoiceRecording.mockImplementation(async () => {
      throw new Error('ipc down')
    })
    render(<LivePanels />)
    await act(async () => {})
    fireEvent.click(screen.getByTitle(/^Record F1\./))
    await act(async () => {})
    toggle(/Voice Keyer/, false)
    await act(async () => {})
    const said = pushToast.mock.calls.map((c) => String(c[0])).join(' | ')
    expect(said, 'reported a discard that did not happen').not.toMatch(/discarded/i)
    expect(said, 'the recorder may still be running and nothing said so').toMatch(/still be running/i)
  })

  it('⊞ Undo is a SECOND hide path, and it says what it will end before the press', async () => {
    // Reproduced before it was covered: untick the keyer, tick it back, start a recording,
    // press ⊞ Undo — the keyer unmounts, cancelVoiceRecording bins the take, and the only
    // word about it arrived after the fact. The tick warns first; so must this. The real
    // panel record is required here — a stubbed `undo: () => {}` proves nothing about it.
    render(<LivePanels />)
    await act(async () => {})
    toggle(/Voice Keyer/, false)
    await act(async () => {})
    toggle(/Voice Keyer/, true)
    await act(async () => {})
    expect(document.querySelector('[data-pane="voiceKeyer"] .vk')).not.toBeNull()

    // BEFORE the press: the consequence is on the button the operator is about to click.
    const undo = within(openMenu()).getByRole('button', { name: /undo last change/i })
    expect((undo as HTMLButtonElement).disabled, 'nothing to undo — the case is vacuous').toBe(false)
    expect(
      descriptionOf(undo),
      'the ⊞ Undo that will unmount the voice keyer says nothing about it',
    ).toBe(VOICE_KEYER_UNDO_ENDS)

    // …and the press really does reach the same teardown the tick does.
    fireEvent.click(screen.getByTitle(/^Record F1\./))
    await act(async () => {})
    cancelVoiceRecording.mockClear()
    fireEvent.click(within(openMenu()).getByRole('button', { name: /undo last change/i }))
    await act(async () => {})
    expect(document.querySelector('[data-pane="voiceKeyer"]'), 'Undo did not hide the keyer — this case proves nothing').toBeNull()
    expect(cancelVoiceRecording, 'the recorder was left running behind a closed pane').toHaveBeenCalled()
  })

  it('an Undo that ends nothing carries no warning', async () => {
    // The converse, same reason as the entry notes: a warning the operator cannot act on
    // teaches him to ignore the next one. Undoing a re-tick of DSP Functions hides a pane
    // whose hide ends nothing — which is every entry in every vocabulary but the keyer's.
    render(<LivePanels />)
    await act(async () => {})
    toggle(/DSP Functions/, false)
    await act(async () => {})
    toggle(/DSP Functions/, true)
    await act(async () => {})
    const undo = within(openMenu()).getByRole('button', { name: /undo last change/i })
    expect((undo as HTMLButtonElement).disabled).toBe(false)
    expect(
      descriptionOf(undo),
      'Undo warns about ending a transmission when the pane it hides starts none',
    ).toBe('')
  })

  it('⊞ Reset layout can only MOUNT the keyer — it is not a teardown path', async () => {
    // Three places said the discard notice "covers Reset layout". It cannot: reset applies
    // `emptyPanelLayout()` and `stateOf` reads an absent state as 'docked', so the one thing
    // reset can never do is remove a pane. Driven rather than argued, because the claim was
    // printed in the CHANGELOG for operators to rely on.
    render(<LivePanels />)
    await act(async () => {})
    expect(document.querySelector('[data-pane="voiceKeyer"] .vk')).not.toBeNull()
    fireEvent.click(within(openMenu()).getByRole('button', { name: /reset layout/i }))
    await act(async () => {})
    expect(document.querySelector('[data-pane="voiceKeyer"] .vk'), 'Reset unmounted the keyer').not.toBeNull()
    expect(stopVoice, 'Reset tore down the keyer').not.toHaveBeenCalled()

    // And from the hidden state it puts the pane BACK — the only direction it moves.
    toggle(/Voice Keyer/, false)
    await act(async () => {})
    expect(document.querySelector('[data-pane="voiceKeyer"]')).toBeNull()
    fireEvent.click(within(openMenu()).getByRole('button', { name: /reset layout/i }))
    await act(async () => {})
    expect(document.querySelector('[data-pane="voiceKeyer"] .vk'), 'Reset did not restore the keyer').not.toBeNull()
  })

  it('an id whose hide has a consequence at the WIRE carries a note; one that has none does not', async () => {
    // THE PRACTICE, computed rather than recited. Drive every id in the real vocabulary
    // through the real hide path, ask the WIRE what happened, and require the ⊞ note to
    // agree with it. A second pane whose hide ends something is covered by this the day it
    // is added — no list to update, and no way to ship it silent. (A sender whose hide ends
    // NOTHING is not this test's business, and is not the rule's either: it is hideable and
    // it warns about nothing, which is what Operate's Tx messages already do.)
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
