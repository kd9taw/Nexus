// @vitest-environment jsdom
//
// THE AGC CHIP MUST NEVER CLAIM A SPEED THE RADIO NEVER TOOK.
//
// Operator report (2026-08-13): "In CW mode, AGC changes for F-M-S work slowly or not at all."
// The rig-side half of that lives in `crates/tempo-audio/src/service.rs` (a pick now forces the
// write past the loop's what-we-last-wrote dedupe, and a refused step stops being re-sent). This
// suite pins the cockpit's half.
//
// `snap.radio.agc` is the rig READ-BACK — it lags up to a heavy poll (750 ms) behind the click,
// so the chip has to be optimistic or the operator clicks into a control that does not respond.
// The old mirror was a `useState` + a `useEffect` keyed on `snap.radio.agc`, and that effect
// only runs when the VALUE CHANGES. On a refusal the value never changes: Hamlib carries AGC as
// an enum (OFF/SUPERFAST/FAST/SLOW/USER/MEDIUM/AUTO) and backends do not all implement every
// step — MEDIUM least of all — so `L AGC 5` comes back `RPRT -1`, the rig stays where it was,
// and the read-back keeps reporting the OLD speed. The mirror therefore lit Mid forever on a
// radio still running Fast: the app believing it applied, which is the one thing it must not do.
//
// The chip is now DERIVED — the pick shows until the snapshot NAMES it as refused — so there is
// no remembered state to go stale, and both directions are asserted here.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, fireEvent, act } from '@testing-library/react'
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

const { setAgc } = vi.hoisted(() => ({ setAgc: vi.fn(async () => ({})) }))

vi.mock('../api', () => ({
  // Hand-kept mock: an export CwCockpit calls but this list omits makes it THROW ON MOUNT,
  // which reads as a behaviour regression rather than the stale mock it actually is.
  getCatCwUnprovenRigModels: vi.fn(async () => []),
  setAgc,
  getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 } })),
  setSettings: vi.fn(async () => ({})),
  sendCw: vi.fn(async () => {}),
  setCwKeyer: vi.fn(async () => null),
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
}))

vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

afterEach(() => {
  cleanup()
  setAgc.mockClear()
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

/** The AGC chips, by their accessible label — the operator's own handle on the control. */
function chip(label: 'Fast' | 'Mid' | 'Slow') {
  const group = document.querySelector('.ph-agc')!
  return Array.from(group.querySelectorAll('button')).find(
    (b) => b.textContent?.trim() === label,
  ) as HTMLButtonElement
}
const lit = () =>
  Array.from(document.querySelectorAll('.ph-agc button'))
    .filter((b) => b.getAttribute('aria-pressed') === 'true')
    .map((b) => b.textContent?.trim())

describe('the CW cockpit AGC chip', () => {
  it('lights on the click, ahead of the rig read-back', async () => {
    // The whole reason the control is optimistic: `setAgc` returns a snapshot carrying the OLD
    // read-back, and the real one is up to a heavy poll away. Reading the snapshot straight
    // would leave the chip dead under the operator's finger for ~a second.
    const { rerender } = await renderCw()
    expect(lit()).toEqual(['Fast'])

    await act(async () => {
      fireEvent.click(chip('Slow'))
    })
    expect(setAgc).toHaveBeenCalledWith('slow')
    expect(lit(), 'the click must light its own chip immediately').toEqual(['Slow'])

    // …and a snapshot still reporting the OLD speed must not snatch it back.
    await act(async () => {
      rerender(<CwCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    })
    expect(lit()).toEqual(['Slow'])
  })

  it('hands the chip back to the rig when the rig REFUSES the step', async () => {
    // ⭐ THE REGRESSION. A rig whose Hamlib backend has no MEDIUM answers `L AGC 5` with
    // RPRT -1: the radio stays on Fast and every read-back keeps saying "fast", so nothing
    // about `snap.radio.agc` ever changes. The old mirror synced only on a change, so it
    // never corrected — the cockpit claimed Mid for the rest of the session.
    const { rerender } = await renderCw()
    await act(async () => {
      fireEvent.click(chip('Mid'))
    })
    expect(lit(), 'optimistic, as designed').toEqual(['Mid'])

    // The loop's verdict comes back on the next snapshot: the rig refused Mid.
    await act(async () => {
      rerender(
        <CwCockpit
          snap={makeSnap({ refusedAgc: 'mid' })}
          theme="dark"
          onWorkSpot={() => {}}
          spots={[]}
        />,
      )
    })
    expect(lit(), 'the chip must show what the RADIO is on, not what was asked for').toEqual([
      'Fast',
    ])
  })

  it('a refusal does not poison the next pick', async () => {
    // The other direction — a give-up that outlived its cause would be its own bug. With Mid
    // refused and still named in the snapshot, picking SLOW must light Slow: the refusal is
    // about one step, not about the control.
    await renderCw({ refusedAgc: 'mid' })
    expect(lit()).toEqual(['Fast'])

    await act(async () => {
      fireEvent.click(chip('Slow'))
    })
    expect(setAgc).toHaveBeenCalledWith('slow')
    expect(lit()).toEqual(['Slow'])
  })

  it('re-picking the speed the rig is already on still reaches the wire', async () => {
    // The "or not at all" half, seen from the UI. The radio's own AGC knob and its per-mode
    // AGC memory move the rig without us, so re-picking the speed the app last commanded is a
    // real gesture with a real effect — the cockpit must never swallow it as a no-op. (What
    // the RADIO LOOP then does with it is pinned at the wire in
    // `a_reselected_agc_speed_reaches_a_rig_that_moved_on_its_own`.)
    await renderCw()
    expect(lit()).toEqual(['Fast'])
    await act(async () => {
      fireEvent.click(chip('Fast'))
    })
    expect(setAgc).toHaveBeenCalledWith('fast')
  })
})
