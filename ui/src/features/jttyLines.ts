// The JTTY receive window as WSJT-X prints it in its two panes — pure, so the rules are tested
// without a cockpit. WSJT-X 3.3.0-beta4: `JttyReceiveLine::formatted` (the two display options)
// and `MainWindow::renderLine` (All Decodes chronological, QSO Frequency in arrival order).
// No prose here: a decoded line is the station's text and WSJT-X's columns, never translated.
//
// The line itself (`JttyLine.line`: frequency, SNR and the text wrapped at 40) is the engine's,
// byte for byte WSJT-X's; nothing here re-wraps or re-spaces it.

import type { JttyLine } from '../types'

/** WSJT-X's two display options for the JTTY panes, both off by default and both kept between
 *  sessions there (`JTTY_LowerCase`, `JTTY_IncludeTime`). */
export interface JttyLineOptions {
  lowerCase: boolean
  includeTime: boolean
}

/** WSJT-X's "Include Time" label: when the message's first frame started, UTC, `hhmmss`. Qt's
 *  `toString("hhmmss")` drops the milliseconds, so the seconds are truncated, never rounded. */
export function jttyTimeLabel(startUtcMs: number): string {
  const d = new Date(startUtcMs)
  const two = (n: number) => String(n).padStart(2, '0')
  return `${two(d.getUTCHours())}${two(d.getUTCMinutes())}${two(d.getUTCSeconds())}`
}

/** One line as a pane shows it. WSJT-X lower-cases the MESSAGE and then formats it; the rest of
 *  the line is the frequency and SNR columns, digits and spaces, so lower-casing the formatted
 *  line is the same text. The time label goes in front, a space after it, and the continuation
 *  lines keep their two-space indent. */
export function jttyPaneText(line: Pick<JttyLine, 'line' | 'startUtcMs'>, o: JttyLineOptions): string {
  const text = o.lowerCase ? line.line.toLowerCase() : line.line
  return o.includeTime ? `${jttyTimeLabel(line.startUtcMs)} ${text}` : text
}

/** WSJT-X's All Decodes pane: every message received, in the order they STARTED (a message found
 *  late by a retro search still lands where it began), ties by message id. What this station
 *  sent is not among them: WSJT-X shows it in the QSO Frequency pane only. A review's lines
 *  (decode again) follow everything received live, each review after its heading, in the order
 *  they started, as WSJT-X adds a review below its spacer (`insertLineSpacer`). */
export function jttyAllPane(lines: readonly JttyLine[]): JttyLine[] {
  return lines
    .filter((l) => !l.tx)
    .sort((a, b) => (a.review ?? 0) - (b.review ?? 0) || a.startUtcMs - b.startUtcMs || a.id - b.id)
}

/** WSJT-X's QSO Frequency pane: the messages first heard within F Tol of Rx, in the order they
 *  arrived, and what this station sent, as it went out (`updateJttyTransmitDisplay`). */
export function jttyRxPane(lines: readonly JttyLine[]): JttyLine[] {
  return lines.filter((l) => l.rx)
}

/** A pane line cut into runs, each confirmed call a run of its own carrying the call as the
 *  engine spells it. Only a call the two-decode rule confirmed is ever marked (`confirmedCalls`):
 *  a call heard once may be a false decode, and nothing may look it up. */
export function jttySegments(text: string, confirmed: readonly string[]): Array<{ text: string; call?: string }> {
  const calls = new Map(confirmed.map((c) => [c.toUpperCase(), c] as const))
  const out: Array<{ text: string; call?: string }> = []
  let plain = ''
  for (const [token] of text.matchAll(/\s+|\S+/g)) {
    const call = calls.get(token.toUpperCase())
    if (call === undefined) {
      plain += token
      continue
    }
    if (plain) out.push({ text: plain })
    plain = ''
    out.push({ text: token, call })
  }
  if (plain || out.length === 0) out.push({ text: plain })
  return out
}
