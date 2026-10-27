// @vitest-environment jsdom
//
// THE LOGBOOK OPENS ON A COMPUTER THAT MAKES NO WEBGL 2 CONTEXT (a report on 1.18.0 from Linux Mint 22.3:
// "LOGBOOK HIT AN ERROR … THREE.WebGLRenderer: Error creating WebGL context.").
//
// three.js draws with WebGL 2 only, but the globe's GPU gate (gpu.ts `gpuCapableForGlobe`) passes a WebGL 1
// context with a hardware renderer. On such a machine the band mounted the globe, three.js threw on the
// WebGL 2 context it could not get, and nothing between the globe and App's section boundary caught it: the
// whole Logbook was the error page, and the operator could not open the log at all.
//
// The machine here is that one, made the way three.js sees it: a canvas gives a WebGL 1 context that names a
// real GPU, and no WebGL 2 one. The GPU gate, QsoGlobe, react-globe.gl and three.js are the real ones, inside
// App's real section boundary; only the canvas contexts are fakes.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { render, waitFor, fireEvent, cleanup, screen } from '@testing-library/react'
import { Logbook } from './Logbook'
import { ErrorBoundary } from './ErrorBoundary'
import * as api from '../api'
import * as toast from '../toast'
import { t } from '../i18n'
import { LOGBOOK_GLOBE_KEY } from '../features/logbookGlobe'
import type { LogQuestion } from '../features/logAnswers'

const engineLog = vi.hoisted(() => vi.fn())
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    deleteQsoById: noop(), editQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(), previewImport: noop(), importLog: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    saveTextToDownloads: noop(),
    logQso: noop(), markQslSentById: noop(), purgeLog: noop(), qrzLookup: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(),
  }
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn((fn: () => unknown) => fn()) }))

/** WEBGL_debug_renderer_info's UNMASKED_RENDERER_WEBGL, and the texture limit the gate reads. */
const UNMASKED_RENDERER_WEBGL = 0x9246
const MAX_TEXTURE_SIZE = 0x0d33

/** A WebGL 1 context on a GPU that has no WebGL 2 (an OpenGL 2.1 Intel part, as Mesa names it). */
function webgl1() {
  return {
    MAX_TEXTURE_SIZE,
    getExtension: (name: string) =>
      name === 'WEBGL_debug_renderer_info'
        ? { UNMASKED_RENDERER_WEBGL }
        : name === 'WEBGL_lose_context'
          ? { loseContext() {}, restoreContext() {} }
          : null,
    getParameter: (p: number) =>
      p === UNMASKED_RENDERER_WEBGL ? 'Mesa Intel(R) HD Graphics (ILK)' : p === MAX_TEXTURE_SIZE ? 8192 : null,
    isContextLost: () => false,
  }
}

