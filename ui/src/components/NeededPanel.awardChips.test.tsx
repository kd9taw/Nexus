// @vitest-environment jsdom
//
// THE NEEDED BOARD'S WPX AND WAE CHIPS ARE OPT-IN (operator ruling, 2026-10-25). A new-prefix line
// on every headline was tried and reverted: almost every station heard is an unworked WPX prefix,
// so it flooded the board. Now the station sends `wpxNeed` / `waeNeed` only while "Show WPX prefix
// needs" / "Show WAE country needs" is on (both off as they ship), and the board draws a chip for
// each. With them off the board renders exactly as it did before they existed.

import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render, within } from '@testing-library/react'
import { NeededPanel } from './NeededPanel'
import { t } from '../i18n'
import type { NeedAlert } from '../types'

const row = (call: string, extra: Partial<NeedAlert> = {}) =>
  ({
    call, entity: 'Fed. Rep. of Germany', band: '20m', zone: 14, tags: ['NewBand'], priority: 50,
    headline: 'New band — 20m Fed. Rep. of Germany', mode: 'CW', freqMhz: 14.025, ...extra,
  }) as NeedAlert

const board = (alerts: NeedAlert[]) => {
  localStorage.clear()
  sessionStorage.clear()
  const { container, unmount } = render(
    <NeededPanel alerts={alerts} bandPlan={[]} selectedCall={null} onQsy={() => {}} onSelect={() => {}} />,
  )
  const html = container.innerHTML
  const rows = [...container.querySelectorAll<HTMLElement>('[role="row"]')]
  const out = { html, rows }
  unmount()
  return out
}

/** The row for `call`, whatever order the board sorts them in. */
const rowOf = (rows: HTMLElement[], call: string) => rows.find((r) => within(r).queryByText(call))!

const chipsOf = (r: HTMLElement) =>
  [...r.querySelectorAll<HTMLElement>('.np-need .need-chip')].map((c) => c.textContent)

afterEach(cleanup)

describe("the Needed board's opt-in WPX and WAE chips", () => {
  it('shows a WPX chip only on a row the station marked, with its prefix in the tooltip', () => {
    const { rows } = board([row('DL2XX', { wpxNeed: 'DL2' }), row('DL3YY')])
    const [marked, plain] = [rowOf(rows, 'DL2XX'), rowOf(rows, 'DL3YY')]
    expect(chipsOf(plain)).toEqual([t('need.chip.newBand.label')])
    expect(chipsOf(marked)).toEqual([t('need.chip.newBand.label'), 'WPX'])
    const chip = within(marked).getByText('WPX')
    expect(chip.getAttribute('title')).toBe(t('need.chip.wpx.title', { prefix: 'DL2', band: '20m', mode: 'CW' }))
    expect(marked.getAttribute('aria-label')).toMatch(/NewBand WPX$/)
  })

  it('shows a WAE chip only on a row the station marked, and both when both are marked', () => {
    const { rows } = board([row('DL2XX', { waeNeed: 'DL' }), row('DL3YY', { wpxNeed: 'DL3', waeNeed: 'DL' })])
    expect(chipsOf(rowOf(rows, 'DL2XX')).slice(1)).toEqual(['WAE'])
    expect(chipsOf(rowOf(rows, 'DL3YY')).slice(1)).toEqual(['WPX', 'WAE'])
    expect(within(rowOf(rows, 'DL2XX')).getByText('WAE').getAttribute('title')).toBe(
      t('need.chip.wae.title', { country: 'DL', band: '20m', mode: 'CW' }),
    )
  })

  it('with the settings off (no field, or an explicit null) the board is byte-identical', () => {
    const before = board([row('DL2XX'), row('DL3YY', { tags: ['NewEntity'], priority: 100 })])
    const off = board([
      row('DL2XX', { wpxNeed: null, waeNeed: null }),
      row('DL3YY', { tags: ['NewEntity'], priority: 100, wpxNeed: null, waeNeed: null }),
    ])
    expect(off.html).toBe(before.html)
    expect(before.html).not.toMatch(/WPX|WAE/)
    // Positive control: the same board with one chip on is not the same markup.
    const on = board([row('DL2XX', { wpxNeed: 'DL2' }), row('DL3YY', { tags: ['NewEntity'], priority: 100 })])
    expect(on.html).not.toBe(before.html)
  })
})
