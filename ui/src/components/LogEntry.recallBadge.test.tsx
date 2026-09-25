// @vitest-environment jsdom
//
// NO NEED BADGE UNTIL THE CALL'S ANSWER ARRIVES — the log strip's card (LogEntry → RecallPanel).
// (The Operate cockpit's card, the other host: OperateCockpit.recall.test.tsx.)
//
// Until the window had the log's answer for the typed call, the strip fell back to the EMPTY log's
// answer, and in an empty log every entity is new: "New DXCC!" over a station whose country is
// already in the log, for as long as the log took to load — and, once the engine answers each call
// (C17a), on every call typed. Now the card shows no need badge until the answer lands, then exactly
// the badge it always showed.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { getLogDelta } from '../api'
import { t } from '../i18n'
import { setLogSource } from '../features/logSource'
import { createAskingLogSource } from '../features/askingLogSource'
import { answerFrom } from '../features/logAnswers'
import type { AppSnapshot, LoggedQso } from '../types'

// W1ABC's country is in the log, on 40 m; the rig is on 20 m — so the card's badge, once the log
// has answered, is the new BAND slot (a derivation, not a flag).
const priorQsos = [
  {
    id: 'id-1',
    call: 'W1ABC',
    country: 'United States',
    grid: 'FN31',
    band: '40m',
    freqMhz: 7.2,
    mode: 'SSB',
    rstSent: '59',
    rstRcvd: '57',
    whenUnix: Date.UTC(2026, 2, 14) / 1000,
    confirmed: false,
    awardConfirmed: false,
  },
] as unknown as LoggedQso[]

vi.mock('../api', async (importOriginal) => {
  // Every export stubbed (a hand-kept list throws on mount at the first new call — see
  // CockpitRecall.test.tsx); only what the strip's card reads is given a shape.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  return {
    ...auto,
    getLogDelta: vi.fn(),
    getLog: vi.fn(async () => []),
    qrzLookup: vi.fn(async () => null),
    resolveEntity: vi.fn(async () => 'United States'),
    lookupPark: vi.fn(async () => null),
    lookupParkLive: vi.fn(async () => null),
    searchParks: vi.fn(async () => []),
  }
})

const snap = { radio: { band: '20m', dialMhz: 14.2 }, hunt: null, logTick: 1 } as unknown as AppSnapshot
const need = () => document.querySelector('.recall-card .recall-badge.need')?.textContent ?? null
const TODAYS = `★ ${t('recall.need.band')}`

function typeCall(call: string) {
  render(<LogEntry snap={snap} mode="PH" defaultRst="59" exchange="terrestrial" fieldDay={null} fdMode={undefined} />)
  fireEvent.change(screen.getByPlaceholderText(t('logEntry.call.placeholder')), { target: { value: call } })
}

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  vi.mocked(getLogDelta).mockReset()
})

describe('the log strip’s card: no need badge until the call’s answer arrives', () => {
  it('the whole-log list: nothing while the log loads, then exactly today’s badge', async () => {
    let release = () => {}
    vi.mocked(getLogDelta).mockImplementation(
      () => new Promise((resolve) => (release = () => resolve({ revision: 1, full: true, rows: priorQsos }))) as never,
    )
    typeCall('W1ABC')
    await waitFor(() => expect(document.querySelector('.recall-card')).not.toBeNull())
    await act(async () => {}) // the entity resolves (cty.dat, local)
    expect(need(), 'a need badge before the log answered').toBeNull()

    await act(async () => release())
    await waitFor(() => expect(need(), 'the answer, landed').toBe(TODAYS))
  })

  it('an asking source: nothing while this call’s answer is out, then exactly today’s badge', async () => {
    const out: (() => void)[] = []
    setLogSource(
      createAskingLogSource(async (q) => {
        await new Promise<void>((resolve) => out.push(resolve))
        return answerFrom(priorQsos, q, 1)
      }),
    )
    typeCall('W1ABC')
    await waitFor(() => expect(document.querySelector('.recall-card')).not.toBeNull())
    await waitFor(() => expect(out.length, 'the card asked').toBeGreaterThan(0))
    expect(need(), 'a need badge before the answer').toBeNull()

    await act(async () => {
      for (const answer of out.splice(0)) answer()
    })
    await waitFor(() => expect(need(), 'the answer, landed').toBe(TODAYS))
  })
})
