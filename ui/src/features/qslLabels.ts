// QSL LABELS — the sheet of address labels the Logbook's "Cards to send" prints: one label per
// card, stuck on the back of a QSL card, naming the station it goes to (and its QSL manager, ADIF
// `QSL_VIA`, when the log holds one), each contact's date, UTC time, band, mode and the report
// sent, the operator's own call and "TNX QSO 73".
//
// Three label stocks, chosen as the ones QSL labels are most often printed on:
//   - Avery 5160 — US Letter, 30 labels (3 × 10) of 2⅝ × 1 in: the US address label every
//     office stocks and most label templates start from.
//   - Avery L7160 — A4, 21 labels (3 × 7) of 63.5 × 38.1 mm: the A4 counterpart, and the
//     roomier label, with space for several contacts with one station.
//   - Dymo 30252 — a roll of single 3½ × 1⅛ in address labels for a thermal label printer; each
//     label prints as its own page. Brother's DK-11201 roll (90 × 29 mm) takes the same layout.
// Each is pure data here, so the maths (where each label sits, how many contacts fit on one) is
// tested without a browser, and one more stock is one more row.
//
// The sheet is a self-contained HTML page: printed by the computer's browser, which honours the
// page size and zero margins it states (`@page`), so the labels land on the stock at 100 %. The
// label TEXT is English by convention — a QSL card is read by a station anywhere — and is a set
// of invariant tokens like the calls, bands and modes on it; the help line around the sheet on
// screen is the operator's language, passed in.
//
// Every value from the log is escaped: a call or a QSL route came from whatever file was imported.

/** Whether a paper card is still to go out for the contact: its ADIF `QSL_SENT` is `Q` (queued) or
 *  `R` (the other station asked for one), kept as written, and it is not marked sent — the log's
 *  own rule (tempo-core `qsl_card_queued`). */
export function cardQueued(q: { qslSent?: { sent: boolean } | null; extra?: [string, string][] | null }): boolean {
  if (q.qslSent?.sent) return false
  const v = q.extra?.find(([k]) => k === 'QSL_SENT')?.[1]?.trim().toUpperCase()
  return v === 'Q' || v === 'R'
}

/** A label stock: the page, the grid of labels on it, and the type size the label is set in. All
 *  lengths in millimetres. */
export interface LabelFormat {
  id: LabelFormatId
  /** The stock's name as its maker prints it on the box. */
  name: string
  pageW: number
  pageH: number
  cols: number
  rows: number
  labelW: number
  labelH: number
  /** The first label's top-left corner on the page. */
  left: number
  top: number
  /** From one label's corner to the next one's, across and down. */
  pitchX: number
  pitchY: number
  /** Space kept clear inside each label's edge (a thermal printer cannot print to the edge). */
  pad: number
  /** The type size, in points. */
  fontPt: number
}

export type LabelFormatId = 'avery5160' | 'averyL7160' | 'dymo30252'

const IN = 25.4

export const LABEL_FORMATS: readonly LabelFormat[] = [
  {
    id: 'avery5160',
    name: 'Avery 5160 (US Letter, 30 per sheet)',
    pageW: 8.5 * IN,
    pageH: 11 * IN,
    cols: 3,
    rows: 10,
    labelW: 2.625 * IN,
    labelH: 1 * IN,
    left: 0.1875 * IN,
    top: 0.5 * IN,
    pitchX: 2.75 * IN,
    pitchY: 1 * IN,
    pad: 1.5,
    fontPt: 7.5,
  },
  {
    id: 'averyL7160',
    name: 'Avery L7160 (A4, 21 per sheet)',
    pageW: 210,
    pageH: 297,
    cols: 3,
    rows: 7,
    labelW: 63.5,
    labelH: 38.1,
    left: 7.21,
    top: 15.15,
    pitchX: 66.04,
    pitchY: 38.1,
    pad: 2,
    fontPt: 8.5,
  },
  {
    id: 'dymo30252',
    name: 'Dymo 30252 / Brother DK-11201 (roll, 1 per label)',
    pageW: 3.5 * IN,
    pageH: 1.125 * IN,
    cols: 1,
    rows: 1,
    labelW: 3.5 * IN,
    labelH: 1.125 * IN,
    left: 0,
    top: 0,
    pitchX: 3.5 * IN,
    pitchY: 1.125 * IN,
    pad: 2.5,
    fontPt: 7.5,
  },
]

