// @vitest-environment jsdom
//
// The "Activation over?" checklist: nothing ticked to begin with; Keep activating changes nothing;
// its export writes exactly the files the Logbook's per-activation export writes — the same command
// asked the same way, the same file names, the same text — one per park per UTC day of THIS
// activation; and it reminds, without a tick, of what the files cannot fix. The api is a mock:
// nothing reaches a backend or a network.
import { afterEach, beforeAll, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { t } from '../i18n'
import type { Activation, LogExport, LoggedActivation, LoggedQso } from '../types'
import * as api from '../api'
import type { LogQuestion } from '../features/logAnswers'

vi.setConfig({ testTimeout: 15_000 })

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})

const engineLog = vi.hoisted(() => vi.fn())
/** Whether the press being handled came through a Remote stream, as `isStreamInput` answers it. */
const stream = vi.hoisted(() => ({ on: false }))
vi.mock('../remote-native/stream-input', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  isStreamInput: () => stream.on,
}))
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    exportGeneralLog: vi.fn(),
    saveTextToDownloads: vi.fn(async (name: string) => `/home/op/Downloads/${name}`),
    deleteQsoById: noop(), importAdif: noop(), editQsoById: vi.fn(async () => ({})),
    logOperators: vi.fn(async () => [] as string[]), exportLogForOperator: vi.fn(),
    logActivations: vi.fn(async () => []), exportLogForActivation: vi.fn(),
    noteActivationExported: vi.fn(async () => {}),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    logQso: vi.fn(async () => ({})), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: noop(), markQslCardById: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
    selfSpot: vi.fn(),
  }
})

import { ActivationOverDialog } from './ActivationOverDialog'
import { Logbook } from './Logbook'

const DAY = 86_400
// A two-fer started at 23:30Z on 2026-10-26 and still running after 0000Z.
const START = 1_793_057_400
const DAY1 = Math.floor(START / DAY) * DAY
const DAY2 = DAY1 + DAY

const act: Activation = {
  program: 'POTA',
  reference: 'US-1234,US-5678',
  qsoCount: 4,
  states: ['US-ND', 'US-MN'],
  myState: null,
  parks: [
    { reference: 'US-1234', count: 4, p2p: 1 },
    { reference: 'US-5678', count: 4, p2p: 1 },
  ],
  grid: null,
  startedAt: START,
  utcDay: DAY2 / DAY,
}

const bucket = (reference: string, dayStartUnix: number, qsos: number, callsign = 'KD9TAW/P'): LoggedActivation => ({
  program: 'POTA',
  reference,
  dayStartUnix,
  date: new Date(dayStartUnix * 1000).toISOString().slice(0, 10),
  callsign,
  qsos,
})
/** What the log holds: this activation's four files, a park it is not, and a day before it started. */
const LOG_BUCKETS = [
  bucket('US-1234', DAY1, 3),
  bucket('US-5678', DAY1, 3),
  bucket('US-1234', DAY2, 12),
  bucket('US-5678', DAY2, 12),
  bucket('US-9999', DAY2, 15),
  bucket('US-1234', DAY1 - DAY, 11),
]
/** The station's file for a bucket: its text names the bucket, so a file written for the wrong one shows. */
const fileFor = (reference: string, dayStartUnix: number): LogExport => ({
  text: `<MY_SIG_INFO:${reference.length}>${reference} <QSO_DATE:8>${dayStartUnix} <EOR>\n`,
  saving: 0,
  held: 0,
})

beforeEach(() => {
  engineLog.mockResolvedValue([
    { call: 'K1ABC', band: '20m', freqMhz: 14.285, mode: 'SSB', whenUnix: START + 60, ota: null } as unknown as LoggedQso,
  ])
  vi.mocked(api.logActivations).mockResolvedValue(LOG_BUCKETS)
  vi.mocked(api.exportLogForActivation).mockImplementation(async (reference: string, dayStartUnix: number) =>
    fileFor(reference, dayStartUnix),
  )
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  stream.on = false
})

