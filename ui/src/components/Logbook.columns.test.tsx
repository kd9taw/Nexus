// @vitest-environment jsdom
//
// The Logbook's columns: what the log stores and the table never showed (their grid, IOTA, county,
// zones, continent, distance, the QSL dates, each service's upload state, the contest exchange),
// a chooser that shows and hides them, a drag or keyboard resize, and the choice handed to the
// one writer that saves it in settings.json — clamped against the window when it is read back.
//
// The contact below is shaped as the engine's page answer carries a stored contest contact
// (`log_queries` → `LoggedQso`): the tags the store keeps verbatim ride in `extra`, IOTA in `ota`,
// the stamps in `upload` and the contest block in `contest`. src-tauri's
// `a_page_row_carries_what_the_logbook_columns_show` holds the engine to that shape from a real
// store.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, fireEvent, screen, cleanup, within } from '@testing-library/react'
import { Logbook } from './Logbook'
import * as api from '../api'
import type { LogQuestion } from '../features/logAnswers'
import type { LogColumnsSaved } from '../features/logColumns'

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 120 })
})

const engineLog = vi.hoisted(() => vi.fn())
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    deleteQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    editQsoById: vi.fn(() => Promise.resolve({ kind: 'applied' })),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    logQso: vi.fn(() => Promise.resolve({})), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: noop(), markQslCardById: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

/** A stored contest contact as a page answer carries it. */
const CONTACT = {
  id: 'id-DL1ABC', call: 'DL1ABC', grid: 'JO31LK', band: '20m', freqMhz: 14.025, mode: 'CW',
  rstSent: '599', rstRcvd: '599', name: null, qth: null, comment: null, notes: null,
  country: 'Fed. Rep. of Germany', whenUnix: 1_700_000_000,
  confirmed: true, awardConfirmed: true,
  qslRcvd: { card: false, lotw: true, eqsl: false, qrz: false },
  qslSent: { sent: true, via: 'B', dateUnix: 1_700_500_000 },
  ota: { myProgram: null, myRef: null, theirProgram: null, theirRef: null, iota: 'EU-005' },
  upload: { lotw: { outcome: 'accepted', whenUnix: 1_700_100_000 }, clublog: { outcome: 'rejected', whenUnix: 1_700_200_000, detail: 'record' } },
  contest: { contestId: 'CQ-WW-CW', stx: 12, srx: 345, sent: [['RST', '599'], ['CQZ', '4']], rcvd: [['RST', '599'], ['CQZ', '14']] },
  extra: [
    ['CNTY', 'MA,MIDDLESEX'], ['CQZ', '14'], ['ITUZ', '28'], ['CONT', 'EU'], ['DISTANCE', '6512.4'],
    ['QSLRDATE', '20240102'], ['LOTW_QSLRDATE', '20240203'], ['EQSL_QSLRDATE', '20240304'],
  ],
}

async function renderLog(props: { columns?: LogColumnsSaved | null; onColumnsChange?: (c: LogColumnsSaved | null) => void } = {}) {
  engineLog.mockResolvedValue([CONTACT])
  const utils = render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" {...props} />)
  await waitFor(() => expect(utils.container.querySelector('.logbook-row:not(.head):not(.placeholder)')).not.toBeNull())
  return utils
}

const headers = () => screen.getAllByRole('columnheader').map((h) => h.getAttribute('aria-label') ?? (h.firstChild?.textContent ?? '').trim())
const row = (c: HTMLElement) => c.querySelector('.logbook-row:not(.head):not(.placeholder)') as HTMLElement
const table = (c: HTMLElement) => c.querySelector('.logbook-table') as HTMLElement
const template = (c: HTMLElement) => table(c).style.getPropertyValue('--log-cols')

async function openChooser() {
  fireEvent.keyDown(screen.getByRole('button', { name: 'Columns' }), { key: 'Enter' })
  return screen.findByRole('menu')
}

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
  document.documentElement.removeAttribute('data-viewport')
  document.documentElement.style.removeProperty('--vw-eff')
  document.documentElement.style.removeProperty('--vh-eff')
})

describe('the default table is the one the Logbook had', () => {
  it('the same columns, and one track per cell rendered', async () => {
    const { container } = await renderLog()
    expect(headers()).toEqual(['Call', 'Country', 'Band', 'Freq', 'Mode', 'Sent', 'Rcvd', 'Date (UTC)', 'Time (UTC)', 'Park', 'QSL', 'Notes', 'Edit / delete'])
    const tracks = template(container).replace(/minmax\([^)]*\)/, 'M').split(/\s+/)
    expect(tracks).toHaveLength(row(container).querySelectorAll(':scope > .log-cell').length)
    expect(tracks).toHaveLength(container.querySelectorAll('.logbook-row.head > .log-cell').length)
    expect(table(container).classList.contains('wide')).toBe(false)
  })

  it('at a narrow window: the six columns, each with its track', async () => {
    document.documentElement.setAttribute('data-viewport', 'sm')
    const { container } = await renderLog()
    expect(headers()).toEqual(['Call', 'Country', 'Band', 'QSL', 'Notes', 'Edit / delete'])
    expect(template(container)).toBe('1fr 1.3fr 0.7fr 0.7fr 1fr minmax(384px, 0.8fr)')
    expect(row(container).querySelectorAll(':scope > .log-cell')).toHaveLength(6)
  })
})

