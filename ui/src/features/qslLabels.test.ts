// QSL labels: where each label of the three stocks sits, how many contacts one holds, how contacts
// group onto labels, and the printable page — pure maths and markup, held to the stocks' published
// dimensions (Avery's templates for 5160 and L7160, Dymo's for 30252).
import { describe, expect, it } from 'vitest'
import {
  LABEL_FORMATS,
  cardQueued,
  clampStart,
  groupLabels,
  labelBox,
  labelDateTime,
  labelFormat,
  labelSheetHtml,
  labelsPerPage,
  pagesNeeded,
  qsosPerLabel,
  type LabelQso,
} from './qslLabels'

const WORDS = { title: 'QSL labels', help: 'Print at 100 %', print: 'Print' }

function qso(call: string, whenUnix: number, extra: Partial<LabelQso> = {}): LabelQso {
  return { call, via: null, whenUnix, band: '20m', mode: 'FT8', rst: '-07', myCall: 'KD9TAW', ...extra }
}

const close = (a: number, b: number) => expect(Math.abs(a - b)).toBeLessThan(0.01)

describe('label stocks', () => {
  it('lays each stock out to its published template, every label on the page', () => {
    const want = {
      avery5160: { per: 30, page: [215.9, 279.4], label: [66.675, 25.4], last: [4.7625 + 2 * 69.85 + 66.675, 12.7 + 9 * 25.4 + 25.4] },
      averyL7160: { per: 21, page: [210, 297], label: [63.5, 38.1], last: [7.21 + 2 * 66.04 + 63.5, 15.15 + 6 * 38.1 + 38.1] },
      dymo30252: { per: 1, page: [88.9, 28.575], label: [88.9, 28.575], last: [88.9, 28.575] },
    } as const
    expect(LABEL_FORMATS.map((f) => f.id)).toEqual(Object.keys(want))
    for (const f of LABEL_FORMATS) {
      const w = want[f.id]
      expect(labelsPerPage(f), f.id).toBe(w.per)
      close(f.pageW, w.page[0])
      close(f.pageH, w.page[1])
      close(f.labelW, w.label[0])
      close(f.labelH, w.label[1])
      // The last label's far corner, and every label inside the page and clear of its neighbours.
      const last = labelBox(f, w.per - 1)
      close(last.x + last.w, w.last[0])
      close(last.y + last.h, w.last[1])
      for (let i = 0; i < w.per; i++) {
        const b = labelBox(f, i)
        expect(b.x, `${f.id} ${i}`).toBeGreaterThanOrEqual(0)
        expect(b.y + b.h, `${f.id} ${i}`).toBeLessThanOrEqual(f.pageH + 1e-9)
        expect(b.x + b.w, `${f.id} ${i}`).toBeLessThanOrEqual(f.pageW + 1e-9)
        if (i % f.cols) expect(b.x).toBeGreaterThanOrEqual(labelBox(f, i - 1).x + f.labelW - 1e-9)
      }
    }
  })

  it('fills a sheet across, then down, and starts the next sheet over', () => {
    const f = labelFormat('avery5160')
    expect(labelBox(f, 1).y).toBe(labelBox(f, 0).y)
    expect(labelBox(f, 3).x).toBe(labelBox(f, 0).x)
    expect(labelBox(f, 3).y).toBeGreaterThan(labelBox(f, 0).y)
    expect(labelBox(f, 30)).toEqual(labelBox(f, 0))
  })

  it('fits as many contacts on a label as its height holds, never none', () => {
    expect(LABEL_FORMATS.map((f) => [f.id, qsosPerLabel(f)])).toEqual([
      ['avery5160', 4],
      ['averyL7160', 6],
      ['dymo30252', 4],
    ])
    expect(qsosPerLabel({ ...labelFormat('avery5160'), labelH: 5 })).toBe(1)
  })

  it('counts the sheets a run takes from a part-used first sheet', () => {
    const f = labelFormat('averyL7160')
    expect(pagesNeeded(f, 0, 1)).toBe(0)
    expect(pagesNeeded(f, 21, 1)).toBe(1)
    expect(pagesNeeded(f, 22, 1)).toBe(2)
    expect(pagesNeeded(f, 1, 21)).toBe(1)
    expect(pagesNeeded(f, 2, 21)).toBe(2)
    expect([clampStart(f, 0), clampStart(f, 99), clampStart(f, Number.NaN), clampStart(f, 3.7)]).toEqual([1, 21, 1, 3])
    expect(clampStart(labelFormat('dymo30252'), 5)).toBe(1)
  })
})