function checklist(over: Partial<Parameters<typeof ActivationOverDialog>[0]> = {}) {
  const onEnd = vi.fn(async () => {})
  const onKeep = vi.fn()
  render(<ActivationOverDialog act={act} canQrt={false} onKeep={onKeep} onEnd={onEnd} {...over} />)
  return { onEnd, onKeep }
}

it('starts with nothing ticked; Keep activating does nothing at all', async () => {
  const { onEnd, onKeep } = checklist({ canQrt: true })
  const exportTick = (await screen.findByRole('checkbox', { name: t('activationOver.export.label', { count: 4 }) })) as HTMLInputElement
  const qrt = screen.getByRole('checkbox', { name: t('ota.selfSpot.qrt.label') }) as HTMLInputElement
  expect([exportTick.checked, qrt.checked]).toEqual([false, false])
  fireEvent.click(screen.getByRole('button', { name: t('activationOver.keep') }))
  expect(onKeep).toHaveBeenCalledTimes(1)
  expect(onEnd).not.toHaveBeenCalled()
  expect(api.exportLogForActivation).not.toHaveBeenCalled()
  expect(api.saveTextToDownloads).not.toHaveBeenCalled()
  expect(api.selfSpot).not.toHaveBeenCalled()
})

it('End with nothing ticked ends the activation and writes and posts nothing', async () => {
  const { onEnd } = checklist({ canQrt: true })
  await screen.findByRole('checkbox', { name: t('activationOver.export.label', { count: 4 }) })
  fireEvent.click(screen.getByRole('button', { name: t('activationOver.end') }))
  await waitFor(() => expect(onEnd).toHaveBeenCalledWith({ qrt: false }))
  expect(api.exportLogForActivation).not.toHaveBeenCalled()
  expect(api.saveTextToDownloads).not.toHaveBeenCalled()
  expect(api.selfSpot).not.toHaveBeenCalled()
})

it('no QRT is offered until a spot from here stands', async () => {
  checklist({ canQrt: false })
  await screen.findByRole('checkbox', { name: t('activationOver.export.label', { count: 4 }) })
  expect(screen.queryByRole('checkbox', { name: t('ota.selfSpot.qrt.label') })).toBeNull()
})

it('exports the same files the Logbook writes: one per park per UTC day of this activation, then ends', async () => {
  // The Logbook's own export, file by file, for this activation's four buckets.
  render(<Logbook defaultBand="20m" defaultFreqMhz={14.285} defaultMode="SSB" />)
  const ours = LOG_BUCKETS.slice(0, 4)
  for (const b of ours) {
    const option = await waitFor(() => {
      const o = screen
        .getAllByRole('option')
        .find((x) => x.textContent?.includes(b.reference) && x.textContent?.includes(b.date))
      expect(o).toBeTruthy()
      return o as HTMLOptionElement
    })
    fireEvent.change(option.closest('select')!, { target: { value: option.value } })
    const before = vi.mocked(api.saveTextToDownloads).mock.calls.length
    fireEvent.click(screen.getByRole('button', { name: t('logbook.export.activation.button') }))
    await waitFor(() => expect(vi.mocked(api.saveTextToDownloads).mock.calls.length).toBe(before + 1))
  }
  const logbook = {
    asked: vi.mocked(api.exportLogForActivation).mock.calls.slice(),
    saved: vi.mocked(api.saveTextToDownloads).mock.calls.slice(),
  }
  cleanup()
  vi.mocked(api.exportLogForActivation).mockClear()
  vi.mocked(api.saveTextToDownloads).mockClear()

  const { onEnd } = checklist()
  fireEvent.click(await screen.findByRole('checkbox', { name: t('activationOver.export.label', { count: 4 }) }))
  fireEvent.click(screen.getByRole('button', { name: t('activationOver.end') }))
  await waitFor(() => expect(onEnd).toHaveBeenCalledWith({ qrt: false }))
  const checklistCalls = {
    asked: vi.mocked(api.exportLogForActivation).mock.calls.slice(),
    saved: vi.mocked(api.saveTextToDownloads).mock.calls.slice(),
  }
  // The same command asked the same four ways, and the same four files, name and text.
  expect(checklistCalls.asked).toEqual(logbook.asked)
  expect(checklistCalls.saved).toEqual(logbook.saved)
  expect(checklistCalls.saved.map(([name]) => name)).toEqual([
    `KD9TAW-P@US-1234-20261026.adi`,
    `KD9TAW-P@US-5678-20261026.adi`,
    `KD9TAW-P@US-1234-20261027.adi`,
    `KD9TAW-P@US-5678-20261027.adi`,
  ])
  // The files are written before the activation ends.
  const lastSave = Math.max(...vi.mocked(api.saveTextToDownloads).mock.invocationCallOrder)
  expect(lastSave).toBeLessThan(onEnd.mock.invocationCallOrder[0])
})

