// @vitest-environment jsdom
//
// AN IOTA OFFER TAKEN WITH USE IS FOR ONE CONTACT. The operator takes the chip for the contact in
// front of them; once that contact is logged, the next one starts with nothing taken, even when it
// is the same station again (an island DXpedition worked on a second band). Pinned on both forms
// that keep their offer across contacts: the log strip and the Logbook's Add form. The
// confirm-before-log popup is a new form per contact, so it starts over by itself.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor, within } from '@testing-library/react'
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
    consent: true,
    count: 1,
    fetchedAt: 0,
    lastError: null,
  })
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  resetIotaGroupsForTest()
})

const chip = () => screen.queryByRole('group', { name: t('iota.offer.aria', { ref: 'EU-028' }) })
const use = () => within(chip()!).getByRole('button', { name: t('iota.offer.use') })

describe('a Use press is for one contact', () => {
  it('in the log strip: the same call next time starts with nothing taken', async () => {
    const snap = { mycall: 'KD9TAW', mygrid: 'EN52', radio: { band: '20m', dialMhz: 14.025 }, hunt: null } as unknown as AppSnapshot
    render(<LogEntry snap={snap} mode="CW" defaultRst="599" exchange="terrestrial" titled={false} />)
    const call = () => document.querySelector<HTMLInputElement>('input.le-call')!
    fireEvent.change(call(), { target: { value: 'IA5X' } })
    await waitFor(() => expect(chip()).toBeTruthy())
    fireEvent.click(use())
    expect(use().getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(screen.getByRole('button', { name: /^log$/i }))
    await waitFor(() => expect(logQso).toHaveBeenCalledTimes(1))
    expect(vi.mocked(logQso).mock.calls[0][2], 'premise: logged with the offer').toEqual(OFFER)
    await waitFor(() => expect(call().value).toBe(''))

    fireEvent.change(call(), { target: { value: 'IA5X' } })
    await waitFor(() => expect(vi.mocked(getIotaOffer).mock.calls.length).toBeGreaterThan(1))
    await waitFor(() => expect(chip()).toBeTruthy())
    expect(use().getAttribute('aria-pressed'), 'taken for the last contact, not this one').toBe('false')
    fireEvent.click(screen.getByRole('button', { name: /^log$/i }))
    await waitFor(() => expect(logQso).toHaveBeenCalledTimes(2))
    expect(vi.mocked(logQso).mock.calls[1][2]).toBeNull()
  })

  it("in the Logbook's Add form: the same call next time starts with nothing taken", async () => {
    render(<Logbook defaultBand="20m" defaultFreqMhz={14.025} defaultMode="CW" />)
    const openForm = () => fireEvent.click(screen.getByRole('button', { name: t('logbook.form.open') }))
    const form = () => document.querySelector<HTMLElement>('.logbook-form')!
    const call = () => within(form()).getByLabelText(t('logbook.field.call.label'), { exact: true, selector: 'input' })
    openForm()
    await waitFor(() => expect(document.querySelector('.logbook-form')).not.toBeNull())
    fireEvent.change(call(), { target: { value: 'IA5X' } })
    await waitFor(() => expect(chip()).toBeTruthy())
    fireEvent.click(use())
    expect(use().getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(within(form()).getByRole('button', { name: t('logbook.form.log') }))
    await waitFor(() => expect(logQso).toHaveBeenCalledTimes(1))
    expect(vi.mocked(logQso).mock.calls[0][2], 'premise: logged with the offer').toEqual(OFFER)
    await waitFor(() => expect(document.querySelector('.logbook-form')).toBeNull())

    openForm()
    await waitFor(() => expect(document.querySelector('.logbook-form')).not.toBeNull())
    fireEvent.change(call(), { target: { value: 'IA5X' } })
    await waitFor(() => expect(vi.mocked(getIotaOffer).mock.calls.length).toBeGreaterThan(1))
    await waitFor(() => expect(chip()).toBeTruthy())
    expect(use().getAttribute('aria-pressed'), 'taken for the last contact, not this one').toBe('false')
  })
})
