// @vitest-environment jsdom
//
// THE LOG-ENTRY BOXES SHOW NO MADE-UP VALUES.
//
// An empty box used to show a sample in grey — W1AW in Call, 1D and WI in Field Day's Class and
// Section, a county in a QSO party's county box, FN31 and 20m in the Logbook's add-a-contact form.
// A sample callsign or grid in an empty box reads as a value already there. Every such box is now
// blank. A box whose hint is a word ('Call', 'Grid', 'Name') or names a format (the UTC date
// format, 'RST', 'MHz') keeps it. Asserted box by box, by value.
//
// ⚠️ jsdom lays out nothing; no assertion here is about geometry.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { Logbook } from './Logbook'
import { t } from '../i18n'
import type { AppSnapshot, ContestFieldSpec, FieldDayStatus } from '../types'
import type { LogQuestion } from '../features/logAnswers'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => {
  const noop = () => vi.fn(() => Promise.resolve(null))
  return {
    fdLogManual: noop(), contestLogManual: noop(), contestWorking: noop(), contestEntryReset: noop(),
    contestPlace: noop(), logQso: noop(), lookupPark: noop(), lookupParkLive: noop(), qrzLookup: noop(),
    resolveEntity: noop(), searchParks: vi.fn(() => Promise.resolve([])), setCwPeerInfo: noop(),
    setLogFormGrid: noop(),
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, [])),
    deleteQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(), editQsoById: noop(),
    logOperators: vi.fn(() => Promise.resolve([])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(() => Promise.resolve([])), setSatTagById: noop(),
    purgeLog: noop(), markQslSentById: noop(), markQslCardById: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})

afterEach(cleanup)

const snap = { radio: { band: '20m', dialMhz: 14.2 }, hunt: null } as unknown as AppSnapshot

const contest = (receives?: ContestFieldSpec[]): FieldDayStatus =>
  ({ running: true, state: '', qsoCount: 0, sections: 0, points: 0, log: [], ...(receives ? { receives } : {}) }) as unknown as FieldDayStatus

function strip(fieldDay: FieldDayStatus | null) {
  render(<LogEntry snap={snap} mode="PH" defaultRst="59" exchange="terrestrial" fieldDay={fieldDay} fdMode="PH" />)
}

/** The contest line's boxes, by caption, each with its placeholder. */
const contestBoxes = () =>
  Object.fromEntries(
    [...document.querySelectorAll('.le-fd-big .le-fd-field')].map((l) => [
      l.querySelector('.le-fd-cap-text, .le-fd-cap')?.textContent ?? '',
      (l.querySelector('input') as HTMLInputElement).placeholder,
    ]),
  )

describe('the contest entry line', () => {
  it('Field Day: Call, Class and Section are blank', () => {
    strip(contest())
    expect(contestBoxes()).toEqual({ Call: '', Class: '', Section: '' })
  })

  it('a QSO party county box and a Sweepstakes section box are blank', () => {
    strip(
      contest([
        { key: 'QTH', kind: 'enum', required: true, domain: 'il_counties', domains: ['il_counties'] },
        { key: 'SEC', kind: 'enum', required: true, domain: 'arrl_sections', domains: ['arrl_sections'] },
      ] as ContestFieldSpec[]),
    )
    expect(contestBoxes()).toEqual({ Call: '', County: '', Section: '' })
  })
})

describe('the ordinary log line', () => {
  it('the park box names what goes in it, with no sample reference', () => {
    strip(null)
    const park = document.querySelector<HTMLInputElement>('.le-park-ref')!
    expect(park.placeholder).toBe('Park (reference or name)')
    fireEvent.change(document.querySelector('.le-park-prog')!, { target: { value: 'SOTA' } })
    expect(document.querySelector<HTMLInputElement>('.le-park-ref')!.placeholder).toBe('Summit (reference)')
  })

  it('keeps the word labels', () => {
    strip(null)
    const shown = [...document.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>('input, textarea')].map((i) => i.placeholder)
    for (const word of ['Call', 'Name', 'QTH (city)', 'State', 'Country', 'Comment (sharable)', 'RST']) {
      expect(shown, word).toContain(word)
    }
  })
})

describe("the Logbook's add-a-contact form", () => {
  it('every box that showed a sample is blank; the format and word hints stay', async () => {
    const { container, getByText } = render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />)
    fireEvent.click(getByText(t('logbook.form.open')))
    await waitFor(() => expect(container.querySelector('.logbook-form')).not.toBeNull())
    const box = (label: string) => {
      const l = [...container.querySelectorAll('.logbook-form label.logbook-field')].find(
        (x) => x.querySelector('span')?.textContent === label,
      )
      return (l!.querySelector('input, textarea') as HTMLInputElement).placeholder
    }
    expect({
      call: box(t('logbook.field.call.label')),
      grid: box(t('logbook.field.grid.label')),
      band: box(t('logbook.field.band.label')),
      mode: box(t('logbook.field.mode.label')),
      rstSent: box(t('logbook.field.rstSent.label')),
      rstRcvd: box(t('logbook.field.rstRcvd.label')),
      state: box(t('logbook.field.state.label')),
      txPower: box(t('logbook.field.txPower.label')),
      parkTheirs: box(t('logbook.field.parkTheirs.label')),
      parkMine: box(t('logbook.field.parkMine.label')),
      name: box(t('logbook.field.name.label')),
      qth: box(t('logbook.field.qth.label')),
      comment: box(t('logbook.field.comment.label')),
      date: box(t('logbook.field.date.label')),
    }).toEqual({
      call: '',
      grid: '',
      band: '',
      mode: '',
      rstSent: '',
      rstRcvd: '',
      state: '',
      txPower: '',
      parkTheirs: '',
      parkMine: '',
      name: '',
      qth: '',
      comment: 'Shared on the QSL',
      date: 'YYYY-MM-DD',
    })
  })
})