it('a file that fails stops the rest, says why and leaves the activation running', async () => {
  vi.mocked(api.exportLogForActivation).mockRejectedValueOnce('disk full')
  const { onEnd } = checklist()
  fireEvent.click(await screen.findByRole('checkbox', { name: t('activationOver.export.label', { count: 4 }) }))
  fireEvent.click(screen.getByRole('button', { name: t('activationOver.end') }))
  expect((await screen.findByRole('alert')).textContent).toBe('disk full')
  expect(api.saveTextToDownloads).not.toHaveBeenCalled()
  expect(onEnd).not.toHaveBeenCalled()
})

it('reminds of the K- prefix, a missing grid and an unpicked state, and that the upload is the operator’s', async () => {
  checklist({ act: { ...act, reference: 'K-1234', parks: [{ reference: 'K-1234', count: 4, p2p: 0 }] } })
  await screen.findByText(t('ota.activation.legacyRef', { reference: 'K-1234' }))
  expect(screen.getByText(t('activationOver.note.noGrid'))).toBeTruthy()
  expect(screen.getByText(t('activationOver.note.noState'))).toBeTruthy()
  expect(screen.getByText(t('activationOver.note.upload'))).toBeTruthy()
  cleanup()
  checklist({ act: { ...act, grid: 'EN52', myState: 'ND' } })
  await screen.findByText(t('activationOver.note.upload'))
  expect(screen.queryByText(t('activationOver.note.noGrid'))).toBeNull()
  expect(screen.queryByText(t('activationOver.note.noState'))).toBeNull()
})

it('through the Remote stream, a ticked export is refused: no file, no record, and the activation keeps running', async () => {
  const { onEnd } = checklist()
  fireEvent.click(await screen.findByRole('checkbox', { name: t('activationOver.export.label', { count: 4 }) }))
  stream.on = true
  fireEvent.click(screen.getByRole('button', { name: t('activationOver.end') }))
  expect((await screen.findByRole('alert')).textContent).toBe(t('activationOver.export.remote'))
  expect(api.exportLogForActivation).not.toHaveBeenCalled()
  expect(api.saveTextToDownloads).not.toHaveBeenCalled()
  expect(api.noteActivationExported).not.toHaveBeenCalled()
  expect(onEnd).not.toHaveBeenCalled()
})

it('records each file as exported only once it is saved', async () => {
  const { onEnd } = checklist()
  fireEvent.click(await screen.findByRole('checkbox', { name: t('activationOver.export.label', { count: 4 }) }))
  fireEvent.click(screen.getByRole('button', { name: t('activationOver.end') }))
  await waitFor(() => expect(onEnd).toHaveBeenCalled())
  expect(vi.mocked(api.noteActivationExported).mock.calls).toEqual([
    ['US-1234', DAY1, 'KD9TAW/P'],
    ['US-5678', DAY1, 'KD9TAW/P'],
    ['US-1234', DAY2, 'KD9TAW/P'],
    ['US-5678', DAY2, 'KD9TAW/P'],
  ])
  const saved = vi.mocked(api.saveTextToDownloads).mock.invocationCallOrder
  const noted = vi.mocked(api.noteActivationExported).mock.invocationCallOrder
  noted.forEach((n, i) => expect(n).toBeGreaterThan(saved[i]))
})