export function labelFormat(id: string): LabelFormat {
  return LABEL_FORMATS.find((f) => f.id === id) ?? LABEL_FORMATS[0]
}

/** Labels on one page of `f`. */
export function labelsPerPage(f: LabelFormat): number {
  return f.cols * f.rows
}

/** Where the label at `slot` (0-based, across then down) sits on its page, in millimetres. */
export function labelBox(f: LabelFormat, slot: number): { x: number; y: number; w: number; h: number } {
  const at = slot % labelsPerPage(f)
  const col = at % f.cols
  const row = Math.floor(at / f.cols)
  return { x: f.left + col * f.pitchX, y: f.top + row * f.pitchY, w: f.labelW, h: f.labelH }
}

/** The height of one line of label text, in millimetres (the type size, leaded 15 %). */
export function lineMm(f: LabelFormat): number {
  return f.fontPt * (IN / 72) * 1.15
}

/** The lines every label carries besides its contacts: the station, the column heads, the sender. */
const FIXED_LINES = 3

/** How many contacts one label of `f` holds, at least one. */
export function qsosPerLabel(f: LabelFormat): number {
  const lines = Math.floor((f.labelH - 2 * f.pad) / lineMm(f) + 1e-9)
  return Math.max(1, lines - FIXED_LINES)
}

/** One contact as a label prints it. */
export interface LabelQso {
  call: string
  /** The station's QSL route (ADIF `QSL_VIA`), when the log holds one. */
  via: string | null
  whenUnix: number
  band: string
  mode: string
  /** The report sent. */
  rst: string | null
  /** The call the contact was made under: its `STATION_CALLSIGN`, else the operator's. */
  myCall: string
}

/** One label: a station and the contacts with it it confirms. */
export interface QslLabel {
  call: string
  via: string | null
  myCall: string
  qsos: LabelQso[]
}

/** The labels for `qsos`: sorted by call (the order a QSL bureau sorts cards in), then time. With
 *  `combine`, the contacts with one station — same call, same QSL route, made under the same own
 *  call — share labels, up to `perLabel` on each; without it, one label per contact. */
export function groupLabels(qsos: readonly LabelQso[], perLabel: number, combine: boolean): QslLabel[] {
  const sorted = [...qsos].sort(
    (a, b) => a.call.localeCompare(b.call, 'en') || a.whenUnix - b.whenUnix,
  )
  const labels: QslLabel[] = []
  const per = combine ? Math.max(1, perLabel) : 1
  const key = (q: LabelQso) => `${q.call.toUpperCase()}\u0000${(q.via ?? '').toUpperCase()}\u0000${q.myCall.toUpperCase()}`
  let open: QslLabel | null = null
  let openKey = ''
  for (const q of sorted) {
    if (open && openKey === key(q) && open.qsos.length < per) {
      open.qsos.push(q)
      continue
    }
    open = { call: q.call, via: q.via, myCall: q.myCall, qsos: [q] }
    openKey = key(q)
    labels.push(open)
  }
  return labels
}

/** The pages `count` labels fill on `f`, starting at label `start` (1-based) of the first page —
 *  a part-used sheet goes back in the printer. */
export function pagesNeeded(f: LabelFormat, count: number, start: number): number {
  if (count <= 0) return 0
  return Math.ceil((count + clampStart(f, start) - 1) / labelsPerPage(f))
}

/** `start` held to a label the first page has. */
export function clampStart(f: LabelFormat, start: number): number {
  const n = Math.floor(Number.isFinite(start) ? start : 1)
  return Math.min(Math.max(1, n), labelsPerPage(f))
}

// The label text. Invariant tokens: a QSL card's words, read the world over (see the header).
const TO = 'To'
/** The word before a QSL route (`via EA8MGR`): a QSL card's own token. */
export const VIA = 'via'
const HEADS = ['Date', 'UTC', 'Band', 'Mode', 'RST']
const THANKS = 'TNX QSO 73'