describe('grouping contacts onto labels', () => {
  const qsos = [
    qso('W1AW', 300),
    qso('EA8XYZ', 200),
    qso('W1AW', 100),
    qso('W1AW', 400),
    qso('W1AW', 500, { via: 'K1MGR' }),
  ]

  it('one contact per label, sorted by call then time, when not combined', () => {
    const labels = groupLabels(qsos, 4, false)
    expect(labels.map((l) => [l.call, l.qsos.map((q) => q.whenUnix)])).toEqual([
      ['EA8XYZ', [200]],
      ['W1AW', [100]],
      ['W1AW', [300]],
      ['W1AW', [400]],
      ['W1AW', [500]],
    ])
  })

  it('shares a label among one station’s contacts up to its capacity, never across QSL routes', () => {
    const labels = groupLabels(qsos, 2, true)
    expect(labels.map((l) => [l.call, l.via, l.qsos.map((q) => q.whenUnix)])).toEqual([
      ['EA8XYZ', null, [200]],
      ['W1AW', null, [100, 300]],
      ['W1AW', null, [400]],
      ['W1AW', 'K1MGR', [500]],
    ])
  })

  it('never puts contacts made under two own calls on one label', () => {
    const labels = groupLabels([qso('W1AW', 1), qso('W1AW', 2, { myCall: 'KD9TAW/P' })], 4, true)
    expect(labels.map((l) => l.myCall)).toEqual(['KD9TAW', 'KD9TAW/P'])
  })
})

describe('the printable sheet', () => {
  it('prints every label field, at the stock’s page size with no margin, from the chosen label', () => {
    const f = labelFormat('averyL7160')
    const html = labelSheetHtml(groupLabels([qso('EA8XYZ', 1_760_970_180, { via: 'EA8MGR', rst: '599' })], 6, true), f, 5, WORDS)
    expect(html).toContain('@page { size: 210mm 297mm; margin: 0; }')
    expect(labelDateTime(1_760_970_180)).toEqual({ date: '2025-10-20', time: '1423' })
    for (const s of ['<b>EA8XYZ</b>', 'via EA8MGR', '<td>2025-10-20</td><td>1423</td><td>20m</td><td>FT8</td><td>599</td>', 'KD9TAW &middot; TNX QSO 73']) {
      expect(html).toContain(s)
    }
    // Label 5 is row 2, column 2 of the sheet.
    const b = labelBox(f, 4)
    expect(html).toContain(`left:${Number(b.x.toFixed(3))}mm;top:${Number(b.y.toFixed(3))}mm`)
    expect(html.match(/class="label"/g)?.length).toBe(1)
    expect(html.match(/class="sheet"/g)?.length).toBe(1)
  })

  it('escapes what came from the log', () => {
    const html = labelSheetHtml(groupLabels([qso('<img src=x onerror=alert(1)>', 1, { via: '"&' })], 4, true), labelFormat('avery5160'), 1, WORDS)
    expect(html).not.toContain('<img')
    expect(html).toContain('&#60;img')
    expect(html).toContain('via &#34;&#38;')
  })

  it('a roll prints one label per page', () => {
    const labels = groupLabels([qso('A1A', 1), qso('B1B', 2), qso('C1C', 3)], 4, true)
    const html = labelSheetHtml(labels, labelFormat('dymo30252'), 1, WORDS)
    expect(html.match(/class="sheet"/g)?.length).toBe(3)
    expect(html).toContain('@page { size: 88.9mm 28.575mm; margin: 0; }')
  })
})

describe('cardQueued', () => {
  it('is a QSL_SENT of Q or R on a contact not marked sent', () => {
    const e = (v: string): [string, string][] => [['QSL_SENT', v]]
    expect(cardQueued({ extra: e('Q') })).toBe(true)
    expect(cardQueued({ extra: e(' r ') })).toBe(true)
    expect(cardQueued({ extra: e('N') })).toBe(false)
    expect(cardQueued({ extra: [] })).toBe(false)
    expect(cardQueued({ extra: e('Q'), qslSent: { sent: true } })).toBe(false)
  })
})
