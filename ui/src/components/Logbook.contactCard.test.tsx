// @vitest-environment jsdom
//
// THE LOGBOOK CONTACT CARD (operator, 2026-10-26: "When you open a specific entry, it pops a card
// where you can edit details, see the QRZ default picture, link to QRZ bio for the call, etc.. The
// card should show recent QSOs that are logged, band, freq, mode, RST, etc.").
//
// Pinned here, through the Logbook as it mounts it, with the engine answering from a log:
//   · it opens on a double-click (and Enter, Logbook.keyboard.test.tsx), and a single click does not;
//   · Save is the edit form's save: ONE `editQsoById`, carrying the stored contact with ONLY the
//     boxes changed — every other field exactly as it was stored, including the ones a box cannot
//     show exactly (a frequency's sixth place, a time's seconds, an imported comment's line break);
//   · a field kept as written goes as a change against the value it was changed from, and only a
//     changed one goes at all;
//   · the history's Undo is on the card; the last ten contacts with the call, newest first;
//     switching to one; the unsaved-changes guard; a box that cannot be emptied says so;
//   · the edit form's Cancel (#239), and Download confirmations (#348).
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { render, waitFor, fireEvent, cleanup, screen, within, act } from '@testing-library/react'
import { Logbook } from './Logbook'
import { ConfirmHost } from '../confirm'
import * as api from '../api'
import { pushToast } from '../toast'
import { t } from '../i18n'
import type { LogQuestion } from '../features/logAnswers'
import type { LoggedQso } from '../types'
import { columnName, logColumn } from '../features/logColumns'
import { fmtUtc } from '../features/logQuery'

vi.setConfig({ testTimeout: 15_000 })

const engine = vi.hoisted(() => ({ log: [] as unknown[], revision: 1 }))
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) =>
      (await import('../features/logAnswers.testkit')).answerAs(q, engine.log as LoggedQso[], engine.revision),
    ),
    deleteQsoById: noop(),
    editQsoById: vi.fn(async (target: api.RowRef) => ({
      kind: 'applied',
      current: { row: (engine.log as LoggedQso[]).find((r) => r.id === target.id), editKey: 'k2' },
    })),
    exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(async () => [] as string[]), exportLogForOperator: noop(),
    logActivations: vi.fn(async () => []), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    saveTextToDownloads: noop(), logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: noop(), markQslCardById: noop(), syncLotwReport: noop(), uploadLotwReport: noop(),
    qrzPushQso: noop(), clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
    qsoHistory: vi.fn(async () => []), undoQsoChange: noop(), openQrzPage: noop(),
    getSettings: noop(), downloadLotwReport: noop(), downloadEqslReport: noop(), syncQrz: noop(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (run: () => Promise<unknown>) => {
    try {
      return await run()
    } catch {
      return null
    }
  }),
  failureText: (f: string) => f,
}))

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

/** The contact the card opens on: values a box cannot show exactly, and fields kept as written. */
const DL: LoggedQso = {
  id: 'id-dl', call: 'DL1ABC', grid: 'JO31AB', band: '20m', freqMhz: 14.074123, mode: 'FT8',
  rstSent: '-10', rstRcvd: '-12', name: null, qth: 'Koln', comment: 'line one\nline two', notes: 'dipole',
  state: null, txPower: 50, country: 'Germany', whenUnix: 1_700_000_007, timeOffUnix: 1_700_000_067,
  confirmed: true, awardConfirmed: true, qslRcvd: { card: false, lotw: true, eqsl: false, qrz: false },
  qslSent: { sent: true, via: 'B', dateUnix: 1_700_100_000 },
  ota: { theirProgram: 'SOTA', theirRef: 'DM/NS-001', myProgram: null, myRef: null, iota: null },
  myGrid: 'EN52', myRig: 'FT-710',
  extra: [['CNTY', 'X,Y'], ['CQZ', '14'], ['ITUZ', '28']],
}
/** Earlier contacts with the same call, an hour apart, and others with other calls. */
const earlier = (n: number): LoggedQso => ({
  ...DL, id: `id-dl-${n}`, whenUnix: 1_700_000_007 - 3600 * (n + 1), band: n % 2 ? '40m' : '20m',
  freqMhz: n % 2 ? 7.074 : 14.074, comment: null, extra: [], qslRcvd: { card: false, lotw: false, eqsl: n === 0, qrz: false },
})
const OTHER: LoggedQso = { ...DL, id: 'id-k1', call: 'K1XYZ', whenUnix: 1_700_000_500 }

