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

// ⭐ THESE FOUR USED TO PIN THE OPPOSITE, AND THE REASON THEY GAVE WAS OVERRULED, NOT FORGOTTEN.
//
// What they said, verbatim: *"a control that does nothing is worse than none. An auto-notch-only
// radio must not grow a frequency slider with nothing behind it."* That is a real argument — a
// live-looking slider over a radio that cannot move is a lie about the radio — and #95 was
// partly a report of exactly that shape from the other direction.
//
// OPERATOR RULING, 2026-09-20: it is overruled, on a ground the argument does not answer. A
// control that VANISHES is indistinguishable from one that was never built. The operator who
// cannot find a manual notch learns nothing about whether his radio lacks it, whether his CAT
// backend lacks it, or whether Nexus simply never wrote it — and the third reading is the one
// he acts on, because it is the only one he can do something about. #95 itself was filed in
// that confusion. So the control stays, DISABLED, carrying the reason as text.
//
// The old argument survives as a CONSTRAINT rather than a behaviour: the control must be
// visibly dead and must say why, which is what each assertion below now demands — present AND
// disabled AND a reason, three things, because any two of them pass on a half-built control.
// The general shape of the rule is pinned in PhoneCockpit.chain.test.tsx; what is here is the
// #95 surface specifically, in the file that reported it.
describe('#95 — and the dead controls it must not HIDE', () => {
  /** Present, disabled, and carrying a reason — the three the ruling turns on. */
  function deadAndSaid(el: HTMLElement | null, what: string) {
    expect(el, `${what}: not on screen — a vanished control reads as one Nexus never built`).toBeTruthy()
    expect((el as HTMLInputElement | HTMLButtonElement).disabled, `${what}: live over a radio that cannot drive it`).toBe(true)
    const mark = el!.closest('[data-chain]')?.querySelector('.ph-unavail')
    expect(mark, `${what}: dead and silent — the operator cannot tell "your radio can't" from "Nexus didn't"`).not.toBeNull()
    expect(mark!.textContent, `${what}: the reason is not text`).toMatch(/\S/)
  }

  it('shows the notch-frequency slider DISABLED, with the reason, on a rig that does not report one', () => {
    mount({ notch: true })
    deadAndSaid(screen.queryByLabelText('Manual notch frequency in hertz'), 'notch frequency')
  })

  it('shows the compressor slider DISABLED, with the reason, on a rig that does not report one', () => {
    mount({ comp: true })
    deadAndSaid(screen.queryByLabelText('Speech processor depth'), 'compressor depth')
  })

  it('shows the manual-notch button DISABLED, with the reason, on a rig with only the automatic notch', () => {
    // The pair still has to be told apart, and that is what this case is really about: the
    // AUTOMATIC notch is live here and the MANUAL one is not, so the two must render
    // differently — which a disabled state and a reason do, and a missing button cannot.
    mount({ notch: true })
    const auto = screen.getByRole('button', { name: 'Auto notch' }) as HTMLButtonElement
    expect(auto.disabled, 'the automatic notch is dead on a rig that reports it').toBe(false)
    deadAndSaid(screen.queryByRole('button', { name: 'Manual notch' }), 'manual notch')
  })

  it('control: a bare rig grows all of them, every one dead and every one saying why', () => {
    // The direction this control guards has INVERTED with the ruling. It used to stop the
    // "queryBy … toBeNull" assertions above passing on a component that rendered nothing;
    // now the risk is the mirror image — a component that renders every control LIVE over a
    // radio that reports nothing, which is the lie the overruled argument was written
    // against. So each one is checked dead AND explained.
    mount({})
    deadAndSaid(screen.queryByLabelText('Manual notch frequency in hertz'), 'notch frequency')
    deadAndSaid(screen.queryByLabelText('Speech processor depth'), 'compressor depth')
    deadAndSaid(screen.queryByRole('button', { name: 'Manual notch' }), 'manual notch')
    deadAndSaid(screen.queryByRole('button', { name: 'Auto notch' }), 'auto notch')
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