/** What each kind of context this machine makes: 'webgl' only, unless a test says otherwise. */
let makes: (kind: string) => unknown

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  globalThis.IntersectionObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof IntersectionObserver
  // react-virtual reads offsetHeight; the globe band measures its client box before it draws.
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})
beforeEach(() => {
  localStorage.clear()
  makes = (kind) => (kind === 'webgl' ? webgl1() : null)
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(((kind: string) => makes(kind)) as never)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(900)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(320)
  vi.spyOn(console, 'error').mockImplementation(() => {})
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

const qso = (call: string, i: number) => ({
  call, grid: 'FN31', band: '40m', freqMhz: 7.074, mode: 'FT8', rstSent: '-10', rstRcvd: '-12', name: null,
  qth: null, comment: null, notes: null, country: 'United States', whenUnix: 1_700_000_000 + i, confirmed: false,
  awardConfirmed: false, qslRcvd: null, qslSent: null, ota: null, upload: undefined,
})
const LOG = [qso('K1ABC', 0), qso('K2XYZ', 1), qso('W3DEF', 2)]

/** The calls in the table, from each row's Edit button. */
const calls = (c: HTMLElement) =>
  [...c.querySelectorAll('button[aria-label^="Edit "]')].map((b) => b.getAttribute('aria-label')!.slice(5)).sort()

/** The Logbook as App mounts it, inside App's section boundary, with its rows and its log size in: the
 *  band is drawn for a log with contacts in it, so "no band" after this means gated, not not-yet. */
async function mount() {
  engineLog.mockResolvedValue(LOG)
  const { container } = render(
    <ErrorBoundary label={t('logbook.title')}>
      <Logbook defaultBand="40m" defaultFreqMhz={7.074} defaultMode="FT8" />
    </ErrorBoundary>,
  )
  await waitFor(() => expect(calls(container)).toEqual(['K1ABC', 'K2XYZ', 'W3DEF']))
  await waitFor(() => expect(container.querySelector('.count-badge')?.textContent).toBe('3'))
  return container
}

/** That, once its globe band has settled: the section's error page, the band's note, or a globe drawn. */
async function open() {
  const container = await mount()
  await waitFor(
    () => {
      const band = container.querySelector('.log-globe-band')
      const settled = container.querySelector('.view-crash') ?? band?.querySelector('.globe-note') ?? band?.querySelector('canvas')
      expect(settled, 'the band never settled').toBeTruthy()
    },
    { timeout: 20_000 },
  )
  return container
}

/** The section's error page, if that is what is on screen. */
const crash = (c: HTMLElement) => c.querySelector('.view-crash')?.textContent ?? null

describe('the Logbook on a computer with WebGL 1 and no WebGL 2', () => {
  it('opens with its table, and the globe band says why there is no globe', async () => {
    const c = await open()
    expect(crash(c), 'the Logbook is the error page').toBeNull()
    expect(calls(c)).toEqual(['K1ABC', 'K2XYZ', 'W3DEF'])
    const band = c.querySelector('.log-globe-band')
    expect(band?.textContent).toBe(t('logbook.globe.noWebgl2'))
    expect(band?.querySelector('canvas'), 'a globe canvas in the band').toBeNull()
  }, 30_000)

  it('searches, edits and imports while the band shows the note', async () => {
    const editQsoById = vi.mocked(api.editQsoById)
    editQsoById.mockResolvedValue({ kind: 'applied' } as never)
    vi.mocked(api.previewImport).mockResolvedValue({
      format: 'adif', template: null, records: 1, contacts: 1, firstUnix: null, lastUnix: null,
      new: 1, duplicates: 0, updates: 0, notes: [], coverage: [], unmapped: [],
    })
    const importLog = vi.mocked(api.importLog)
    importLog.mockResolvedValue({
      stats: { added: 1, skipped: 0, updated: 0, total: 4, run: 'import-run' },
      report: {
        format: 'adif', template: null, records: 1, contacts: 1, added: 1, duplicates: 0, updated: 0,
        newOnly: false, fields: [], unmapped: [], notes: [], accounted: true,
      },
    })
    const c = await open()
    expect(crash(c), 'the Logbook is the error page').toBeNull()

    // Search: the table narrows to the one call.
    fireEvent.change(c.querySelector('.log-search') as HTMLInputElement, { target: { value: 'K2X' } })
    await waitFor(() => expect(calls(c)).toEqual(['K2XYZ']))
    fireEvent.change(c.querySelector('.log-search') as HTMLInputElement, { target: { value: '' } })
    await waitFor(() => expect(calls(c)).toEqual(['K1ABC', 'K2XYZ', 'W3DEF']))

    // Edit: the form opens on the row and saves it back.
    fireEvent.click(c.querySelector('button[aria-label="Edit W3DEF"]') as HTMLButtonElement)
    await waitFor(() => expect(c.querySelector('.logbook-form')).not.toBeNull())
    fireEvent.click(c.querySelector('.logbook-form button[type="submit"]') as HTMLButtonElement)
    await waitFor(() => expect(editQsoById).toHaveBeenCalledTimes(1))
    expect(editQsoById.mock.calls[0][1].call).toBe('W3DEF')

    // Import: the file's text goes to the engine, and the result is said.
    const adif = '<CALL:5>N4GHI <BAND:3>20m <MODE:3>FT8 <QSO_DATE:8>20261015 <TIME_ON:4>1200 <EOR>'
    fireEvent.change(c.querySelector('.log-actions input[type="file"]') as HTMLInputElement, {
      target: { files: [new File([adif], 'log.adi')] },
    })
    await waitFor(() => expect(api.previewImport).toHaveBeenCalledWith(adif))
    fireEvent.click(await screen.findByRole('button', { name: t('logbook.import.preview.import') }))
    await waitFor(() => expect(importLog).toHaveBeenCalledWith(adif, false))
    await waitFor(() => expect(vi.mocked(toast.pushToast)).toHaveBeenCalledWith(
      t('logbook.import.imported', { count: 1 }),
      'success',
      10_000,
      expect.objectContaining({ actionLabel: t('logbook.undo') }),
    ))
    expect(crash(c), 'the Logbook is the error page').toBeNull()
  }, 30_000)

  it('control: with no WebGL at all the GPU gate is shut, and there is no band', async () => {
    makes = () => null
    const c = await mount()
    expect(crash(c)).toBeNull()
    expect(c.querySelector('.log-globe-band')).toBeNull()
  }, 30_000)

  it("1.18.0's way round it: with Settings' Logbook globe off, the Logbook opens on this machine", async () => {
    localStorage.setItem(LOGBOOK_GLOBE_KEY, 'off')
    const c = await mount()
    expect(crash(c)).toBeNull()
    expect(c.querySelector('.log-globe-band')).toBeNull()
  }, 30_000)
})
