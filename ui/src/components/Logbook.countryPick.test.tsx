// @vitest-environment jsdom
//
// SETTING A CONTACT'S COUNTRY BY HAND. On the station the Logbook's Country cell opens a search over
// every DXCC entity the station knows — the deleted ones too, an old log's — and the one picked is
// sent as the contact's DXCC number, addressed by the row's id and edit key. The way back is the
// picker's own Clear entry, offered only while there is a number to clear; LoTW's number, where the
// log keeps it, is named, because it counts over the one set here.
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { Logbook } from './Logbook'
import * as api from '../api'
import * as toast from '../toast'
import type { LogQuestion } from '../features/logAnswers'
import type { DxccEntityOption, LoggedQso } from '../types'

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
afterEach(cleanup)

const ENTITIES: DxccEntityOption[] = [
  { code: 182, name: 'Navassa Island', deleted: false },
  { code: 229, name: 'GERMAN DEMOCRATIC REPUBLIC', deleted: true },
  { code: 230, name: 'Fed. Rep. of Germany', deleted: false },
  { code: 291, name: 'United States', deleted: false },
]

/** The log the engine holds: `askLog` answers from it as the engine does (features/logAnswers.testkit). */
const engineLog = vi.hoisted(() => vi.fn())
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    deleteQsoById: noop(), editQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: vi.fn(() => Promise.resolve({})),
    markQslCardById: vi.fn(() => Promise.resolve({})),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
    getDxccEntities: vi.fn(),
    setDxccById: vi.fn(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

/** The 2015 K1N contact as the station sends it: `dxcc` and `extra` as its record holds them. */
function k1n(dxcc: number | null, entity: string, extra: [string, string][] = []) {
  return {
    id: 'id-K1N', call: 'K1N', grid: null, band: '20m', freqMhz: 14.024, mode: 'CW',
    rstSent: '599', rstRcvd: '599', name: null, qth: null, comment: null, notes: null,
    country: 'United States', entity, dxcc, extra, whenUnix: 1_423_571_640,
    confirmed: false, awardConfirmed: false,
    qslRcvd: null, qslSent: null, ota: null, upload: undefined,
  }
}

async function mount(row: ReturnType<typeof k1n>) {
  engineLog.mockResolvedValue([row])
  vi.mocked(api.getDxccEntities).mockResolvedValue(ENTITIES)
  vi.mocked(api.setDxccById).mockImplementation(async (target, dxcc) => ({
    kind: 'applied',
    current: {
      row: {
        ...row,
        dxcc,
        entity: ENTITIES.find((e) => e.code === dxcc)?.name ?? 'United States',
      } as unknown as LoggedQso,
      editKey: `${target.editKey}+`,
    },
  }))
  render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />)
  const cell = await screen.findByRole('button', { name: /^Country of K1N/ })
  return cell
}

/** Open the picker from the keyboard and wait for the station's entities. */
async function open(cell: HTMLElement) {
  fireEvent.keyDown(cell, { key: 'Enter' })
  const menu = await screen.findByRole('menu')
  await waitFor(() => expect(within(menu).getAllByRole('menuitemradio').length).toBeGreaterThan(0))
  return menu
}

