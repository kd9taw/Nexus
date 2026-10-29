// @vitest-environment jsdom
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, cleanup, fireEvent } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { parseRules, cmpSpec } from '../cssCascade'
import { Logbook } from './Logbook'
import type { LogQuestion } from '../features/logAnswers'
import {
  clublogPushQso, deleteQsoById, hrdlogPushQso, markQslCardById, markQslSentById, qrzCorrectPreview, qrzPushQso,
  queueQslCardById, setSatTagById, uploadLotwReportByIds, wrlPushQso,
} from '../api'
import { confirmDialog } from '../confirm'
import { fmtUtc } from '../features/logQuery'
import { t } from '../i18n'
import type { UploadReport } from '../types'
import { chooseRowAction, menuItem, openRowMenu, rowMore } from './Logbook.rowMenu.testkit'

// THE LOGBOOK ROW: two buttons always on screen (✎ Edit and ⋯), every other action one deliberate
// choice in the ⋯ menu. What this file holds down:
//   · nothing in a row is clickable while it cannot be seen — a click or a tap on the blank right end
//     of a row once fired a hidden button under it and uploaded a contact to QRZ;
//   · each item that sends does what the row's own button did, with the same command;
//   · each upload names its service in full and says whether this contact went there already.

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

const engineLog = vi.hoisted(() => vi.fn())
vi.mock('../api', () => {
  const noop = () => vi.fn()
  const answer = () => vi.fn(async () => ({ kind: 'done' }))
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    deleteQsoById: answer(), editQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => ['AO-91', 'SO-50']),
    setSatTagById: answer(), markQslSentById: answer(), markQslCardById: answer(), queueQslCardById: answer(),
    logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(),
    uploadLotwReportByIds: vi.fn(async () => ({ dispatched: 1, outcome: 'pending' })),
    qrzPushQso: vi.fn(async () => ({ result: 'ok' })),
    qrzCorrectPreview: vi.fn(async () => ({ confirmation: 'Change the time' })),
    clublogPushQso: vi.fn(async () => ({ result: 'ok' })),
    hrdlogPushQso: vi.fn(async () => ({ result: 'ok' })),
    wrlPushQso: vi.fn(async () => ({ result: 'accepted' })),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))
vi.mock('../confirm', () => ({ confirmDialog: vi.fn(async () => false) }))

const SENT_AT = 1_700_100_000
function contact(over: Record<string, unknown> = {}) {
  return {
    call: 'K0ABC', grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8',
    rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
    country: 'United States', whenUnix: 1_700_000_000, confirmed: false, awardConfirmed: false,
    qslRcvd: null, qslSent: null, ota: null,
    upload: { qrz: { outcome: 'accepted', whenUnix: SENT_AT }, clublog: { outcome: 'rejected', whenUnix: SENT_AT } },
    ...over,
  }
}

async function renderLog(over: Record<string, unknown> = {}): Promise<HTMLElement> {
  engineLog.mockResolvedValue([contact(over)])
  const { container } = render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />)
  await waitFor(() => expect(container.querySelector('.logbook-row:not(.head):not(.placeholder)')).not.toBeNull())
  return container.querySelector('.logbook-row:not(.head):not(.placeholder)') as HTMLElement
}