beforeEach(() => {
  engine.log = [OTHER, DL, ...Array.from({ length: 12 }, (_, n) => earlier(n))]
  engine.revision += 1
  vi.mocked(api.editQsoById)
    .mockReset()
    .mockImplementation(async (target: api.RowRef) => ({
      kind: 'applied',
      current: { row: (engine.log as LoggedQso[]).find((r) => r.id === target.id)!, editKey: 'k2' },
    }) as never)
  vi.mocked(api.qsoHistory).mockResolvedValue([])
  vi.mocked(pushToast).mockClear()
})

async function mounted() {
  const r = render(
    <>
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />
      <ConfirmHost />
    </>,
  )
  await waitFor(() => expect(rowOf('DL1ABC')).not.toBeNull())
  return r
}
const rowOf = (call: string) =>
  [...document.querySelectorAll<HTMLElement>('.logbook-row:not(.head)')].find((r) => r.textContent?.includes(call)) ?? null
const cardEl = () => document.querySelector<HTMLElement>('.contact-card-dialog')
async function openCard(call = 'DL1ABC') {
  fireEvent.doubleClick(rowOf(call)!)
  await waitFor(() => expect(cardEl()).not.toBeNull())
  return cardEl()!
}
const box = (card: HTMLElement, label: string) =>
  within(card).getByText(label, { selector: '.logbook-field > span' }).parentElement!.querySelector('input, textarea') as
    | HTMLInputElement
    | HTMLTextAreaElement
const save = (card: HTMLElement) => fireEvent.click(within(card).getByRole('button', { name: t('logbook.form.save') }))

describe('the contact card opens from the list', () => {
  it('on a double-click, and not on a single click', async () => {
    await mounted()
    fireEvent.click(rowOf('DL1ABC')!)
    await new Promise((r) => setTimeout(r, 30))
    expect(cardEl(), 'a single click selects, it does not open').toBeNull()
    const card = await openCard()
    expect(box(card, t('logbook.field.call.label')).value).toBe('DL1ABC')
    // The QRZ page button, and the initials while the callbook has given no photo.
    expect(within(card).getByRole('button', { name: /QRZ/ })).toBeTruthy()
    expect(card.querySelector('.callcard-initials')?.textContent).toBe('DL')
    expect(api.qrzLookup, 'nothing asked of the callbook on open').not.toHaveBeenCalled()
  })
})

