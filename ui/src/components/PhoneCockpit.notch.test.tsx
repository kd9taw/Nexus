// @vitest-environment jsdom
//
// #95 (Pete-the-Geek, FT-991A): "Notch does not toggle the Notch on the radio and no display
// is available on the screen to adjust the Notch frequency… COMP toggles PROC on the radio but
// no slider is available to fine tune the PROC value."
//
// Two halves. The half that can be fixed from here is the missing CONTROLS: a manual notch and
// somewhere to put it, and a depth for the compressor. The other half — whether his radio's
// automatic notch does anything audible — needs the Yaesu on a bench and is not in this file.
//
// WHAT THESE PIN, and it is the distinction the bug was made of: `notch` is the AUTOMATIC
// notch (Hamlib ANF) and `manualNotch` is the MANUAL one (MN). They are separate rig
// functions at separate indices, a radio may report either, both or neither, and every
// control renders ONLY when its own field is non-null — so no rig grows a slider that does
// nothing, which is the failure being reported from the other direction.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import { setRigFunc } from '../api'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({
  // The Phone cockpit reads the FM repeater shift from Settings — it is the only surface
  // that carries it, and the transmit contract will not state a frequency without it.
  getSettings: vi.fn(async () => ({})),
  setPtt: vi.fn(async () => ({})),
  setTxEnabled: vi.fn(async () => ({})),
  setRfPower: vi.fn(async () => {}),
  setMicGain: vi.fn(async () => {}),
  setNrLevel: vi.fn(async () => {}),
  // The two #95 additions.
  setCompLevel: vi.fn(async () => {}),
  setNotchFreq: vi.fn(async () => {}),
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
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// The scope, band strip, keyer and log form are not what this file is about; stubbing them
// keeps the failure surface to the DSP pane. CockpitHeader is stubbed for the same reason —
// unlike the stop-line sweep, nothing here asserts on a header control.
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))


afterEach(cleanup)

/** A Phone-cockpit snapshot with the rig reporting exactly the DSP surface given. */
function snapWith(radio: Record<string, unknown>): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    radio: {
      dialMhz: 14.2,
      band: '20m',
      sideband: 'USB',
      // ⚠️ catOk, EXPLICITLY. Every case here is about what the RADIO reports; a dead CAT
      // link is a different question with its own answer (the pane banner), and it would
      // otherwise swallow every one of these by explaining them all at once.
      catOk: true,
      transmitting: false,
      txEnabled: false,
      txAllowed: true,
      slot: 0,
      nextSlotMs: 0,
      ...radio,
    },
    link: {},
    qso: {},
    stations: [],
    conversations: [],
  } as unknown as AppSnapshot
}

const mount = (radio: Record<string, unknown>) =>
  render(<PhoneCockpit snap={snapWith(radio)} theme="dark" />)

