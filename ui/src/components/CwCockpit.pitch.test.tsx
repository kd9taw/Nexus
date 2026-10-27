// @vitest-environment jsdom
//
// THE CW PITCH BOX COULD ONLY HOLD 300 OR 1200. It clamped to 300–1200 Hz on every keystroke, so
// the "6" of "650" became 300 at once and the next keys built 3005 → 1200; only the spinner could
// reach anything between. The box now keeps the typed text until the operator is done: a value
// inside the range applies as it is typed (and a spinner step at once), and blur or Enter clamps
// whatever is in the box. Typed ONE KEY AT A TIME, each onto what the box holds after React has
// answered the last one: a whole-string change is a paste, and a paste never showed the bug.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, act } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import type { AppSnapshot } from '../types'

const decodeState = {
  text: '',
  wpm: 22,
  sent: [] as string[],
  keyerError: null as string | null,
  candidates: [] as { call: string; best: boolean }[],
  state: 'listening',
  headline: '',
  prompt: '',
  recommended: null as string | null,
  workedCall: null as string | null,
  rst: null as string | null,
  name: null as string | null,
}

const { setCwKeyer } = vi.hoisted(() => ({ setCwKeyer: vi.fn(async (..._a: unknown[]) => null) }))

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
    sendCw: vi.fn(async () => {}),
    setCwKeyer,
    setCwWpm: vi.fn(async () => {}),
    stopCw: vi.fn(async () => {}),
    cwDecode: vi.fn(async () => decodeState),
    cwClear: vi.fn(async () => {}),
    setAiCw: vi.fn(async () => {}),
    selectPeer: vi.fn(async () => null),
    previewCw: vi.fn(async (t: string) => t),
    pointRotatorAtCall: vi.fn(async () => 0),
    setRigFunc: vi.fn(async () => ({})),
    setFilterWidth: vi.fn(async () => ({})),
    setNrLevel: vi.fn(async () => {}),
    setScopeSpan: vi.fn(async () => ({})),
    setScopeRef: vi.fn(async () => {}),
    setFlexPanSpan: vi.fn(async () => ({})),
    setFlexPanRef: vi.fn(async () => ({})),
    openPanelWindow: vi.fn(async () => {}),
    setTune: vi.fn(async () => ({})),
    setFrequency: vi.fn(async () => ({})),
    haltTx: vi.fn(async () => ({})),
  }
})

// The REAL CockpitHeader: the pitch box is one of its children. Its band and tuning controls are stubbed.
vi.mock('./BandPicker', () => ({ BandPicker: () => null }))
vi.mock('./TuningStrip', () => ({ TuningStrip: () => null }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

afterEach(() => {
  cleanup()
  setCwKeyer.mockClear()
})

function makeSnap(over: Record<string, unknown> = {}): AppSnapshot {
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
      // The rig is on FAST and says so on every read-back.
      agc: 'fast',
      refusedAgc: null,
      nb: true,
      nr: true,
      notch: null,
      filterWidthHz: 500,
      splitTxMhz: null,
      smeterDb: null,
      ...over,
    },
  } as unknown as AppSnapshot
}

async function renderCw(over: Record<string, unknown> = {}) {
  const r = render(
    <CwCockpit snap={makeSnap(over)} theme="dark" onWorkSpot={() => {}} spots={[]} />,
  )
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

function pitchBox(): HTMLInputElement {
  return screen.getByRole('spinbutton', { name: 'CW pitch (Hz)' }) as HTMLInputElement
}

/** Select-all and type over, one key at a time. */
function typeOver(input: HTMLInputElement, text: string) {
  fireEvent.focus(input)
  fireEvent.change(input, { target: { value: '' } })
  for (const ch of text) fireEvent.change(input, { target: { value: input.value + ch } })
}

/** The pitches the keyer was sent, in order. */
const sentPitches = () => setCwKeyer.mock.calls.map((c) => c[1])

describe('CW pitch box', () => {
  it('takes a typed 650 and sends 650 — never 300 or 1200 on the way', async () => {
    await renderCw()
    const box = pitchBox()
    typeOver(box, '650')
    expect(box.value).toBe('650')
    fireEvent.blur(box)
    expect(box.value).toBe('650')
    expect(sentPitches()).toEqual([650])
  })

  it('commits on Enter as on blur', async () => {
    await renderCw()
    const box = pitchBox()
    typeOver(box, '1000')
    fireEvent.keyDown(box, { key: 'Enter' })
    expect(box.value).toBe('1000')
    expect(sentPitches()).toEqual([1000])
  })

  it('clamps an out-of-range entry when the operator leaves the box, not while typing', async () => {
    await renderCw()
    const box = pitchBox()
    typeOver(box, '50')
    expect(box.value).toBe('50') // still the operator's text
    expect(sentPitches()).toEqual([]) // nothing out of range goes to the keyer
    fireEvent.blur(box)
    expect(box.value).toBe('300')
    expect(sentPitches()).toEqual([300])

    typeOver(box, '1500')
    fireEvent.blur(box)
    expect(box.value).toBe('1200')
    expect(sentPitches()).toEqual([300, 1200])
  })

  it('a spinner step applies at once', async () => {
    await renderCw()
    const box = pitchBox()
    fireEvent.change(box, { target: { value: '610' } })
    expect(sentPitches()).toEqual([610])
    expect(box.value).toBe('610')
  })

  it('a cleared box goes back to the pitch it had, sending nothing', async () => {
    await renderCw()
    const box = pitchBox()
    typeOver(box, '')
    fireEvent.blur(box)
    expect(box.value).toBe('600')
    expect(sentPitches()).toEqual([])
  })
})