describe('Save is the edit form’s save, with only what was changed', () => {
  it('a changed name: one save, and every other field exactly as stored', async () => {
    await mounted()
    const card = await openCard()
    fireEvent.change(box(card, t('logbook.field.name.label')), { target: { value: 'Hans' } })
    save(card)
    await waitFor(() => expect(api.editQsoById).toHaveBeenCalledTimes(1))
    const [ref, edit, fields] = vi.mocked(api.editQsoById).mock.calls[0]
    expect(ref.id).toBe('id-dl')
    expect(fields, 'no field kept as written was touched, so none is sent').toBeUndefined()
    expect(edit).toEqual({
      call: 'DL1ABC', grid: 'JO31AB', state: null, band: '20m', freqMhz: 14.074123, mode: 'FT8',
      rstSent: '-10', rstRcvd: '-12', name: 'Hans', qth: 'Koln', comment: 'line one\nline two', notes: 'dipole',
      txPower: 50, whenUnix: 1_700_000_007, timeOffUnix: 1_700_000_067,
      ota: { myProgram: null, myRef: null, theirProgram: 'SOTA', theirRef: 'DM/NS-001', iota: null },
      myGrid: 'EN52', myRig: 'FT-710', qslSentVia: 'B', qslCard: false,
    })
  })

  it('fields kept as written: a corrected one and a removed one go, against what they held; the rest do not', async () => {
    await mounted()
    const card = await openCard()
    fireEvent.click(within(card).getByText(t('contactCard.more')))
    fireEvent.change(within(card).getByRole('textbox', { name: 'CNTY' }), { target: { value: 'MA,ESSEX' } })
    fireEvent.click(within(card).getByRole('button', { name: t('contactCard.field.remove', { tag: 'CQZ' }) }))
    fireEvent.click(within(card).getByRole('button', { name: t('contactCard.field.add') }))
    fireEvent.change(within(card).getByRole('textbox', { name: t('contactCard.field.tag') }), { target: { value: 'my_city' } })
    const added = within(card).getAllByRole('textbox', { name: 'MY_CITY' })
    fireEvent.change(added[0], { target: { value: 'Boston' } })
    save(card)
    await waitFor(() => expect(api.editQsoById).toHaveBeenCalledTimes(1))
    const [, edit, fields] = vi.mocked(api.editQsoById).mock.calls[0]
    expect(fields).toEqual([
      { tag: 'CNTY', old: 'X,Y', new: 'MA,ESSEX' },
      { tag: 'CQZ', old: '14', new: null },
      { tag: 'MY_CITY', old: null, new: 'Boston' },
    ])
    expect(edit.name, 'and the fields with boxes are as stored').toBeNull()
  })

  it('after a save of More fields alone the card is the contact as saved: not changed, and a second save goes', async () => {
    // The engine applies the change and keeps the edit key: `extra` is not under it.
    vi.mocked(api.editQsoById).mockImplementation(async (target: api.RowRef, _edit, fields) => {
      engine.log = (engine.log as LoggedQso[]).map((r) =>
        r.id === target.id
          ? { ...r, extra: (r.extra ?? []).map(([k, v]) => [k, fields?.find((f) => f.tag === k)?.new ?? v] as [string, string]) }
          : r,
      )
      engine.revision += 1
      const row = (engine.log as LoggedQso[]).find((r) => r.id === target.id)!
      return { kind: 'applied', current: { row, editKey: target.editKey } } as never
    })
    await mounted()
    const card = await openCard()
    fireEvent.click(within(card).getByText(t('contactCard.more')))
    fireEvent.change(within(card).getByRole('textbox', { name: 'CNTY' }), { target: { value: 'MA,ESSEX' } })
    save(card)
    await waitFor(() => expect(api.editQsoById).toHaveBeenCalledTimes(1))
    const button = () => within(cardEl()!).getByRole('button', { name: t('logbook.form.save') }) as HTMLButtonElement
    await waitFor(() => expect(button().disabled, 'saved: nothing left unsaved').toBe(true))
    expect((within(cardEl()!).getByRole('textbox', { name: 'CNTY' }) as HTMLInputElement).value).toBe('MA,ESSEX')
    fireEvent.change(within(cardEl()!).getByRole('textbox', { name: 'CNTY' }), { target: { value: 'MA,SUFFOLK' } })
    save(cardEl()!)
    await waitFor(() => expect(api.editQsoById).toHaveBeenCalledTimes(2))
    expect(vi.mocked(api.editQsoById).mock.calls[1][2], 'made against the value saved').toEqual([
      { tag: 'CNTY', old: 'MA,ESSEX', new: 'MA,SUFFOLK' },
    ])
  })

  it('a multi-line field is edited in a multi-line box, and keeps its line breaks as written', async () => {
    engine.log = (engine.log as LoggedQso[]).map((r) =>
      r.id === 'id-dl' ? { ...r, extra: [...(r.extra ?? []), ['ADDRESS', '1 Main St\r\nTown'] as [string, string]] } : r,
    )
    engine.revision += 1
    await mounted()
    const card = await openCard()
    fireEvent.click(within(card).getByText(t('contactCard.more')))
    const address = within(card).getByRole('textbox', { name: 'ADDRESS' })
    expect(address.tagName, 'a box that holds lines').toBe('TEXTAREA')
    expect(within(card).getByRole('textbox', { name: 'CNTY' }).tagName, 'a one-line field keeps its one-line box').toBe('INPUT')
    fireEvent.change(address, { target: { value: '1 Main Street\nTown' } })
    save(card)
    await waitFor(() => expect(api.editQsoById).toHaveBeenCalledTimes(1))
    expect(vi.mocked(api.editQsoById).mock.calls[0][2]).toEqual([
      { tag: 'ADDRESS', old: '1 Main St\r\nTown', new: '1 Main Street\r\nTown' },
    ])
  })

  it('a contest contact shows its exchange columns, read only, and refuses a change to one', async () => {
    engine.log = (engine.log as LoggedQso[]).map((r) =>
      r.id === 'id-dl' ? { ...r, contest: { contestId: 'OH-QSO-PARTY', adif: [['CNTY', 'OH,FRANKLIN']] as [string, string][] } } : r,
    )
    engine.revision += 1
    await mounted()
    const card = await openCard()
    fireEvent.click(within(card).getByText(t('contactCard.more')))
    const shown = [...card.querySelectorAll('.contact-card-facts .qso-detail-field')].map((e) => e.textContent)
    expect(shown).toContain('CNTYOH,FRANKLIN')
    fireEvent.change(within(card).getByRole('textbox', { name: 'CNTY' }), { target: { value: 'OH,DELAWARE' } })
    save(card)
    await waitFor(() => expect(within(card).getByRole('alert').textContent).toBe(t('contactCard.error.contest', { tag: 'CNTY' })))
    expect(api.editQsoById).not.toHaveBeenCalled()
  })

  it('an own-station field changed on a contact LoTW has: saved, and the card says it is not sent again', async () => {
    engine.log = (engine.log as LoggedQso[]).map((r) =>
      r.id === 'id-dl' ? { ...r, upload: { lotw: { outcome: 'accepted', whenUnix: 1_700_000_100 } } } : r,
    )
    engine.revision += 1
    await mounted()
    const card = await openCard()
    const note = () => card.querySelector('.contact-card-lotw-note')
    expect(note(), 'nothing changed yet').toBeNull()
    fireEvent.change(box(card, t('logbook.field.myGrid.label')), { target: { value: 'EN53' } })
    expect(note()?.textContent).toBe(t('contactCard.lotwNotResent', { fields: 'MY_GRIDSQUARE' }))
    save(card)
    await waitFor(() => expect(api.editQsoById).toHaveBeenCalledTimes(1))
    expect(vi.mocked(api.editQsoById).mock.calls[0][1].myGrid, 'still saved').toBe('EN53')
  })

  it('shows what the record holds beyond the boxes: QSL sent and its date, the confirmations, upload times, places', async () => {
    engine.log = (engine.log as LoggedQso[]).map((r) =>
      r.id === 'id-dl'
        ? { ...r, upload: { lotw: { outcome: 'accepted', whenUnix: 1_700_200_000 } }, extra: [...(r.extra ?? []), ['CONT', 'EU'] as [string, string], ['DISTANCE', '6870.4'] as [string, string]] }
        : r,
    )
    engine.revision += 1
    await mounted()
    const card = await openCard()
    fireEvent.click(within(card).getByText(t('contactCard.more')))
    const facts = new Map(
      [...card.querySelectorAll('.contact-card-facts .qso-detail-field')].map((e) => [e.querySelector('dt')?.textContent, e.querySelector('dd')?.textContent]),
    )
    const col = (id: string) => columnName(logColumn(id)!)
    expect(facts.get(t('qso.detail.qsl.sent'))).toBe(`B · ${fmtUtc(1_700_100_000)}`)
    expect(facts.get(t('qso.detail.qsl.confirmed'))).toBe(t('qso.detail.qsl.yes'))
    expect(facts.get(t('qso.detail.qsl.awardConfirmed'))).toBe(t('qso.detail.qsl.yes'))
    expect(facts.get(col('upLotw'))).toBe(`${t('logbook.upload.done')} · ${fmtUtc(1_700_200_000)}`)
    expect(facts.get(col('county'))).toBe('X,Y')
    expect(facts.get(col('cqz'))).toBe('14')
    expect(facts.get(col('ituz'))).toBe('28')
    expect(facts.get(col('cont'))).toBe('EU')
    expect(facts.get(col('distance'))).toBe('6870')
  })

  it('Save is not offered until something changed', async () => {
    await mounted()
    const card = await openCard()
    const button = within(card).getByRole('button', { name: t('logbook.form.save') }) as HTMLButtonElement
    expect(button.disabled).toBe(true)
    fireEvent.change(box(card, t('logbook.field.qth.label')), { target: { value: 'Bonn' } })
    expect(button.disabled).toBe(false)
  })

  it('a box an empty value cannot clear says so, and nothing is saved', async () => {
    await mounted()
    const card = await openCard()
    fireEvent.change(box(card, t('logbook.field.myGrid.label')), { target: { value: '' } })
    save(card)
    await waitFor(() =>
      expect(within(card).getByRole('alert').textContent).toBe(
        t('contactCard.error.cannotClear', { label: t('logbook.field.myGrid.label') }),
      ),
    )
    expect(api.editQsoById).not.toHaveBeenCalled()
  })

  it('the contact’s history and its Undo are on the card', async () => {
    vi.mocked(api.qsoHistory).mockResolvedValue([
      { seq: 7, kind: 'edit', source: 'operator', run: 'r7', atMs: 1_700_000_100_000, undoes: null, undoneBy: null, changes: [{ tag: 'NAME', old: null, new: 'Hans' }] },
    ])
    vi.mocked(api.undoQsoChange).mockResolvedValue({
      changed: 1, run: 'u', kept: [], held: [], heldUnder: null, gone: 0, alreadyThere: 0, changedSince: 0, duplicate: 0, unknownKind: 0, changedSinceCalls: [], duplicateCalls: [], owedCalls: [], owedAt: [],
    } as never)
    await mounted()
    const card = await openCard()
    expect(api.qsoHistory).toHaveBeenCalledWith('id-dl')
    fireEvent.click(await within(card).findByRole('button', { name: t('qso.history.undo') }))
    await waitFor(() => expect(api.undoQsoChange).toHaveBeenCalledWith(7))
  })

  it('after an Undo on the card, the card is the contact as the undo left it', async () => {
    vi.mocked(api.qsoHistory).mockResolvedValue([
      { seq: 7, kind: 'edit', source: 'operator', run: 'r7', atMs: 1_700_000_100_000, undoes: null, undoneBy: null, changes: [{ tag: 'QTH', old: 'Bonn', new: 'Koln' }] },
    ])
    vi.mocked(api.undoQsoChange).mockImplementation(async () => {
      engine.log = (engine.log as LoggedQso[]).map((r) => (r.id === 'id-dl' ? { ...r, qth: 'Bonn' } : r))
      engine.revision += 1
      return {
        changed: 1, run: 'u', kept: [], held: [], heldUnder: null, gone: 0, alreadyThere: 0, changedSince: 0, duplicate: 0, unknownKind: 0, changedSinceCalls: [], duplicateCalls: [], owedCalls: [], owedAt: [],
      } as never
    })
    await mounted()
    const card = await openCard()
    expect(box(card, t('logbook.field.qth.label')).value).toBe('Koln')
    fireEvent.click(await within(card).findByRole('button', { name: t('qso.history.undo') }))
    await waitFor(() => expect(box(cardEl()!, t('logbook.field.qth.label')).value).toBe('Bonn'))
  })
})

