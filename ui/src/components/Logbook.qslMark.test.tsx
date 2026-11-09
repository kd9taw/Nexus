// @vitest-environment jsdom
//
// THE QSL MARK NAMES THE SERVICE THAT CONFIRMED A CONTACT, AND WHAT AN eQSL IS WORTH.
//
// A contact confirmed in the operator's QRZ logbook and nowhere else showed the yellow word
// "eQSL", so an eQSL user could not tell a card from a QRZ match. QRZ has its own letter beside
// LoTW's, the card's and eQSL's. And the eQSL tooltips said WAZ does not accept eQSL, which CQ's
// own rules contradict: CQ accepts eQSLs from Authenticity Guaranteed members for WAZ and WPX.
// The ARRL's awards are the ones that turn eQSL away, and Nexus counts an AG eQSL toward WAZ as CQ
// does, so the tooltip says both.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, cleanup } from '@testing-library/react'
import { Logbook } from './Logbook'
import type { LogQuestion } from '../features/logAnswers'

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})

/** The log the engine holds: `askLog` answers from it as the engine does (features/logAnswers.testkit). */
const engineLog = vi.hoisted(() => vi.fn())
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    deleteQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    editQsoById: vi.fn(async () => ({})),
    logOperators: vi.fn(async () => [] as string[]), exportLogForOperator: noop(),
    logActivations: vi.fn(async () => []), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]),
    setSatTagById: vi.fn(async () => ({})),
    logQso: vi.fn(async () => ({})), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: noop(), markQslCardById: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

type Channels = { card: boolean; lotw: boolean; eqsl: boolean; qrz: boolean }

/** One contact, confirmed by `qslRcvd`'s channels as the engine derives the two flags from them.
 *  `qslRcvd: null` with `confirmed` is a row that names no channel. `extra` is the fields kept as
 *  written, where an eQSL merge keeps the card's AG mark and the day it arrived. */
function logRow(qslRcvd: Channels | null, confirmed?: boolean, extra: [string, string][] = []) {
  return {
    id: 'id-K0ABC', call: 'K0ABC', grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8',
    rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
    country: 'United States', whenUnix: 1_700_000_000,
    confirmed:
      confirmed ?? (!!qslRcvd && (qslRcvd.card || qslRcvd.lotw || qslRcvd.eqsl || qslRcvd.qrz)),
    awardConfirmed: !!qslRcvd && (qslRcvd.card || qslRcvd.lotw),
    qslRcvd, qslSent: null, ota: null, upload: undefined, extra,
  }
}

/** The QSL column's mark for that contact: its text and its tooltip. */
async function qslMark(qslRcvd: Channels | null, confirmed?: boolean, extra?: [string, string][]) {
  engineLog.mockResolvedValue([logRow(qslRcvd, confirmed, extra)])
  const { container } = render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />)
  await waitFor(() =>
    expect(container.querySelector('.logbook-row:not(.head) .log-qsl')).not.toBeNull(),
  )
  const mark = container.querySelector('.logbook-row:not(.head) .log-qsl') as HTMLElement
  return { text: mark.textContent, title: mark.getAttribute('title') }
}

const NONE: Channels = { card: false, lotw: false, eqsl: false, qrz: false }

/** What an eQSL is worth, as the eQSL tooltips say it in English. */
const EQSL_WORTH =
  '(ARRL awards do not accept it; CQ accepts it for WAZ and WPX from AG members, and Nexus counts ' +
  'an AG one toward WAZ)'

describe('the Logbook QSL mark', () => {
  afterEach(() => {
    cleanup()
    vi.clearAllMocks()
  })

  it('marks a confirmation from the QRZ logbook alone Q, never eQSL', async () => {
    expect(await qslMark({ ...NONE, qrz: true })).toEqual({
      text: 'Q',
      title: 'QRZ logbook confirmed (ARRL awards do not accept it)',
    })
  })

  it('joins QRZ to the other channels that confirmed the contact', async () => {
    const { text, title } = await qslMark({ ...NONE, lotw: true, eqsl: true, qrz: true })
    expect(text).toBe('L·E·Q')
    expect(title).toBe(
      `LoTW confirmed (award-eligible) · eQSL received ${EQSL_WORTH} · ` +
        'QRZ logbook confirmed (ARRL awards do not accept it)',
    )
  })

  it('says the ARRL refuses an eQSL and CQ takes an AG one, never that WAZ refuses it', async () => {
    expect(await qslMark({ ...NONE, eqsl: true })).toEqual({
      text: 'E',
      title: `eQSL received ${EQSL_WORTH}`,
    })
  })

  it('keeps the yellow eQSL word on a row that names no channel, with the same sentence', async () => {
    expect(await qslMark(null, true)).toEqual({ text: 'eQSL', title: `eQSL only ${EQSL_WORTH}` })
  })

  it('marks an eQSL that eQSL calls Authenticity Guaranteed AG, with the day the card arrived', async () => {
    const card: [string, string][] = [['APP_EQSL_AG', 'Y'], ['EQSL_QSLRDATE', '20261002']]
    expect(await qslMark({ ...NONE, eqsl: true }, undefined, card)).toEqual({
      text: 'AG',
      title:
        'Authenticity Guaranteed (AG) eQSL received (ARRL awards do not accept it; CQ accepts it ' +
        'for WAZ and WPX, and Nexus counts it toward WAZ) · eQSL card arrived 2026-10-02',
    })
    cleanup()
    const { text } = await qslMark({ ...NONE, lotw: true, eqsl: true }, undefined, card)
    expect(text).toBe('L·AG')
  })

  it('keeps E for a card eQSL does not mark AG, with the day it arrived', async () => {
    expect(await qslMark({ ...NONE, eqsl: true }, undefined, [['EQSL_QSLRDATE', '20261002']])).toEqual({
      text: 'E',
      title: `eQSL received ${EQSL_WORTH} · eQSL card arrived 2026-10-02`,
    })
  })
})