// ── The cascade, resolved for an element the pointer has never visited ────────────────────────
// jsdom applies no stylesheet, so the sheet is resolved here: every rule whose selector the element
// matches (jsdom's own `matches`, so `:hover` and `:focus-within` are false — the row nobody has
// pointed at), important first, then specificity, then source order. Its inline style wins over all.
const RULES = parseRules(readFileSync(resolve(process.cwd(), 'src', 'styles.css'), 'utf8').replace(/\/\*[\s\S]*?\*\//g, ''))

function resolved(el: Element, prop: string): string | null {
  const inline = (el as HTMLElement).style?.getPropertyValue(prop)
  if (inline) return inline
  let best: { value: string; important: boolean; spec: readonly number[]; order: number } | null = null
  for (const rule of RULES) {
    if (/::/.test(rule.selector)) continue
    let hit = false
    try {
      hit = el.matches(rule.selector)
    } catch {
      continue
    }
    if (!hit) continue
    for (const d of rule.decls) {
      if (d.prop !== prop) continue
      const important = /!important/.test(d.value)
      const cand = { value: d.value.replace(/\s*!important\s*/, '').trim(), important, spec: rule.spec, order: rule.order }
      const beats =
        !best ||
        (cand.important && !best.important) ||
        (cand.important === best.important && (cmpSpec(cand.spec, best.spec) > 0 || (cmpSpec(cand.spec, best.spec) === 0 && cand.order > best.order)))
      if (beats) best = cand
    }
  }
  return best?.value ?? null
}

/** Why `el` cannot be seen though it is in the page, or null: an opacity of 0 or `visibility:
 *  hidden` on it or on any box around it, up to the row. */
function unseen(el: Element, row: Element): string | null {
  for (let a: Element | null = el; a && row.contains(a); a = a.parentElement) {
    const o = resolved(a, 'opacity')
    if (o !== null && Number(o) === 0) return `opacity 0 on ${a.tagName.toLowerCase()}.${[...a.classList].join('.')}`
    if (resolved(a, 'visibility') === 'hidden') return `visibility hidden on ${a.tagName.toLowerCase()}`
  }
  return null
}

const CLICKABLE = 'button, select, a[href], input, textarea, [role="button"], [role="menuitem"], [tabindex]:not([tabindex="-1"])'

describe('a Logbook row has nothing clickable that cannot be seen', () => {
  it('every control in a row nobody has pointed at is visible', async () => {
    const row = await renderLog()
    const controls = [...row.querySelectorAll(CLICKABLE)]
    expect(controls.length, 'the row must render its controls, or this check is vacuous').toBeGreaterThan(1)
    const blind = controls.flatMap((el) => {
      const why = unseen(el, row)
      return why ? [`${el.getAttribute('aria-label') ?? el.textContent}: ${why}`] : []
    })
    expect(blind, 'a control a click or a tap reaches while the operator cannot see it').toEqual([])
  })

  it('FIRES: a control hidden the way the old row hid its buttons is reported', async () => {
    const row = await renderLog()
    const control = row.querySelector('.log-rowactions button') as HTMLElement
    control.style.setProperty('opacity', '0')
    expect(unseen(control, row)).toMatch(/opacity 0/)
    control.style.removeProperty('opacity')
    // …and one in a box made invisible around it.
    ;(row.querySelector('.log-rowactions') as HTMLElement).style.setProperty('visibility', 'hidden')
    expect(unseen(control, row)).toMatch(/visibility hidden/)
  })
})

describe('the row shows two buttons, and the rest is in its ⋯ menu', () => {
  it('shows ✎ Edit and ⋯ and nothing else', async () => {
    const row = await renderLog()
    const cell = row.querySelector('.log-rowactions')!
    expect([...cell.querySelectorAll('button, select')].map((b) => b.getAttribute('aria-label'))).toEqual([
      t('logbook.row.edit', { call: 'K0ABC' }),
      t('logbook.row.more', { call: 'K0ABC' }),
    ])
  })

  it('lists each upload by its full service name with this contact’s state', async () => {
    await renderLog()
    const more = rowMore('K0ABC')
    const menu = await openRowMenu('K0ABC')
    const texts = [...menu.querySelectorAll('[role="menuitem"]')].map((i) => i.textContent)
    const qrzSent = t('logbook.upload.title', { state: t('logbook.upload.done'), date: fmtUtc(SENT_AT) })
    const clublogRefused = t('logbook.upload.title', { state: t('logbook.upload.failed'), date: fmtUtc(SENT_AT) })
    expect(texts).toEqual([
      t('logbook.row.menu.spot'),
      t('logbook.row.menu.qrz') + qrzSent,
      t('logbook.row.menu.qrzCorrect'),
      t('logbook.row.menu.clublog') + clublogRefused,
      t('logbook.row.menu.hrdlog'),
      t('logbook.row.menu.wrl'),
      t('logbook.row.menu.lotw') + t('logbook.upload.none'),
      'QSL▸',
      t('logbook.row.menu.sat') + '▸',
      t('logbook.row.menu.delete'),
    ])
    expect(menu.getAttribute('aria-labelledby'), 'the menu is named by the row’s ⋯').toBe(more.id)
  })

  it('each upload item sends this contact with the command its old button used', async () => {
    const cases: [string, () => void][] = [
      [t('logbook.row.menu.qrz'), () => expect(qrzPushQso).toHaveBeenCalledWith(expect.objectContaining({ call: 'K0ABC' }))],
      [t('logbook.row.menu.qrzCorrect'), () => expect(qrzCorrectPreview).toHaveBeenCalledWith(expect.objectContaining({ call: 'K0ABC' }), 1)],
      [t('logbook.row.menu.clublog'), () => expect(clublogPushQso).toHaveBeenCalledWith(expect.objectContaining({ call: 'K0ABC' }))],
      [t('logbook.row.menu.hrdlog'), () => expect(hrdlogPushQso).toHaveBeenCalledWith(expect.objectContaining({ call: 'K0ABC' }))],
      [t('logbook.row.menu.wrl'), () => expect(wrlPushQso).toHaveBeenCalledWith(expect.objectContaining({ call: 'K0ABC' }))],
      [t('logbook.row.menu.lotw'), () => expect(uploadLotwReportByIds).toHaveBeenCalledWith([expect.any(String)])],
    ]
    for (const [label, sent] of cases) {
      await renderLog()
      await chooseRowAction('K0ABC', label)
      await waitFor(sent)
      // One choice, one send — and nothing else went out with it.
      const all = [qrzPushQso, qrzCorrectPreview, clublogPushQso, hrdlogPushQso, wrlPushQso, uploadLotwReportByIds]
      expect(all.reduce((n, f) => n + vi.mocked(f).mock.calls.length, 0), label).toBe(1)
      cleanup()
      vi.clearAllMocks()
    }
  })

  it('opening the menu sends nothing', async () => {
    await renderLog()
    await openRowMenu('K0ABC')
    for (const f of [qrzPushQso, clublogPushQso, hrdlogPushQso, wrlPushQso, uploadLotwReportByIds]) expect(f).not.toHaveBeenCalled()
  })

  it('says why LoTW cannot be chosen while TQSL is running', async () => {
    let finish: (v: UploadReport) => void = () => {}
    vi.mocked(uploadLotwReportByIds).mockImplementationOnce(() => new Promise<UploadReport>((r) => (finish = r)))
    await renderLog()
    await chooseRowAction('K0ABC', t('logbook.row.menu.lotw'))
    await waitFor(() => expect(uploadLotwReportByIds).toHaveBeenCalledTimes(1))
    const menu = await openRowMenu('K0ABC')
    const lotw = menuItem(menu, t('logbook.row.menu.lotw'))
    expect(lotw.getAttribute('aria-disabled')).toBe('true')
    expect(lotw.textContent).toContain(t('logbook.row.menu.lotwBusy'))
    fireEvent.click(lotw)
    expect(uploadLotwReportByIds).toHaveBeenCalledTimes(1)
    finish({ dispatched: 1, outcome: 'pending' })
  })

  it('QSL, satellite and delete do what the row’s menus and ✕ did', async () => {
    await renderLog()
    await chooseRowAction('K0ABC', 'QSL', t('logbook.row.qslSent.bureau'))
    await waitFor(() => expect(markQslSentById).toHaveBeenCalledWith(expect.anything(), 'B'))
    await chooseRowAction('K0ABC', 'QSL', t('logbook.row.qslQueue.add'))
    await waitFor(() => expect(queueQslCardById).toHaveBeenCalledWith(expect.anything(), true))
    await chooseRowAction('K0ABC', 'QSL', t('logbook.row.qslRcvd.card'))
    await waitFor(() => expect(markQslCardById).toHaveBeenCalledWith(expect.anything(), true))
    await chooseRowAction('K0ABC', t('logbook.row.menu.sat'), 'SO-50')
    await waitFor(() => expect(setSatTagById).toHaveBeenCalledWith(expect.anything(), 'SO-50'))
    // Delete still asks first, and a no deletes nothing.
    await chooseRowAction('K0ABC', t('logbook.row.menu.delete'))
    await waitFor(() => expect(confirmDialog).toHaveBeenCalledTimes(1))
    expect(deleteQsoById).not.toHaveBeenCalled()
  })

  it('offers the way back out of a send and a satellite tag only when there is one', async () => {
    await renderLog({ qslSent: { sent: true, via: 'B' }, propMode: 'SAT', satName: 'AO-91' })
    await chooseRowAction('K0ABC', 'QSL', t('logbook.row.qslSent.clear'))
    await waitFor(() => expect(markQslSentById).toHaveBeenCalledWith(expect.anything(), null))
    await chooseRowAction('K0ABC', t('logbook.row.menu.sat'), t('logbook.row.sat.clear'))
    await waitFor(() => expect(setSatTagById).toHaveBeenCalledWith(expect.anything(), null))
  })
})