describe('the recent contacts with the call', () => {
  const listed = (card: HTMLElement) => [...card.querySelectorAll('.contact-card-recent tbody tr')].map((r) => r.textContent ?? '')

  it('the last ten, newest first, this call only, with band, freq, mode, RST and the confirmations', async () => {
    await mounted()
    const card = await openCard()
    await waitFor(() => expect(listed(card)).toHaveLength(10))
    const rows = [...card.querySelectorAll<HTMLElement>('.contact-card-recent tbody tr')]
    expect(rows[0].getAttribute('aria-current'), 'the newest is the one open').toBe('true')
    expect(rows[0].textContent).toContain('2023-11-14 22:13Z')
    expect(rows[1].textContent).toContain('2023-11-14 21:13Z')
    expect(rows[1].textContent).toContain('14.074')
    expect(rows[1].textContent).toContain('-10')
    expect(rows[1].lastElementChild?.textContent, 'confirmed by eQSL').toBe('E')
    expect(rows[2].lastElementChild?.textContent, 'not confirmed').toBe('')
    expect(rows[2].textContent).toContain('40m')
    expect(listed(card).some((r) => r.includes('K1XYZ'))).toBe(false)
  })

  it('clicking one opens that contact on the card', async () => {
    await mounted()
    const card = await openCard()
    await waitFor(() => expect(listed(card)).toHaveLength(10))
    fireEvent.click(card.querySelectorAll('.contact-card-recent tbody tr')[2])
    await waitFor(() => expect(box(cardEl()!, t('logbook.field.band.label')).value).toBe('40m'))
    expect(box(cardEl()!, t('logbook.field.time.label')).value).toBe('20:13:27')
  })

  it('with unsaved changes it asks first: Keep editing stays, Discard goes', async () => {
    await mounted()
    const card = await openCard()
    await waitFor(() => expect(listed(card)).toHaveLength(10))
    fireEvent.change(box(card, t('logbook.field.qth.label')), { target: { value: 'Bonn' } })
    fireEvent.click(card.querySelectorAll('.contact-card-recent tbody tr')[2])
    fireEvent.click(await screen.findByRole('button', { name: t('contactCard.discard.keep') }))
    await waitFor(() => expect(screen.queryByRole('button', { name: t('contactCard.discard.keep') })).toBeNull())
    expect(box(cardEl()!, t('logbook.field.qth.label')).value, 'still the edit').toBe('Bonn')
    fireEvent.click(cardEl()!.querySelectorAll('.contact-card-recent tbody tr')[2])
    fireEvent.click(await screen.findByRole('button', { name: t('contactCard.discard.confirm') }))
    await waitFor(() => expect(box(cardEl()!, t('logbook.field.band.label')).value).toBe('40m'))
  })

  it('a corrected call says which shown values it takes off, and saves only on confirm', async () => {
    await mounted()
    const card = await openCard()
    fireEvent.change(box(card, t('logbook.field.call.label')), { target: { value: 'DL1ABD' } })
    save(card)
    const title = await screen.findByText(t('contactCard.callFix.title', { old: 'DL1ABC', call: 'DL1ABD' }))
    const dialog = title.closest('[role="dialog"], [role="alertdialog"]') as HTMLElement
    const said = dialog.textContent ?? ''
    for (const cid of ['cqz', 'ituz', 'county']) expect(said).toContain(columnName(logColumn(cid)!))
    expect(said).toContain(t('contactCard.callFix.lotw'))
    expect(said, 'eQSL never confirmed it').not.toContain(t('contactCard.callFix.eqsl'))
    fireEvent.click(within(dialog).getByRole('button', { name: t('contactCard.discard.keep') }))
    await new Promise((r) => setTimeout(r, 30))
    expect(api.editQsoById, 'Keep editing saves nothing').not.toHaveBeenCalled()
    save(cardEl()!)
    fireEvent.click(await screen.findByRole('button', { name: t('contactCard.callFix.confirm') }))
    await waitFor(() => expect(api.editQsoById).toHaveBeenCalledTimes(1))
    expect(vi.mocked(api.editQsoById).mock.calls[0][1].call).toBe('DL1ABD')
  })

  it('a save that leaves the call alone asks nothing', async () => {
    await mounted()
    const card = await openCard()
    fireEvent.change(box(card, t('logbook.field.qth.label')), { target: { value: 'Bonn' } })
    save(card)
    await waitFor(() => expect(api.editQsoById).toHaveBeenCalledTimes(1))
    expect(screen.queryByText(t('contactCard.callFix.confirm'))).toBeNull()
  })

  it('Escape with unsaved changes asks too; with none it just closes', async () => {
    await mounted()
    let card = await openCard()
    fireEvent.change(box(card, t('logbook.field.qth.label')), { target: { value: 'Bonn' } })
    fireEvent.keyDown(box(card, t('logbook.field.qth.label')), { key: 'Escape' })
    fireEvent.click(await screen.findByRole('button', { name: t('contactCard.discard.confirm') }))
    await waitFor(() => expect(cardEl()).toBeNull())
    card = await openCard()
    fireEvent.keyDown(box(card, t('logbook.field.qth.label')), { key: 'Escape' })
    await waitFor(() => expect(cardEl()).toBeNull())
    expect(screen.queryByRole('button', { name: t('contactCard.discard.confirm') })).toBeNull()
  })
})

