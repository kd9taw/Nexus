// @vitest-environment jsdom
//
// ⛔ HOW A PICTURE ENDED IS SAID ONCE, AND "FINISHED" ONLY FOR ONE THAT PLAYED OUT (N57b,
// 2026-09-30). A picture stopped part-way through the abort path (TX Off, Stop TX, a halt) was
// announced "SSTV transmit finished", and the dock's own Stop said "SSTV transmit stopped" and
// then "finished". The states polled below are the ones the engine leaves (pinned on the real
// loop by `a_pictures_progress_tells_a_play_out_from_every_early_end`): a picture that played out
// keeps its whole key-down elapsed; every early end clears it; a cut adds its notice.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, act, cleanup, fireEvent } from '@testing-library/react'
import { SstvView } from './SstvView'
import * as api from '../api'
import { announce as announceFn } from '../announce'
import { EN } from '../i18n'
import type { AppSnapshot, SstvHealth, SstvState } from '../types'

vi.mock('./Waterfall', () => ({ Waterfall: () => null }))
// The rotor strip has its own suite (SstvView.rotor.test.tsx), and it reaches the rotator on mount.
vi.mock('./RotorStrip', () => ({ RotorStrip: () => null }))
vi.mock('../api', () => ({
  getSstvState: vi.fn(),
  sstvArm: vi.fn(),
  sstvAutoArm: vi.fn(),
  getLicensedBandPlan: vi.fn(),
  sstvSend: vi.fn(),
  sstvStop: vi.fn(),
  setOperatingMode: vi.fn(),
}))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn() }))
vi.mock('../announce', () => ({ announce: vi.fn() }))

const announce = vi.mocked(announceFn)
const getSstvState = api.getSstvState as ReturnType<typeof vi.fn>
const sstvAutoArm = api.sstvAutoArm as ReturnType<typeof vi.fn>
const getLicensedBandPlan = api.getLicensedBandPlan as ReturnType<typeof vi.fn>
const sstvStop = api.sstvStop as ReturnType<typeof vi.fn>

const NO_HEALTH: SstvHealth = {
  armed: false,
  audioPeak: 0,
  lastAudioUnix: null,
  drains: 0,
  visSeen: 0,
  lastVisUnix: null,
  unknownVis: 0,
  lastUnknownVisCode: null,
  lastUnknownVisUnix: null,
  images: 0,
  lastImageUnix: null,
}

const IDLE: SstvState = {
  armed: false,
  mode: null,
  linesDone: 0,
  linesTotal: 0,
  previewRgbBase64: null,
  previewWidth: 0,
  previewHeight: 0,
  hedrShiftHz: 0,
  gallery: [],
  health: NO_HEALTH,
  sending: false,
  txMode: null,
  txProgress: 0,
  txElapsedSecs: 0,
  txTotalSecs: 0,
}

/** A PD-120 picture 36 s into its two minutes. */
const ON_AIR: SstvState = {
  ...IDLE,
  sending: true,
  txMode: 'PD-120',
  txProgress: 0.3,
  txElapsedSecs: 36,
  txTotalSecs: 120,
}
/** What the engine leaves when that picture plays out: its mode, and its whole key-down. */
const PLAYED_OUT: SstvState = {
  ...IDLE,
  txMode: 'PD-120',
  txProgress: 1,
  txElapsedSecs: 120,
  txTotalSecs: 120,
}
/** …and when it ends early — Stop, TX Off, Stop TX or a halt all clear its mode and progress. */
const ENDED_EARLY: SstvState = IDLE
/** The radio loop's cut, as `SSTV_STOPPED_TX_OFF` words it. */
const CUT =
  'SSTV stopped: transmit was turned off while the picture was going out, so the rest of it ' +
  'was not sent. Send it again when you are ready.'

const snap = {
  mycall: 'KD9TAW',
  radio: {
    dialMhz: 14.23,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: true,
    tuning: false,
    txAllowed: true,
  },
} as unknown as AppSnapshot

const FINISHED = EN['sstv.tx.announce.finished']
const STOPPED = EN['sstv.tx.announce.stopped']

async function settle() {
  await act(async () => {
    for (let i = 0; i < 6; i++) await Promise.resolve()
  })
}

/** One 1 Hz poll tick answering `state`. */
async function poll(state: SstvState) {
  getSstvState.mockResolvedValue(state)
  await act(async () => {
    vi.advanceTimersByTime(1000)
  })
  await settle()
}

function said(text: string): number {
  return announce.mock.calls.filter(([t]) => t === text).length
}

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true })
  getSstvState.mockReset().mockResolvedValue(IDLE)
  sstvAutoArm.mockReset().mockResolvedValue(IDLE)
  getLicensedBandPlan.mockReset().mockResolvedValue([])
  sstvStop.mockReset()
  announce.mockReset()
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('SstvView: how a picture ended is said once', () => {
  it('a picture that played out is announced as finished, once', async () => {
    render(<SstvView snap={snap} />)
    await settle()
    await poll(ON_AIR)
    await poll(PLAYED_OUT)
    expect(said(FINISHED), 'finished once').toBe(1)
    expect(said(STOPPED), 'never stopped').toBe(0)
  })

  it('the dock\'s own Stop is announced "stopped" once, never "finished"', async () => {
    // What `sstv_stop` answers at once: the picture cleared, the loop not yet past its abort.
    sstvStop.mockResolvedValue({
      ...ON_AIR,
      txMode: null,
      txProgress: 0,
      txElapsedSecs: 0,
      txTotalSecs: 0,
    })
    render(<SstvView snap={snap} />)
    await settle()
    await poll(ON_AIR)
    fireEvent.click(screen.getByRole('button', { name: /^stop$/i }))
    await settle()
    await poll(ENDED_EARLY) // the loop has taken the abort
    expect(sstvStop).toHaveBeenCalledTimes(1)
    expect(said(FINISHED), 'never finished').toBe(0)
    expect(said(STOPPED), 'stopped once').toBe(1)
  })

  it.each(['TX Off', 'Stop TX', 'a halt'])(
    '%s part-way through is announced "stopped" once, never "finished"',
    async () => {
      render(<SstvView snap={snap} />)
      await settle()
      await poll(ON_AIR)
      await poll(ENDED_EARLY)
      expect(said(FINISHED), 'never finished').toBe(0)
      expect(said(STOPPED), 'stopped once').toBe(1)
      expect(screen.queryByRole('alert'), 'no warning line for a stop you made').toBeNull()
    },
  )

  it('a picture the radio loop cut short shows its warning line, and is never announced finished', async () => {
    render(<SstvView snap={snap} />)
    await settle()
    await poll(ON_AIR)
    await poll({ ...ENDED_EARLY, txNotice: CUT })
    expect(screen.getByRole('alert').textContent, 'its warning line').toContain(CUT)
    expect(said(FINISHED), 'never finished').toBe(0)
    expect(said(STOPPED), 'the warning line is the announcement').toBe(0)
  })
})