describe('the added columns show what the store holds', () => {
  const ALL_ADDED: LogColumnsSaved = {
    shown: ['grid', 'iota', 'county', 'cqz', 'ituz', 'cont', 'distance', 'qslSentDate', 'qslRcvdDate', 'lotwRcvdDate', 'eqslRcvdDate', 'upLotw', 'upEqsl', 'upQrz', 'upClublog', 'contest', 'exchSent', 'exchRcvd'],
    hidden: [],
    widths: {},
  }

  it('each column prints its value from the contact', async () => {
    const { container } = await renderLog({ columns: ALL_ADDED })
    const h = headers()
    const cells = [...row(container).querySelectorAll(':scope > .log-cell')]
    const at = (name: string) => cells[h.indexOf(name)]
    expect(cells).toHaveLength(h.length)
    expect(at('Their grid').textContent).toBe('JO31LK')
    expect(at('IOTA').textContent).toBe('EU-005')
    expect(at('County').textContent).toBe('MA,MIDDLESEX')
    expect(at('CQ zone').textContent).toBe('14')
    expect(at('ITU zone').textContent).toBe('28')
    expect(at('Continent').textContent).toBe('EU')
    expect(at('Distance (km)').textContent).toBe('6512')
    expect(at('Card sent').textContent).toBe('2023-11-20')
    expect(at('Card received').textContent).toBe('2024-01-02')
    expect(at('LoTW received').textContent).toBe('2024-02-03')
    expect(at('eQSL received').textContent).toBe('2024-03-04')
    expect(at('LoTW upload').textContent).toBe('✓')
    expect(at('LoTW upload').getAttribute('title')).toMatch(/^Uploaded, 2023-11-16/)
    expect(at('Club Log upload').textContent).toBe('✗')
    expect(at('QRZ upload').getAttribute('title')).toBe('Not uploaded')
    expect(at('Contest').textContent).toBe('CQ-WW-CW')
    expect(at('Exchange sent').textContent).toBe('599 4')
    expect(at('Exchange received').textContent).toBe('599 14')
    // A table showing added columns scrolls sideways, as "More columns" did.
    expect(table(container).classList.contains('wide')).toBe(true)
    expect(table(container).style.getPropertyValue('--log-min-w')).toMatch(/rem$/)
  })

  it('an added column sorts only where the engine can order it: its header is a button or not', async () => {
    await renderLog({ columns: ALL_ADDED })
    const header = (name: string) => screen.getAllByRole('columnheader').find((x) => x.textContent?.startsWith(name))
    // A passthrough tag with no index does not sort; the county, which the store indexes, does.
    expect(header('CQ zone')?.tagName).toBe('SPAN')
    expect(header('County')?.tagName).toBe('BUTTON')
    expect(screen.getByRole('columnheader', { name: 'Call' }).tagName).toBe('BUTTON')
  })
})

describe('the chooser shows and hides columns, and hands each choice to be saved', () => {
  it('turns IOTA on, then a default column off, then resets', async () => {
    const saved = vi.fn()
    const { container } = await renderLog({ onColumnsChange: saved })
    let menu = await openChooser()
    fireEvent.click(within(menu).getByRole('menuitemcheckbox', { name: 'IOTA' }))
    expect(saved).toHaveBeenLastCalledWith({ shown: ['iota'], hidden: [], widths: {} })
    expect(headers()).toContain('IOTA')
    expect(row(container).textContent).toContain('EU-005')

    fireEvent.click(within(menu).getByRole('menuitemcheckbox', { name: 'Freq' }))
    expect(saved).toHaveBeenLastCalledWith({ shown: ['iota'], hidden: ['freq'], widths: {} })
    expect(headers()).not.toContain('Freq')
    // The callsign and the controls are not offered: they cannot be hidden.
    expect(within(menu).queryByRole('menuitemcheckbox', { name: 'Call' })).toBeNull()

    fireEvent.click(within(menu).getByRole('menuitem', { name: 'Reset to the default columns' }))
    expect(saved).toHaveBeenLastCalledWith(null)
    menu = await openChooser()
    expect(within(menu).getByRole('menuitemcheckbox', { name: 'Freq' }).getAttribute('aria-checked')).toBe('true')
    expect(headers()).not.toContain('IOTA')
  })
})

