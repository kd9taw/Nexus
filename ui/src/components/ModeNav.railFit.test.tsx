// @vitest-environment jsdom
//
// EVERY RAIL ITEM KEEPS ITS OWN HEIGHT WHEN THE RAIL RUNS OUT OF ROOM. The rail's section list
// (`.mode-nav-top`) is a bounded column that scrolls once there are more sections than fit. A flex
// item in that column shrinks under the overflow down to its min-height, and a button's
// `min-height` is a declared length, not `auto`, so nothing holds it at its content: once `--touch`
// dropped from 44 to 30 px at Standard (the desktop-sized-controls pass, 2026-11-01), the two bare
// buttons at the top of the list, Phone and CW, were squeezed to 30 px while their icon and label
// still needed ~50. Their content spilled out of both ends of the box (the button centres it), so
// Phone's microphone ran above the scroller's top edge, where a scroll container cannot scroll to,
// and Phone's label ran into CW's icon (the operator's "the phone icon is a bit cutoff", 4K, Midnight).
// The sections further down sat in `.mode-nav-drag` wrappers whose `min-height` is `auto` and so
// never shrank, which is why only the top of the rail showed it.
//
// So: each child of the scroller is held at its content, either by a winning `flex-shrink: 0`
// (or a `flex` shorthand whose shrink is 0) or by an automatic minimum (no declared `min-height`
// other than `auto`, and visible overflow). And the scroller packs from its start edge, so the
// first item begins at the scroll origin rather than being centred or end-packed into the part
// a scroll container cannot reach. Winners are COMPUTED over styles.css + cockpit-panes.css on the
// rendered rail, every theme the cascade resolver sweeps (MODES carries the worst-case skins).
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { ModeNav } from './ModeNav'
import type { FeatureId } from '../features/registry'
import { parseRules, winnerAt, chainOf, MODES, type El } from '../cssCascade'

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))

const ALL_ON = new Proxy({}, { get: () => true }) as Record<FeatureId, boolean>

afterEach(cleanup)

function renderRail() {
  render(
    <ModeNav
      view="operate"
      mode="chat"
      enabled={ALL_ON}
      onSelect={vi.fn()}
      tier="FT8"
      onDigitalMode={vi.fn()}
      onClubBoard={vi.fn()}
      clubBoard
    />,
  )
  const top = document.querySelector('.mode-nav-top')
  expect(top, 'the rail rendered no section list').not.toBeNull()
  return top!
}

/** The shrink factor the cascade gives the chain's element: the winner between `flex` and
 *  `flex-shrink` (they compete as one cascade), or CSS's initial 1. */
function shrinkOf(mode: (typeof MODES)[number], chain: El[]): number {
  const w = winnerAt(RULES, mode, chain, 'flex', 'flex-shrink')
  if (!w) return 1
  if (w.prop === 'flex-shrink') return Number(w.value)
  const v = w.value.trim()
  if (v === 'none') return 0
  if (v === 'auto' || v === 'initial') return 1
  const nums = v.split(/\s+/).filter((t) => /^-?\d*\.?\d+$/.test(t))
  return nums.length >= 2 ? Number(nums[1]) : 1
}

/** Does the element keep its automatic (content) minimum height in a flex column? */
function autoMin(mode: (typeof MODES)[number], chain: El[]): boolean {
  const min = winnerAt(RULES, mode, chain, 'min-height')
  const ov = winnerAt(RULES, mode, chain, 'overflow', 'overflow-y')
  return (!min || min.value === 'auto') && (!ov || ov.value === 'visible')
}

describe('the left rail under overflow', () => {
  it('renders the bare Phone/CW buttons and the wrapped sections the guard is about', () => {
    const top = renderRail()
    const kids = [...top.children]
    // The guard is only as good as its reach: the shapes the rail draws must all be here.
    expect(kids[0].matches('button.mode-btn'), 'Phone is no longer the first, bare item').toBe(true)
    expect(kids.some((k) => k.classList.contains('mode-nav-group'))).toBe(true)
    expect(kids.some((k) => k.classList.contains('mode-nav-drag'))).toBe(true)
    expect(kids.some((k) => k.matches('button.mode-btn') && k !== kids[0])).toBe(true)
  })

  it('no item in the scrolling section list shrinks below its content', () => {
    const top = renderRail()
    const bad: string[] = []
    for (const mode of MODES) {
      for (const kid of top.children) {
        const chain = chainOf(kid)
        if (shrinkOf(mode, chain) === 0 || autoMin(mode, chain)) continue
        const label = kid.querySelector('.mode-label')?.textContent ?? kid.className
        const min = winnerAt(RULES, mode, chain, 'min-height')
        bad.push(`${mode}: ${label} shrinks (min-height ${min?.value} from \`${min?.rule.selector}\`)`)
      }
    }
    expect(bad, bad.slice(0, 8).join('\n')).toEqual([])
  })

  it('the section list packs from its top edge, so the first item starts inside the scroller', () => {
    const top = renderRail()
    for (const mode of MODES) {
      const chain = chainOf(top)
      const ov = winnerAt(RULES, mode, chain, 'overflow', 'overflow-y')
      expect(ov?.value, `${mode}: the section list is no longer the rail's scroller`).toMatch(/auto|scroll/)
      const j = winnerAt(RULES, mode, chain, 'justify-content')
      expect(j?.value ?? 'normal', `${mode}: the section list is packed from \`${j?.rule.selector}\``).toMatch(
        /^(normal|flex-start|start|stretch)$/,
      )
    }
  })
})