describe("the Logbook's Country cell sets the DXCC entity by hand", () => {
  it('finds a deleted entity by name and sends its number for the row it was opened on', async () => {
    vi.mocked(api.setDxccById).mockClear()
    const cell = await mount(k1n(null, 'United States'))
    expect(cell.textContent).toBe('United States')
    const menu = await open(cell)
    const search = within(menu).getByRole('textbox')
    // Typing is the way in: the search has the keyboard as the list opens.
    expect(document.activeElement).toBe(search)
    // The contact has no number of its own: the picker says so.
    expect(menu.textContent).toContain('No DXCC number on record: it counts by its call.')

    fireEvent.change(search, { target: { value: 'german dem' } })
    const found = within(menu).getAllByRole('menuitemradio')
    expect(found.map((e) => e.textContent)).toEqual(['GERMAN DEMOCRATIC REPUBLIC (229) · deleted'])
    fireEvent.click(found[0])

    await waitFor(() => expect(api.setDxccById).toHaveBeenCalledTimes(1))
    expect(api.setDxccById).toHaveBeenCalledWith({ id: 'id-K1N', editKey: 'ek:id-K1N' }, 229)
    await waitFor(() =>
      expect(toast.pushToast).toHaveBeenCalledWith('K1N set to GERMAN DEMOCRATIC REPUBLIC', 'success'),
    )
  })

  it('offers Clear only while the contact carries a number, and a clear sends null', async () => {
    vi.mocked(api.setDxccById).mockClear()
    // No number: nothing to clear.
    let menu = await open(await mount(k1n(null, 'United States')))
    expect(within(menu).queryByRole('menuitem', { name: 'Clear the DXCC number' })).toBeNull()
    cleanup()

    menu = await open(await mount(k1n(182, 'Navassa Island')))
    expect(menu.textContent).toContain('DXCC on record: Navassa Island (182)')
    // The number on record is the checked entity.
    const checked = within(menu)
      .getAllByRole('menuitemradio')
      .filter((e) => e.getAttribute('aria-checked') === 'true')
    expect(checked.map((e) => e.textContent)).toEqual(['Navassa Island (182)'])
    fireEvent.click(within(menu).getByRole('menuitem', { name: 'Clear the DXCC number' }))
    await waitFor(() => expect(api.setDxccById).toHaveBeenCalledWith({ id: 'id-K1N', editKey: 'ek:id-K1N' }, null))
    await waitFor(() =>
      expect(toast.pushToast).toHaveBeenCalledWith('K1N: DXCC number cleared, it counts by its call', 'success'),
    )
  })

  it("names LoTW's number where the log keeps one beside its own", async () => {
    const menu = await open(await mount(k1n(291, 'United States', [['APP_NEXUS_LOTW_DXCC', '182']])))
    expect(menu.textContent).toContain('LoTW confirmed it as Navassa Island (182), and the awards count LoTW’s.')
    cleanup()
    // Control: none kept, none named.
    const plain = await open(await mount(k1n(291, 'United States')))
    expect(plain.textContent).not.toContain('LoTW confirmed')
  })

  // A React portal bubbles its events through the tree: a double-click in the picker would reach the
  // row's own double-click, which opens the contact's detail view behind the menu. And the picker is
  // not modal: a modal menu takes the page's pointer events, so the second click of a double-click
  // on the country itself landed on nothing, and the row never opened the contact (measured in
  // Chromium; jsdom sees only the pointer-events lock that caused it).
  it("keeps a double-click inside the picker away from the row's contact card, and leaves the page's clicks alone", async () => {
    const cell = await mount(k1n(null, 'United States'))
    const menu = await open(cell)
    expect(document.body.style.pointerEvents).not.toBe('none')
    // Each double-click is checked as it lands: an open contact card hides the menu from the
    // accessibility tree, so a later lookup would fail first and name the wrong cause.
    fireEvent.doubleClick(within(menu).getByRole('textbox'))
    expect(document.querySelector('.contact-card-dialog')).toBeNull()
    fireEvent.doubleClick(within(menu).getAllByRole('menuitemradio')[0])
    expect(document.querySelector('.contact-card-dialog')).toBeNull()
    fireEvent.keyDown(within(menu).getByRole('textbox'), { key: 'Escape' })
    await waitFor(() => expect(screen.queryByRole('menu')).toBeNull())
    // Control: the row's double-click does open it, so the absence above is a real one.
    const row = cell.closest('.logbook-row') as HTMLElement
    fireEvent.doubleClick(row)
    await waitFor(() => expect(document.querySelector('.contact-card-dialog')).not.toBeNull())
  })
})
