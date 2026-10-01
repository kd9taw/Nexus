// @vitest-environment jsdom
//
// ⛔ A PICTURE REFUSED WHILE IT WAITS IS DROPPED, WITH A NOTICE (the operator, 2026-09-30: "a
// refused picture or beacon is dropped with a notice, never sent later"). The engine drops a
// picture that was still waiting for the transmitter when TX went off or the dial left the
// licence privileges, and says why in `txNotice`. These pin what the SSTV cockpit does with
// that: the warning line beside Send, gone while a picture is on the air, and no "transmit
// finished" for a picture that never went out.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, act, cleanup } from '@testing-library/react'
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

/** A picture waiting for the transmitter: queued, not yet on the air. */
const QUEUED: SstvState = { ...IDLE, sending: true, txMode: 'Scottie 1', txTotalSecs: 110 }

/** The engine's own sentence, as `SSTV_REFUSED_PRIVILEGES` words it. */
const NOTICE =
  'SSTV not sent: this frequency is outside your license privileges, so the picture that was ' +
  'waiting was dropped, not held for later. Send it again from inside them.'

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

function finishedAnnouncements(): number {
  return announce.mock.calls.filter(([text]) => text === FINISHED).length
}

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true })
  getSstvState.mockReset().mockResolvedValue(IDLE)
  sstvAutoArm.mockReset().mockResolvedValue(IDLE)
  getLicensedBandPlan.mockReset().mockResolvedValue([])
  announce.mockReset()
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('SstvView: a picture the engine dropped', () => {
  it('says so in the transmit dock, beside Send', async () => {
    getSstvState.mockResolvedValue({ ...IDLE, txNotice: NOTICE })
    render(<SstvView snap={snap} />)
    await settle()
    const line = screen.getByRole('alert')
    expect(line.textContent).toContain(NOTICE)
    expect(line.closest('.sstv-tx-bar')).not.toBeNull()
  })

  it('shows no warning line while nothing was dropped, and none while a picture is on the air', async () => {
    render(<SstvView snap={snap} />)
    await settle()
    expect(screen.queryByRole('alert')).toBeNull()
    // A notice from an earlier drop, with the next picture queued: the progress speaks, not it.
    await poll({ ...QUEUED, txNotice: NOTICE })
    expect(screen.getByRole('progressbar')).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('is never announced as a finished transmission, and a picture that went out still is', async () => {
    render(<SstvView snap={snap} />)
    await settle()
    await poll(QUEUED)
    // Dropped: the queued picture is gone and the engine says why.
    await poll({ ...IDLE, txNotice: NOTICE })
    expect(screen.getByRole('alert').textContent).toContain(NOTICE)
    expect(finishedAnnouncements()).toBe(0)
    // Control: a picture that keys and completes is still announced as finished. It completes
    // as the engine leaves one that played out: its mode, and its whole key-down elapsed (an
    // IDLE answer here is how a picture stopped part-way reads, which is "stopped").
    await poll(QUEUED)
    await poll({ ...IDLE, txMode: 'Scottie 1', txProgress: 1, txElapsedSecs: 110, txTotalSecs: 110 })
    expect(finishedAnnouncements()).toBe(1)
  })
})
