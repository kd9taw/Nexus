// @vitest-environment jsdom
//
// AN IOTA BOX IS FILLED FOR ONE CONTACT. The strip and the Logbook's Add form fill their IOTA box
// with the reference the station names, and log what the box holds. Once a contact is logged, or let
// go of without logging (Clear, or set aside by a band change), the next one starts over: filled
// again, even when the operator cleared or changed the box for the last one, and even for the same
// station (an island DXpedition worked on a second band). The confirm-before-log popup is a new form
// per contact, so it starts over by itself.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor, within, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { Logbook } from './Logbook'
import { getIotaOffer, logQso } from '../api'
import { resetIotaGroupsForTest } from '../features/iotaGroups'
import { t } from '../i18n'
import type { AppSnapshot } from '../types'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return {
    ...auto,
    logOperators: vi.fn(async () => []),
    logActivations: vi.fn(async () => []),
    lotwSatNames: vi.fn(async () => []),
    eqslSentMarkReview: vi.fn(async () => []),
    searchParks: vi.fn(async () => []),
    askLog: vi.fn(() => new Promise(() => {})),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action().catch(() => null)),
}))

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})

const OFFER = { reference: 'EU-028', source: 'spot' as const }

beforeEach(() => {
  vi.mocked(getIotaOffer).mockResolvedValue(OFFER as never)
  vi.mocked(logQso).mockImplementation(async (rec: unknown) => (rec ?? {}) as never)
  resetIotaGroupsForTest([{ reference: 'EU-028', name: 'Example Isles group' }], {
    enabled: true,
    count: 1,
    fetchedAt: 0,
    bundled: true,
    lastError: null,
  })
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  resetIotaGroupsForTest()
})

const box = () => screen.getByTitle(t('iota.prefill.hint')) as HTMLInputElement
const filled = async () => waitFor(() => expect(box().value).toBe('EU-028'))
const clearBox = () => fireEvent.click(screen.getByRole('button', { name: t('iota.offer.dismiss') }))
const loggedIota = (i: number) => (vi.mocked(logQso).mock.calls[i][0] as { ota?: { iota?: string | null } }).ota?.iota ?? null

describe('a fill is for one contact', () => {
  it('in the log strip: logged with the filled reference, and the same call next time is filled again', async () => {
    const snap = { mycall: 'KD9TAW', mygrid: 'EN52', radio: { band: '20m', dialMhz: 14.025 }, hunt: null } as unknown as AppSnapshot
    render(<LogEntry snap={snap} mode="CW" defaultRst="599" exchange="terrestrial" titled={false} />)
    const call = () => document.querySelector<HTMLInputElement>('input.le-call')!
    fireEvent.change(call(), { target: { value: 'IA5X' } })
    await filled()
    fireEvent.click(screen.getByRole('button', { name: /^log$/i }))
    await waitFor(() => expect(logQso).toHaveBeenCalledTimes(1))
    expect(loggedIota(0), 'logged with no press').toBe('EU-028')
    expect(vi.mocked(logQso).mock.calls[0][2], 'marked as the spot’s').toEqual(OFFER)
    await waitFor(() => expect(call().value).toBe(''))

    fireEvent.change(call(), { target: { value: 'IA5X' } })
    await waitFor(() => expect(vi.mocked(getIotaOffer).mock.calls.length).toBeGreaterThan(1))
    await filled()
    clearBox()
    fireEvent.click(screen.getByRole('button', { name: /^log$/i }))
    await waitFor(() => expect(logQso).toHaveBeenCalledTimes(2))
    expect(loggedIota(1), 'cleared: logged with none').toBeNull()
    expect(vi.mocked(logQso).mock.calls[1][2]).toBeNull()
  })

  it("in the Logbook's Add form: filled, logged, and filled again for the next contact", async () => {
    render(<Logbook defaultBand="20m" defaultFreqMhz={14.025} defaultMode="CW" />)
    const openForm = () => fireEvent.click(screen.getByRole('button', { name: t('logbook.form.open') }))
    const form = () => document.querySelector<HTMLElement>('.logbook-form')!
    const call = () => within(form()).getByLabelText(t('logbook.field.call.label'), { exact: true, selector: 'input' })
    const iotaBox = () => within(form()).getByTitle(t('logbook.field.iota.title')) as HTMLInputElement
    openForm()
    await waitFor(() => expect(document.querySelector('.logbook-form')).not.toBeNull())
    fireEvent.change(call(), { target: { value: 'IA5X' } })
    await waitFor(() => expect(iotaBox().value).toBe('EU-028'))
    expect(within(form()).getByText(t('iota.offer.fromSpot'))).toBeTruthy()
    fireEvent.click(within(form()).getByRole('button', { name: t('logbook.form.log') }))
    await waitFor(() => expect(logQso).toHaveBeenCalledTimes(1))
    expect(loggedIota(0)).toBe('EU-028')
    expect(vi.mocked(logQso).mock.calls[0][2], 'marked as the spot’s').toEqual(OFFER)
    await waitFor(() => expect(document.querySelector('.logbook-form')).toBeNull())

    openForm()
    await waitFor(() => expect(document.querySelector('.logbook-form')).not.toBeNull())
    fireEvent.change(call(), { target: { value: 'IA5X' } })
    await waitFor(() => expect(vi.mocked(getIotaOffer).mock.calls.length).toBeGreaterThan(1))
    await waitFor(() => expect(iotaBox().value).toBe('EU-028'))
    fireEvent.change(iotaBox(), { target: { value: 'EU-001' } })
    expect(within(form()).queryByText(t('iota.offer.fromSpot')), 'changed: the operator’s').toBeNull()
  })
})

describe('a contact let go of without logging starts the next one over', () => {
  const at = (tick: number, band: string, dialMhz: number) =>
    ({ mycall: 'KD9TAW', mygrid: 'EN52', radio: { band, dialMhz }, hunt: null, bandChangeTick: tick }) as unknown as AppSnapshot
  const strip = (tick: number, band = '20m', dialMhz = 14.025) => (
    <LogEntry snap={at(tick, band, dialMhz)} mode="CW" defaultRst="599" exchange="terrestrial" titled={false} clearCallOnBandChange />
  )
  const call = () => document.querySelector<HTMLInputElement>('input.le-call')!
  /** IA5X typed, the box filled, and the operator clears it. */
  const takeOut = async () => {
    fireEvent.change(call(), { target: { value: 'IA5X' } })
    await filled()
    clearBox()
    expect(box().value).toBe('')
  }
  /** IA5X typed again: filled again, the last contact's clear forgotten. */
  const retype = async () => {
    const asked = vi.mocked(getIotaOffer).mock.calls.length
    fireEvent.change(call(), { target: { value: 'IA5X' } })
    await waitFor(() => expect(vi.mocked(getIotaOffer).mock.calls.length).toBeGreaterThan(asked))
    await filled()
  }

  it('Clear', async () => {
    render(strip(1))
    await takeOut()
    fireEvent.click(screen.getByRole('button', { name: t('logEntry.clear.label') }))
    await waitFor(() => expect(call().value).toBe(''))
    await retype()
    expect(logQso).not.toHaveBeenCalled()
  })

  it('a band change that sets the line aside', async () => {
    const view = render(strip(1))
    await takeOut()
    view.rerender(strip(2, '40m', 7.025))
    await act(async () => {
      for (let i = 0; i < 8; i++) await Promise.resolve()
    })
    expect(call().value, 'premise: the line was set aside').toBe('')
    await retype()
    expect(logQso).not.toHaveBeenCalled()
  })
})
