// @vitest-environment jsdom
//
// #452 — A PANE THAT MISSED PERIODS FILLS THEM IN. The snapshot's decode list is the current period
// only, while our own overs ride a 30-deep ring re-sent on every poll. A pane that stopped taking
// the feed in (closed, its window in the background, a pop-out out of sight) came back showing our
// report and RR73 to a caller whose calls it never drew. On its next ingest it now asks the station
// for the decodes of the periods in between and draws them as history; what the station no longer
// holds is marked. A refill is a read: it calls nothing else and raises nothing.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import type { DecodeRefill, DecodeRow } from '../types'
import { EN } from '../i18n'

// Every backend call the pane makes, by name: the refill must be the only one.
const calls: string[] = []
const refill = vi.fn<(from: number, to: number) => Promise<DecodeRefill>>()
vi.mock('../api', () => ({
  openQrzPage: vi.fn(() => calls.push('openQrzPage')),
  getDecodeRefill: (from: number, to: number) => {
    calls.push('getDecodeRefill')
    return refill(from, to)
  },
}))
// The alert, sound and announce paths: a refilled row must reach none of them.
const alerted: string[] = []
vi.mock('../alerts', async (orig) => {
  const real = await orig<typeof import('../alerts')>()
  return {
    ...real,
    processDecodes: vi.fn(() => alerted.push('processDecodes')),
    doubleBeep: vi.fn(() => alerted.push('doubleBeep')),
    txEarcon: vi.fn(() => alerted.push('txEarcon')),
  }
})
vi.mock('../announce', async (orig) => {
  const real = await orig<typeof import('../announce')>()
  return { ...real, announce: vi.fn(() => alerted.push('announce')) }
})

import { OperateDecodes } from './OperateDecodes'

const row = (message: string, over: Partial<DecodeRow> = {}): DecodeRow => ({
  from: message.split(' ')[1] ?? null,
  snr: -12,
  dtSec: 0.1,
  freqHz: 600,
  message,
  isCq: message.startsWith('CQ'),
  directedToMe: false,
  worked: false,
  tier: 'FT8',
  rv: 0,
  ...over,
})
const toUs = (message: string) => row(message, { directedToMe: true, freqHz: 1228, from: 'KF9UG' })
const mine = (message: string, txAt: number): DecodeRow =>
  row(message, { from: 'KN0BLE', mine: true, txAt, freqHz: 2601, snr: 0, rv: -1 })

// Slots in the engine's numbering; FT8, 15 s. Our overs are keyed in the even periods.
const START = 100
const ms = (slot: number) => slot * 15_000

/** The Rx Frequency pane on the CQ run's RX offset, far from the caller at 1228 Hz. */
function Rx({ decodes, slot }: { decodes: DecodeRow[]; slot: number }) {
  return (
    <OperateDecodes
      decodes={decodes}
      slot={slot}
      rxOffsetHz={2601}
      band="20m"
      tier="FT8"
      lockedFilter="rx"
      harqRescues={0}
      onCall={() => calls.push('onCall')}
    />
  )
}

const drawn = () =>
  Array.from(document.querySelectorAll('.od-scroll > *')).map((el) =>
    el.classList.contains('od-gap')
      ? `gap: ${el.textContent}`
      : el.classList.contains('decode-row')
        ? `${el.classList.contains('refilled') ? 'refilled ' : ''}${el.querySelector('.decode-msg')?.getAttribute('title')}`
        : null,
  ).filter((x) => x != null)

/** Our own overs, as the ring re-sends them on every poll. */
const ring = [
  mine('CQ KN0BLE EL98', ms(START + 1) / 1000),
  mine('KF9UG KN0BLE -16', ms(START + 3) / 1000),
  mine('KF9UG KN0BLE RR73', ms(START + 5) / 1000),
]

/** A pane that ingested slot 100, then nothing until slot 104 (three periods missed). */
async function pauseAndResume(answer: DecodeRefill) {
  refill.mockResolvedValue(answer)
  vi.setSystemTime(ms(START) + 2_000)
  const view = render(<Rx decodes={[row('CQ W4SHZ EM90', { freqHz: 2600 })]} slot={START} />)
  vi.setSystemTime(ms(START + 4) + 2_000)
  await act(async () => {
    view.rerender(<Rx decodes={[row('CQ K4ABC EM90', { freqHz: 2610 }), ...ring]} slot={START + 4} />)
  })
  return view
}

