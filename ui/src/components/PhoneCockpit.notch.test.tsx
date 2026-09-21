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

describe('#95 — and the dead controls it must not create', () => {
  it('shows NO notch-frequency slider on a rig that does not report one', () => {
    // The other half of the report: a control that does nothing is worse than none. An
    // auto-notch-only radio must not grow a frequency slider with nothing behind it.
    mount({ notch: true })
    expect(screen.queryByLabelText('Manual notch frequency in hertz')).toBeNull()
  })

  it('shows NO compressor slider on a rig that does not report one', () => {
    mount({ comp: true })
    expect(screen.queryByLabelText('Speech processor depth')).toBeNull()
  })

  it('shows NO manual-notch button on a rig with only the automatic notch', () => {
    mount({ notch: true })
    expect(screen.getByRole('button', { name: 'Auto notch' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Manual notch' })).toBeNull()
  })

  it('control: a bare rig grows none of them', () => {
    // Without this, every "queryBy … toBeNull" above would pass on a component that renders
    // nothing at all, which is the failure mode of a negative assertion.
    mount({})
    expect(screen.queryByLabelText('Manual notch frequency in hertz')).toBeNull()
    expect(screen.queryByLabelText('Speech processor depth')).toBeNull()
    expect(screen.queryByRole('button', { name: 'Manual notch' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Auto notch' })).toBeNull()
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
