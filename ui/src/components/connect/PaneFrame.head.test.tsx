// @vitest-environment jsdom
//
// A CONNECT PANE'S HEAD MAKES ROOM FOR ITS ⋯ — computed against the real sheets on the rendered
// frame, never a regex over the CSS.
//
// Found in Chrome (the report's census, before and after, five languages): the ⋯ costs a head
// ~22 px, and without room made for it the flex shrink of the head wrapped titles onto a second
// line — 18 heads taller than before, Band Outlook in the default layout among them — and at the
// 200 px column floor pushed ✕ out of the frame in German ("Bandprognose" cannot wrap). The fix is
// two declarations, and this holds both to the frames they are for:
//   · the picker stops at 7em (its closed label repeats the title);
//   · the title may ellipsize a word too long for what is left (min-width 0, overflow hidden,
//     text-overflow ellipsis) while words still wrap between each other.
// And to nothing else: the cockpits' CockpitPaneFrame shares `.pane-head` / `.pane-title`, and its
// heads are not this change's to touch. jsdom lays nothing out, so the geometry — no head taller,
// ✕ inside every frame — is the browser census's, in the report.
import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { chainOf, parseRules, winnerAt, type Rule } from '../../cssCascade'
import { PaneFrame } from './PaneFrame'
import { CockpitPaneFrame } from '../panes/CockpitPaneFrame'
import type { PaneContext } from './paneContext'

const read = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES: Rule[] = parseRules(read('styles.css') + '\n' + read('cockpit-panes.css'))
const win = (el: Element, prop: string) => winnerAt(RULES, 'dark', chainOf(el), prop)?.value ?? null

afterEach(cleanup)

function connectHead() {
  const { container } = render(
    <div className="app">
      <div className="connect" data-rails="both">
        <div className="connect-rail" data-side="right">
          <PaneFrame
            slotId="right2"
            paneId="outlook"
            ctx={{ prop: null, getout: null, selectedCall: null, pathOpen: [], outlookOpen: [] } as unknown as PaneContext}
            onAssign={() => {}}
            onHide={() => {}}
            onTextScale={() => {}}
          />
        </div>
      </div>
    </div>,
  )
  return container
}

describe('a Connect pane head makes room for its ⋯', () => {
  it('the picker stops at 7em', () => {
    const picker = connectHead().querySelector('.pane-frame[data-slot] .pane-pick')!
    expect(picker, 'control: the frame rendered its picker').not.toBeNull()
    expect(win(picker, 'max-width')).toBe('7em')
    expect(win(picker, 'min-width'), 'it still gives up its width first').toBe('0')
  })

  it('the title may ellipsize a word too long for the room left, and still wraps between words', () => {
    const title = connectHead().querySelector('.pane-frame[data-slot] .pane-title')!
    expect(win(title, 'min-width')).toBe('0')
    expect(win(title, 'overflow')).toBe('hidden')
    expect(win(title, 'text-overflow')).toBe('ellipsis')
    expect(win(title, 'white-space'), 'no nowrap: a two-word title still breaks between its words').toBeNull()
  })

  it('touches no cockpit pane head (CockpitPaneFrame shares the classes)', () => {
    const { container } = render(
      <div className="app">
        <CockpitPaneFrame title="Band Activity" paneId="bandActivity">
          <p>x</p>
        </CockpitPaneFrame>
      </div>,
    )
    const title = container.querySelector('.pane-title')!
    expect(title, 'control: the cockpit frame rendered its title').not.toBeNull()
    expect(win(title, 'min-width')).toBeNull()
    expect(win(title, 'text-overflow')).toBeNull()
    expect(win(title, 'overflow')).toBeNull()
  })
})
