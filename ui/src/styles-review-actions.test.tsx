// @vitest-environment jsdom
//
// THE REVIEW LISTS KEEP THEIR BUTTONS ON SCREEN.
//
// Check eQSL sent marks, Check park states and Check confirmations render one line per contact in
// `.logconfirm.park-review`, a box capped at 90 % of the window that scrolls, with Cancel and the
// action button after the last line. Measured in real headless Chrome (Chrome 140), 2026-10-14,
// with 5,000 eQSL sent marks: the box was 972px tall and its content 180,234px, so at 1920x1080 the
// buttons sat 179,000px below the window when the list opened, and an operator had to scroll the
// whole list to reach Clear or Cancel. The buttons now hold to the bottom of the box (sticky) on
// the box's own background, so they are on screen at every scroll position.
//
// jsdom lays nothing out, so this file cannot see where the buttons land; the measurement above
// is the proof. What it does is resolve the cascade over the elements the dialog really renders:
// the actions are sticky at the bottom edge of the box that scrolls, and paint what the box
// paints, so lines scrolled under them do not show through.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { EqslSentMarkReview } from './components/EqslSentMarkReview'
import { chainOf, expandWith, parseRules, tokensAt, toRgb, winnerAt, type Mode } from './cssCascade'

vi.mock('./api', () => ({ clearEqslSentMarks: vi.fn(), restoreEqslSentMarks: vi.fn() }))
vi.mock('./toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn() }))

afterEach(cleanup)

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))

function mounted() {
  const rows = Array.from({ length: 3 }, (_, k) => ({ id: `m${k}`, call: `K${k}AA`, whenUnix: 1_760_000_000 - k * 60, band: '20m', mode: 'FT8' }))
  const { container } = render(<EqslSentMarkReview open rows={rows} onClose={() => {}} onChanged={() => {}} />)
  const box = container.querySelector('.logconfirm.park-review')
  const actions = box?.querySelector(':scope > .logconfirm-actions')
  expect(box, 'the dialog must render its box').not.toBeNull()
  expect(actions, 'the actions must be a child of the box that scrolls').not.toBeNull()
  return { box: chainOf(box!), actions: chainOf(actions!) }
}

describe('a review list keeps Cancel and its action on screen', () => {
  it.each(['dark', 'light'] as Mode[])('the actions stick to the bottom of the scrolling box (%s)', (mode) => {
    const { box, actions } = mounted()
    expect(winnerAt(RULES, mode, box, 'overflow-y', 'overflow')?.value, 'the box is the scroller').toBe('auto')
    expect(winnerAt(RULES, mode, actions, 'position')?.value).toBe('sticky')
    // Pinned at the box's border edge, not its content edge: the inset reaches down through the
    // box's bottom padding and the actions' own bottom padding fills it, or a line shows below
    // the buttons.
    const pad = winnerAt(RULES, mode, box, 'padding', 'padding-bottom')?.value
    expect(pad, 'the box must declare its padding').toBeTruthy()
    expect(winnerAt(RULES, mode, actions, 'bottom')?.value).toBe(`calc(-1 * ${pad})`)
    expect(winnerAt(RULES, mode, actions, 'padding', 'padding-bottom')?.value.split(/\s+/).pop()).toBe(pad)
  })

  it.each(['dark', 'light'] as Mode[])('the actions paint the box background (%s)', (mode) => {
    const { box, actions } = mounted()
    const paint = (chain: typeof box) => {
      const w = winnerAt(RULES, mode, chain, 'background', 'background-color')
      expect(w, 'no background wins').not.toBeNull()
      return toRgb(expandWith(tokensAt(RULES, mode, chain), w!.value), [255, 0, 255])
    }
    const under = paint(box)
    expect(under, 'the box must paint an opaque colour').not.toBeNull()
    expect(paint(actions)).toEqual(under)
  })
})