describe('#95 — the controls that were missing', () => {
  it('offers a manual-notch frequency slider when the rig reports one', () => {
    mount({ notchFreqHz: 1200 })
    const slider = screen.getByLabelText('Manual notch frequency in hertz') as HTMLInputElement
    // Hz, not a percentage — the operator is placing it on a tone they can hear.
    expect(slider.value).toBe('1200')
    expect(slider.min).toBe('300')
    expect(slider.max).toBe('3400')
  })

  it('offers a speech-processor depth slider when the rig reports one', () => {
    mount({ compLevel: 0.4 })
    const slider = screen.getByLabelText('Speech processor depth') as HTMLInputElement
    expect(slider.value).toBe('40')
  })

  it('offers a MANUAL notch toggle, distinct from the automatic one', () => {
    mount({ notch: true, manualNotch: false })
    // Both, because a radio can have both and they do different things.
    expect(screen.getByRole('button', { name: 'Auto notch' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Manual notch' })).toBeTruthy()
  })
})

// ⭐ THESE FOUR USED TO PIN THE OPPOSITE, THE REASON THEY GAVE WAS OVERRULED, AND THE
// ANSWER WAS THEN REFINED AGAIN. Both moves are recorded, because the second one looks from
// a distance like the first being undone.
//
// What they said, verbatim: *"a control that does nothing is worse than none. An
// auto-notch-only radio must not grow a frequency slider with nothing behind it."*
//
// OPERATOR RULING, 2026-09-20: overruled. A control that VANISHES is indistinguishable from
// one that was never built — the operator learns nothing about whether his radio lacks it,
// whether his CAT backend lacks it, or whether Nexus simply never wrote it, and the third
// reading is the one he acts on. #95 was filed in exactly that confusion.
//
// ⛔ AND THEN, 2026-09-21: the ruling does NOT reach a feature this radio family does not
// have. A grey row per missing control makes a cockpit a wall of grey — an IC-7300 would
// open to four of them — so absence COLLAPSES into one line at the pane's foot: "Not on this
// radio: NOTCH · COMP". The operator still sees that Nexus knows the control exists, which
// is the whole thing the ruling was protecting, and the old argument gets most of what it
// wanted back: no dead slider.
//
// What the ruling still governs, and what the `atuStartTuneUnsupported` case is the model
// of, is "the rig can do it, THIS PATH cannot" — where the control stays, disabled, with its
// reason. The pane-wide version of that is the no-CAT banner.
describe('#95 — and the dead controls it must not HIDE SILENTLY', () => {
  /** The pane-foot line that names what this radio does not have. */
  const footLine = () => document.querySelector('[data-pane="receiver"] .ph-chain-absent')?.textContent ?? ''
  const txFootLine = () => document.querySelector('[data-pane="transmitter"] .ph-chain-absent')?.textContent ?? ''

  /** Collapsed, not vanished: no row, and the control's PLATE named at the pane's foot. Two
   *  things, and the second is the one that keeps the ruling's promise. */
  function collapsed(el: HTMLElement | null, plate: string, foot: () => string, what: string) {
    expect(el, `${what}: still drawing a row this radio cannot drive`).toBeNull()
    expect(foot(), `${what}: gone from the pane and named nowhere — that is the vanishing the ruling forbids`).toContain(plate)
  }

  it('collapses the notch-frequency row, and names NOTCH at the pane foot', () => {
    mount({ notch: true })
    collapsed(screen.queryByLabelText('Manual notch frequency in hertz'), 'NOTCH', footLine, 'notch frequency')
  })

  it('collapses the compressor row, and names COMP at the transmit pane foot', () => {
    mount({ comp: true })
    collapsed(screen.queryByLabelText('Speech processor depth'), 'COMP', txFootLine, 'compressor depth')
  })

  it('collapses the manual notch while the AUTOMATIC one stays live', () => {
    // The pair still has to be told apart, which is what this case was always about: the
    // automatic notch is reported and live, the manual one is not — so one is a working
    // button and the other is a name in the foot line, which no operator can confuse.
    mount({ notch: true })
    const auto = screen.getByRole('button', { name: 'Auto notch' }) as HTMLButtonElement
    expect(auto.getAttribute('aria-disabled'), 'the automatic notch is dead on a rig that reports it').toBeNull()
    collapsed(screen.queryByRole('button', { name: 'Manual notch' }), 'Manual notch', footLine, 'manual notch')
  })

  it('control: a bare rig names them all at the foot, and draws none of them', () => {
    // The direction this control guards has inverted twice. It first stopped the
    // "queryBy-toBeNull" assertions passing on a component that rendered nothing; then it
    // stopped every control rendering LIVE over a radio that reports none. It now stops the
    // third failure, which is the one the collapse makes possible: rows quietly disappearing
    // with nothing at the foot to say they were ever there.
    mount({})
    for (const [el, plate] of [
      [screen.queryByLabelText('Manual notch frequency in hertz'), 'NOTCH'],
      [screen.queryByRole('button', { name: 'Manual notch' }), 'Manual notch'],
      [screen.queryByRole('button', { name: 'Auto notch' }), 'Auto notch'],
    ] as const) {
      collapsed(el as HTMLElement | null, plate, footLine, plate)
    }
    collapsed(screen.queryByLabelText('Speech processor depth'), 'COMP', txFootLine, 'compressor depth')
  })

  it('control: the foot line is EMPTY on a rig that reports everything', () => {
    // Without this, every assertion above passes against a cockpit that lists every control
    // as missing on every radio — the collapse turned into a blanket apology.
    mount({
      notch: true, manualNotch: true, notchFreqHz: 1500, comp: true, compLevel: 0.4,
      nb: true, nr: true, nrLevel: 0.3, agc: 'fast', rfGain: 1, afGain: 0.5, squelch: 0,
      micGain: 0.5, vox: false,
    })
    expect(footLine(), 'a fully-reporting rig was told it is missing something').toBe('')
    expect(txFootLine()).toBe('')
    expect(screen.getByLabelText('Manual notch frequency in hertz')).toBeTruthy()
  })
})

// ── what the two buttons are CALLED (#95, and an open FTDX-10 report) ──────────────────
//
// Both were always wired right — `notch` is Hamlib ANF, `manualNotch` is MN — so this is
// not about behaviour. It is that a Yaesu's own front panel calls the AUTOMATIC notch DNF
// and the MANUAL one NOTCH, exactly inverted from the Hamlib vocabulary these buttons wore:
// the operator pressed "Notch" expecting the manual one and got the hunter. Reported twice.
//
// OPERATOR RULING, 2026-09-20: plain function names, the same on every rig and in every
// locale. Not a per-vendor label table — five rotator models sat dead at the wrong baud on
// exactly that pattern — and not Hamlib's words, because the report stands.
//
// ⚠️ THE PAIR IS THE TEST. Either name asserted alone passes unchanged on a build that has
// swapped the two, which is the failure being fixed. So each name is looked up AND pressed,
// and the rig function that comes out the other side is what says the name sits on the right
// button; a swap makes the FIRST press report `manualNotch`.
describe('the notch pair is named for what each one does', () => {
  it('calls them Auto notch and Manual notch, each on its own rig function', () => {
    vi.mocked(setRigFunc).mockClear()
    mount({ notch: false, manualNotch: false })

    fireEvent.click(screen.getByRole('button', { name: 'Auto notch' }))
    expect(vi.mocked(setRigFunc).mock.calls).toEqual([['notch', true]])

    fireEvent.click(screen.getByRole('button', { name: 'Manual notch' }))
    expect(vi.mocked(setRigFunc).mock.calls).toEqual([['notch', true], ['manualNotch', true]])

    // Both presses above landed, so the card is mounted and populated — which is what lets
    // these two read as "the old names are gone" rather than "nothing rendered".
    expect(screen.queryByRole('button', { name: 'Notch' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'MN' })).toBeNull()
  })
})