function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`)
}

function two(n: number): string {
  return String(n).padStart(2, '0')
}

/** A contact's UTC date (`2026-10-20`) and time (`1423`), as a QSL card writes them. */
export function labelDateTime(whenUnix: number): { date: string; time: string } {
  const d = new Date(whenUnix * 1000)
  return {
    date: `${d.getUTCFullYear()}-${two(d.getUTCMonth() + 1)}-${two(d.getUTCDate())}`,
    time: `${two(d.getUTCHours())}${two(d.getUTCMinutes())}`,
  }
}

function labelHtml(l: QslLabel): string {
  const via = l.via ? ` <span class="via">${VIA} ${esc(l.via)}</span>` : ''
  const rows = l.qsos
    .map((q) => {
      const { date, time } = labelDateTime(q.whenUnix)
      return `<tr><td>${date}</td><td>${time}</td><td>${esc(q.band)}</td><td>${esc(q.mode)}</td><td>${esc(q.rst ?? '')}</td></tr>`
    })
    .join('')
  return (
    `<div class="to">${TO}: <b>${esc(l.call)}</b>${via}</div>` +
    `<table><thead><tr>${HEADS.map((h) => `<th>${h}</th>`).join('')}</tr></thead><tbody>${rows}</tbody></table>` +
    `<div class="from">${esc(l.myCall)} &middot; ${THANKS}</div>`
  )
}

/** What the page says on screen, around the sheet (never printed), in the operator's language. */
export interface SheetWords {
  title: string
  help: string
  print: string
}

const mm = (n: number) => `${Number(n.toFixed(3))}mm`

/** The whole printable page: `labels` laid out on `f` from label `start` of the first page. */
export function labelSheetHtml(labels: readonly QslLabel[], f: LabelFormat, start: number, words: SheetWords): string {
  const per = labelsPerPage(f)
  const first = clampStart(f, start) - 1
  const pages = pagesNeeded(f, labels.length, start)
  const sheets: string[] = []
  for (let p = 0; p < pages; p++) {
    const cells: string[] = []
    for (let slot = 0; slot < per; slot++) {
      const i = p * per + slot - first
      if (i < 0 || i >= labels.length) continue
      const b = labelBox(f, slot)
      cells.push(
        `<div class="label" style="left:${mm(b.x)};top:${mm(b.y)};width:${mm(b.w)};height:${mm(b.h)}">${labelHtml(labels[i])}</div>`,
      )
    }
    sheets.push(`<div class="sheet">${cells.join('')}</div>`)
  }
  const css = `
@page { size: ${mm(f.pageW)} ${mm(f.pageH)}; margin: 0; }
html, body { margin: 0; padding: 0; }
.sheet { position: relative; width: ${mm(f.pageW)}; height: ${mm(f.pageH)}; overflow: hidden; break-after: page; page-break-after: always; }
.sheet:last-of-type { break-after: auto; page-break-after: auto; }
.label { position: absolute; box-sizing: border-box; padding: ${mm(f.pad)}; overflow: hidden; color: #000;
  font: ${f.fontPt}pt/1.15 Arial, Helvetica, sans-serif; }
.label .to b { font-size: 1.15em; }
.label table { border-collapse: collapse; width: 100%; }
.label th, .label td { text-align: left; padding: 0 1.2mm 0 0; white-space: nowrap; font-weight: normal; }
.label th { font-size: 0.85em; }
.label .from { white-space: nowrap; }
.help { font: 14px/1.4 Arial, Helvetica, sans-serif; max-width: 52em; margin: 16px auto; padding: 0 16px; }
@media screen {
  body { background: #e6e6e6; }
  .sheet { background: #fff; margin: 0 auto 16px; box-shadow: 0 1px 4px rgba(0,0,0,.3); }
  .label { outline: 1px dashed #bbb; }
}
@media print { .help { display: none; } }
`
  return (
    `<!doctype html><html><head><meta charset="utf-8"><title>${esc(words.title)}</title><style>${css}</style></head><body>` +
    `<div class="help"><p>${esc(words.help)}</p><button type="button" onclick="window.print()">${esc(words.print)}</button></div>` +
    sheets.join('') +
    `<script>window.addEventListener('load',function(){setTimeout(function(){window.print()},300)})</script>` +
    `</body></html>`
  )
}