describe('resize', () => {
  it('Alt+→ on a header widens its column and saves the width', async () => {
    const saved = vi.fn()
    const { container } = await renderLog({ onColumnsChange: saved })
    fireEvent.keyDown(screen.getByRole('columnheader', { name: 'Country' }), { key: 'ArrowRight', altKey: true })
    // From the column's laid-out width (120 px here) by one step.
    expect(saved).toHaveBeenLastCalledWith({ shown: [], hidden: [], widths: { country: 136 } })
    expect(template(container).split(' ')[1]).toBe('136px')
    // An arrow without Alt is the grid's own keys, not a resize.
    saved.mockClear()
    fireEvent.keyDown(screen.getByRole('columnheader', { name: 'Country' }), { key: 'ArrowRight' })
    expect(saved).not.toHaveBeenCalled()
  })

  it('dragging the header edge resizes without sorting, and saves once on release', async () => {
    const saved = vi.fn()
    const { container } = await renderLog({ onColumnsChange: saved })
    const header = screen.getByRole('columnheader', { name: 'Country' })
    const handle = header.querySelector('.log-col-resize') as HTMLElement
    expect(handle).not.toBeNull()
    fireEvent.pointerDown(handle, { button: 0, clientX: 300, pointerId: 1 })
    fireEvent.pointerMove(handle, { clientX: 360, pointerId: 1 })
    expect(template(container).split(' ')[1]).toBe('180px')
    expect(saved).not.toHaveBeenCalled()
    fireEvent.pointerUp(handle, { clientX: 360, pointerId: 1 })
    fireEvent.click(handle)
    expect(saved).toHaveBeenCalledTimes(1)
    expect(saved).toHaveBeenLastCalledWith({ shown: [], hidden: [], widths: { country: 180 } })
    expect(header.getAttribute('aria-sort'), 'the click that ends a drag must not sort').toBe('none')
  })
})

describe('a saved choice is clamped against the window it is read on', () => {
  it('a 1800 px width saved on a big monitor comes back as half a 1366 px window', async () => {
    // The effective window as useViewport publishes it.
    document.documentElement.style.setProperty('--vw-eff', '1366px')
    document.documentElement.style.setProperty('--vh-eff', '768px')
    const { container } = await renderLog({ columns: { shown: ['county'], hidden: [], widths: { county: 1800 } } })
    expect(template(container).split(' ')).toContain('683px')
    expect(template(container)).not.toContain('1800px')
  })
})

describe('the editor shows the added fields and edits IOTA', () => {
  async function openEdit(container: HTMLElement) {
    fireEvent.click(container.querySelector('button[aria-label="Edit DL1ABC"]') as HTMLButtonElement)
    await waitFor(() => expect(container.querySelector('.logbook-form')).not.toBeNull())
    return container.querySelector('.logbook-form') as HTMLElement
  }

  it('opens on the stored IOTA and sends a changed one in the edit', async () => {
    const { container } = await renderLog()
    const form = await openEdit(container)
    const iota = within(form).getByLabelText('IOTA') as HTMLInputElement
    expect(iota.value).toBe('EU-005')
    fireEvent.change(iota, { target: { value: 'eu-115' } })
    fireEvent.click(within(form).getByRole('button', { name: /save/i }))
    await waitFor(() => expect(api.editQsoById).toHaveBeenCalled())
    const edit = vi.mocked(api.editQsoById).mock.calls[0][1]
    expect(edit.ota).toEqual({ myProgram: null, myRef: null, theirProgram: null, theirRef: null, iota: 'EU-115' })
  })

  it('refuses a value that is not an IOTA reference, and sends nothing', async () => {
    const { container } = await renderLog()
    const form = await openEdit(container)
    fireEvent.change(within(form).getByLabelText('IOTA'), { target: { value: 'EU5' } })
    fireEvent.click(within(form).getByRole('button', { name: /save/i }))
    expect(await within(form).findByRole('alert')).toBeTruthy()
    expect(api.editQsoById).not.toHaveBeenCalled()
  })

  it('lists what the contact carries that no box edits', async () => {
    const { container } = await renderLog()
    const form = await openEdit(container)
    const also = form.querySelector('.logbook-also') as HTMLElement
    expect(also).not.toBeNull()
    const pairs = [...also.querySelectorAll('dt')].map((dt) => `${dt.textContent}=${dt.nextElementSibling?.textContent}`)
    expect(pairs).toEqual(expect.arrayContaining([
      'County=MA,MIDDLESEX', 'CQ zone=14', 'ITU zone=28', 'Continent=EU', 'Distance (km)=6512',
      'LoTW upload=Uploaded', 'Club Log upload=Refused', 'Contest=CQ-WW-CW', 'Exchange sent=599 4',
    ]))
    // The two the form edits are boxes, not this line.
    expect(pairs.some((p) => p.startsWith('IOTA=') || p.startsWith('Their grid='))).toBe(false)
  })
})