beforeEach(() => {
  localStorage.clear()
  calls.length = 0
  alerted.length = 0
  refill.mockReset()
  vi.useFakeTimers({ toFake: ['Date'] })
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('#452: a decode pane that missed periods', () => {
  it('fills in the calls to us it missed, in period order, as history', async () => {
    await pauseAndResume({
      coveredFrom: 0,
      rows: [
        { slot: START + 2, row: toUs('KN0BLE KF9UG EN71') },
        { slot: START + 4, row: toUs('KN0BLE KF9UG R-11') },
      ],
    })
    expect(refill).toHaveBeenCalledWith(START, START + 4)
    expect(drawn()).toEqual([
      'CQ W4SHZ EM90',
      'CQ KN0BLE EL98',
      'refilled KN0BLE KF9UG EN71',
      'KF9UG KN0BLE -16',
      'refilled KN0BLE KF9UG R-11',
      'CQ K4ABC EM90',
      'KF9UG KN0BLE RR73',
    ])
  })

  it('never draws a line twice: rows it already has, and our own overs, are not taken again', async () => {
    await pauseAndResume({
      coveredFrom: 0,
      rows: [
        { slot: START, row: row('CQ W4SHZ EM90', { freqHz: 2600 }) },
        { slot: START + 2, row: toUs('KN0BLE KF9UG EN71') },
        { slot: START + 4, row: row('CQ K4ABC EM90', { freqHz: 2610 }) },
        { slot: START + 4, row: ring[2] },
      ],
    })
    const lines = drawn()
    expect(lines).toEqual([
      'CQ W4SHZ EM90',
      'CQ KN0BLE EL98',
      'refilled KN0BLE KF9UG EN71',
      'KF9UG KN0BLE -16',
      'CQ K4ABC EM90',
      'KF9UG KN0BLE RR73',
    ])
    expect(new Set(lines).size).toBe(lines.length)
  })

  it('marks the periods the station no longer held', async () => {
    // The station's history is whole only from slot 103: 101 and 102 are gone.
    await pauseAndResume({
      coveredFrom: START + 3,
      rows: [{ slot: START + 4, row: toUs('KN0BLE KF9UG R-11') }],
    })
    expect(drawn()).toEqual([
      'CQ W4SHZ EM90',
      `gap: ${EN['operate.decodes.gap'].replace('{{n}}', '2')}`,
      'CQ KN0BLE EL98',
      'KF9UG KN0BLE -16',
      'refilled KN0BLE KF9UG R-11',
      'CQ K4ABC EM90',
      'KF9UG KN0BLE RR73',
    ])
  })

  it('asks nothing while it keeps up, and a refill calls nothing else and raises no alert', async () => {
    refill.mockResolvedValue({ coveredFrom: 0, rows: [] })
    vi.setSystemTime(ms(START) + 2_000)
    const view = render(<Rx decodes={[row('CQ W4SHZ EM90', { freqHz: 2600 })]} slot={START} />)
    await act(async () => {
      view.rerender(<Rx decodes={[row('CQ W4SHZ EM90', { freqHz: 2600 })]} slot={START + 1} />)
    })
    expect(refill).not.toHaveBeenCalled()
    cleanup()
    await pauseAndResume({
      coveredFrom: 0,
      rows: [{ slot: START + 2, row: toUs('KN0BLE KF9UG EN71') }],
    })
    expect(drawn()).toContain('refilled KN0BLE KF9UG EN71')
    expect(calls).toEqual(['getDecodeRefill'])
    expect(alerted).toEqual([])
  })

  it('a failed or refused read leaves the pane as it was', async () => {
    refill.mockRejectedValue('nexusClient:atShackOnly')
    vi.setSystemTime(ms(START) + 2_000)
    const view = render(<Rx decodes={[row('CQ W4SHZ EM90', { freqHz: 2600 })]} slot={START} />)
    await act(async () => {
      view.rerender(<Rx decodes={[row('CQ K4ABC EM90', { freqHz: 2610 })]} slot={START + 4} />)
    })
    expect(drawn()).toEqual(['CQ W4SHZ EM90', 'CQ K4ABC EM90'])
  })
})