describe('the edit form has a Cancel beside Save (#239)', () => {
  it('Cancel closes the form and saves nothing', async () => {
    await mounted()
    fireEvent.click(within(rowOf('DL1ABC')!).getByRole('button', { name: t('logbook.row.edit', { call: 'DL1ABC' }) }))
    const form = await waitFor(() => document.querySelector('.logbook-form') as HTMLElement)
    expect(form).not.toBeNull()
    const actions = form.querySelector('.logbook-form-actions') as HTMLElement
    fireEvent.click(within(actions).getByRole('button', { name: t('logbook.form.cancel') }))
    await waitFor(() => expect(document.querySelector('.logbook-form')).toBeNull())
    expect(api.editQsoById).not.toHaveBeenCalled()
  })
})

describe('Download confirmations (#348)', () => {
  const button = () => screen.getByRole('button', { name: t('logbook.confirmations.all.label') })
  const settings = (over: object) => ({ lotwUsername: '', eqslUsername: '', qrzLogbookUpload: false, qrzAutoSync: false, ...over })

  it('asks LoTW and eQSL where set up, never the QRZ sync, and says what each did', async () => {
    vi.mocked(api.getSettings).mockResolvedValue(
      settings({ lotwUsername: 'KD9XXX', eqslUsername: 'KD9XXX', qrzLogbookUpload: true, qrzAutoSync: true }) as never,
    )
    vi.mocked(api.downloadLotwReport).mockResolvedValue({ newlyConfirmedAny: 3 } as never)
    vi.mocked(api.downloadEqslReport).mockRejectedValue(new Error('bad password'))
    await mounted()
    await act(async () => fireEvent.click(button()))
    await waitFor(() => expect(pushToast).toHaveBeenCalled())
    expect(api.downloadLotwReport).toHaveBeenCalledTimes(1)
    expect(api.downloadEqslReport).toHaveBeenCalledTimes(1)
    expect(api.syncQrz, 'the QRZ sync is its own button: it can add contacts, not just confirm them').not.toHaveBeenCalled()
    expect(vi.mocked(pushToast).mock.calls[vi.mocked(pushToast).mock.calls.length - 1]?.slice(0, 2)).toEqual([
      [
        t('logbook.confirmations.all.ok', { service: 'LoTW', confirmed: 3 }),
        t('logbook.confirmations.all.failed', { service: 'eQSL', reason: 'bad password' }),
      ].join(' · '),
      'error',
    ])
  })

  it('a download runs once at a time: its own button and Download confirmations each wait for the other', async () => {
    vi.mocked(api.getSettings).mockResolvedValue(settings({ lotwUsername: 'KD9XXX' }) as never)
    let finish: (v: unknown) => void = () => {}
    vi.mocked(api.downloadLotwReport).mockReset().mockImplementation(() => new Promise((r) => (finish = r)) as never)
    await mounted()
    const lotw = () => screen.getByRole('button', { name: t('logbook.fetchLotw.label') }) as HTMLButtonElement
    const all = () => screen.getByRole('button', { name: /^(Download confirmations|Downloading…)$/ }) as HTMLButtonElement
    await act(async () => fireEvent.click(button()))
    await waitFor(() => expect(api.downloadLotwReport).toHaveBeenCalledTimes(1))
    expect(lotw().disabled, 'LoTW’s own button waits for the combined run').toBe(true)
    await act(async () => finish({ newlyConfirmedAny: 0, newlyConfirmed: 0, newlyCredited: 0 }))
    await waitFor(() => expect(lotw().disabled).toBe(false))
    await act(async () => fireEvent.click(lotw()))
    expect(all().disabled, 'the combined run waits for LoTW’s own').toBe(true)
    await act(async () => finish({ newlyConfirmedAny: 0, newlyConfirmed: 0, newlyCredited: 0 }))
    await waitFor(() => expect(all().disabled).toBe(false))
    expect(api.downloadLotwReport).toHaveBeenCalledTimes(2)
  })

  it('with none set up, says where to set one up and asks nothing', async () => {
    vi.mocked(api.getSettings).mockResolvedValue(settings({}) as never)
    vi.mocked(api.downloadLotwReport).mockClear()
    await mounted()
    await act(async () => fireEvent.click(button()))
    await waitFor(() => expect(pushToast).toHaveBeenCalledWith(t('logbook.confirmations.all.none'), 'info', 8000))
    expect(api.downloadLotwReport).not.toHaveBeenCalled()
  })
})

