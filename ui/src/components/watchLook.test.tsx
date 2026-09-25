// @vitest-environment jsdom
//
// ONE LOOK FOR ONE LIST (operator, 2026-09-25: "Lime WATCH everywhere"). A station on the watch list
// is marked by the WATCH tile on the Call Roster, the Stations list and Spots (`WatchTile`), and by
// its `Wanted` need on the Needed board and in the band-activity feed (`NEED_CHIP` / `NEED_VISUALS`).
// Those were two looks for one fact — a lime WATCH on the lists, an amber WANTED on the board. This
// pins the board's chip to the tile itself, rendered, so the two cannot drift apart again: the same
// classes, the same word, the same lime.

import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { WatchTile } from './WatchTile'
import { NeededPanel } from './NeededPanel'
import { NEED_CHIP, NEED_VISUALS } from '../features/needVisuals'
import { newWatchFilter } from '../watchlist'
import { parseRules } from '../cssCascade'
import type { NeedAlert } from '../types'

afterEach(cleanup)

/** The WATCH tile as the lists draw it. */
function tile(): HTMLElement {
  const { container } = render(<WatchTile entry={newWatchFilter('call', 'VP8*')} />)
  return container.querySelector('.need-chip') as HTMLElement
}

/** The watch-list chip on a Needed-board row. */
function boardChip(): { chip: HTMLElement; row: HTMLElement } {
  const row: NeedAlert = {
    call: 'VP8PJ', entity: 'Falkland Islands', band: '20m', zone: 13, tags: ['Wanted', 'NewEntity'],
    priority: 120, headline: 'Watch list · New one — Falkland Islands', mode: 'FT8', freqMhz: null,
  } as NeedAlert
  const { container } = render(
    <NeededPanel alerts={[row]} bandPlan={[]} selectedCall={null} onQsy={() => {}} onSelect={() => {}} />,
  )
  const r = [...container.querySelectorAll('[role="row"]')].find((el) => el.textContent?.includes('VP8PJ')) as HTMLElement
  // The row's FIRST chip is its lead tag, `Wanted`.
  return { chip: r.querySelector('.need-chip') as HTMLElement, row: r }
}

describe('the Needed board marks a watched station with the WATCH tile itself', () => {
  it('the same classes and the same word as the tile on the lists', () => {
    const t = tile()
    const expected = { cls: t.className, text: t.textContent }
    cleanup()
    const { chip, row } = boardChip()
    expect({ cls: chip.className, text: chip.textContent }).toEqual(expected)
    // …and the row takes the tile's colour, as a need-tier row takes its tier's.
    expect(row.classList.contains('need-watch')).toBe(true)
  })

  it('the dense form (the roster, the Stations list) and the band-activity badge use the same word and lime', () => {
    const t = tile()
    expect(NEED_CHIP.Wanted.short).toBe(t.textContent)
    expect(NEED_CHIP.Wanted.label).toBe(t.textContent)
    expect(`need-chip need-${NEED_CHIP.Wanted.cls}`).toBe(t.className)
    expect(NEED_VISUALS.wanted.label).toBe(t.textContent)
    expect(NEED_VISUALS.wanted.cls).toBe('need-watch')
  })

  it('the lime is the tile’s token, in both themes, and the feed’s row tint reads it', () => {
    // Comments stripped first: `parseRules` is brace-aware, not comment-aware.
    const css = readFileSync(resolve(process.cwd(), 'src', 'styles.css'), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')
    const rules = parseRules(css)
    const decl = (selector: string, prop: string) =>
      rules.filter((r) => r.selector === selector).flatMap((r) => r.decls).find((d) => d.prop === prop)?.value
    expect(decl('.need-watch', '--need-color')).toBe('var(--need-watch)')
    expect(decl('.decode-row.need-watch', 'border-left')).toContain('var(--need-watch)')
    expect(decl("[data-theme='dark']", '--need-watch'), 'the dark palette has no lime').toBeTruthy()
    expect(decl("[data-theme='light']", '--need-watch'), 'the light palette has no lime').toBeTruthy()
  })
})