it('a file that fails to save is not recorded as exported, and stops the end', async () => {
  vi.mocked(api.saveTextToDownloads).mockRejectedValueOnce('Downloads is not writable')
  const { onEnd } = checklist()
  fireEvent.click(await screen.findByRole('checkbox', { name: t('activationOver.export.label', { count: 4 }) }))
  fireEvent.click(screen.getByRole('button', { name: t('activationOver.end') }))
  expect((await screen.findByRole('alert')).textContent).toBe('Downloads is not writable')
  expect(api.exportLogForActivation).toHaveBeenCalledTimes(1)
  expect(api.noteActivationExported).not.toHaveBeenCalled()
  expect(onEnd).not.toHaveBeenCalled()
})

it('says when the activation\u2019s files could not be read, rather than that there are none', async () => {
  vi.mocked(api.logActivations).mockRejectedValueOnce('the log is busy')
  checklist()
  await screen.findByText(t('activationOver.export.unreadable'))
  expect(screen.queryByText(t('activationOver.export.none'))).toBeNull()
  const tick = screen.getByRole('checkbox') as HTMLInputElement
  expect(tick.disabled).toBe(true)
  // Positive control: an activation with no files says none.
  cleanup()
  vi.mocked(api.logActivations).mockResolvedValueOnce([])
  checklist()
  await screen.findByText(t('activationOver.export.none'))
  expect(screen.queryByText(t('activationOver.export.unreadable'))).toBeNull()
})

it('a park’s day under two calls: says POTA counts each call separately, and lists each call’s file with its count', async () => {
  const mixed: Activation = {
    ...act,
    reference: 'US-1234',
    parks: [
      {
        reference: 'US-1234',
        count: 10,
        p2p: 0,
        calls: [
          { callsign: 'KD9TAW', count: 6 },
          { callsign: 'KD9TAW/M', count: 4 },
        ],
      },
    ],
  }
  vi.mocked(api.logActivations).mockResolvedValue([bucket('US-1234', DAY2, 6, 'KD9TAW'), bucket('US-1234', DAY2, 4, 'KD9TAW/M')])
  checklist({ act: mixed })
  await screen.findByText('US-1234: contacts under 2 calls (KD9TAW 6, KD9TAW/M 4) — POTA counts each call separately')
  const files = [...document.querySelectorAll('.activation-over-files li')].map((li) => li.textContent)
  expect(files).toEqual([
    `KD9TAW@US-1234-20261027.adi — ${t('activationOver.export.file', { count: 6 })}${t('logbook.export.activation.short', { min: 10 })}`,
    `KD9TAW-M@US-1234-20261027.adi — ${t('activationOver.export.file', { count: 4 })}${t('logbook.export.activation.short', { min: 10 })}`,
  ])
})

it('reads the files again when the UTC day turns while it is open, so the new day’s file is listed', async () => {
  vi.mocked(api.logActivations).mockResolvedValue(LOG_BUCKETS.slice(0, 2))
  const onEnd = vi.fn(async () => {})
  const { rerender } = render(<ActivationOverDialog act={{ ...act, utcDay: DAY1 / DAY }} canQrt={false} onKeep={vi.fn()} onEnd={onEnd} />)
  await screen.findByRole('checkbox', { name: t('activationOver.export.label', { count: 2 }) })
  vi.mocked(api.logActivations).mockResolvedValue(LOG_BUCKETS)
  rerender(<ActivationOverDialog act={{ ...act, utcDay: DAY2 / DAY }} canQrt={false} onKeep={vi.fn()} onEnd={onEnd} />)
  await screen.findByRole('checkbox', { name: t('activationOver.export.label', { count: 4 }) })
})