describe('an IOTA reference taken from an offer (operator rulings, 2026-10-27)', () => {
  it('says it is held back from every service, and Confirm takes the mark off and nothing else', async () => {
    const spotted: LoggedQso = {
      ...DL,
      ota: { ...DL.ota!, iota: 'EU-028' },
      extra: [...(DL.extra ?? []), ['APP_NEXUS_OFFER_HELD', 'IOTA']],
    }
    engine.log = [OTHER, spotted, ...Array.from({ length: 12 }, (_, n) => earlier(n))]
    await mounted()
    const card = await openCard()
    expect(within(card).getByText(t('iota.card.spotHeld'))).toBeTruthy()
    fireEvent.click(within(card).getByRole('button', { name: t('iota.card.confirm') }))
    await waitFor(() => expect(api.editQsoById).toHaveBeenCalledTimes(1))
    const [ref, edit, fields] = vi.mocked(api.editQsoById).mock.calls[0]
    expect(ref.id).toBe('id-dl')
    expect(fields).toEqual([{ tag: 'APP_NEXUS_OFFER_HELD', old: 'IOTA', new: null }])
    expect(edit.ota.iota, 'the reference itself is as stored').toBe('EU-028')
    expect(edit.name, 'and every box is as stored').toBeNull()
  })

  it('one taken from the callbook says so, and is held back the same way', async () => {
    const looked: LoggedQso = {
      ...DL,
      ota: { ...DL.ota!, iota: 'EU-028' },
      extra: [...(DL.extra ?? []), ['APP_NEXUS_OFFER_HELD', 'IOTA'], ['APP_NEXUS_CALLBOOK_FILLED', 'IOTA,NAME']],
    }
    engine.log = [OTHER, looked, ...Array.from({ length: 12 }, (_, n) => earlier(n))]
    await mounted()
    const card = await openCard()
    expect(within(card).getByText(t('iota.card.callbookHeld'))).toBeTruthy()
    expect(within(card).queryByText(t('iota.card.spotHeld'))).toBeNull()
    expect(within(card).getByRole('button', { name: t('iota.card.confirm') })).toBeTruthy()
  })

  it('an older contact whose IOTA the callbook filled shows no such note: it was never held', async () => {
    const old: LoggedQso = {
      ...DL,
      ota: { ...DL.ota!, iota: 'EU-028' },
      extra: [...(DL.extra ?? []), ['APP_NEXUS_CALLBOOK_FILLED', 'IOTA']],
    }
    engine.log = [OTHER, old, ...Array.from({ length: 12 }, (_, n) => earlier(n))]
    await mounted()
    const card = await openCard()
    expect(within(card).queryByText(t('iota.card.callbookHeld'))).toBeNull()
    expect(within(card).queryByRole('button', { name: t('iota.card.confirm') })).toBeNull()
  })

  it('a reference the operator typed shows no such note', async () => {
    const typed: LoggedQso = { ...DL, ota: { ...DL.ota!, iota: 'EU-028' } }
    engine.log = [OTHER, typed, ...Array.from({ length: 12 }, (_, n) => earlier(n))]
    await mounted()
    const card = await openCard()
    expect(within(card).queryByText(t('iota.card.spotHeld'))).toBeNull()
    expect(within(card).queryByRole('button', { name: t('iota.card.confirm') })).toBeNull()
  })
})
