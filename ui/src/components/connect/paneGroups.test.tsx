// @vitest-environment jsdom
//
// THE BOX MENU'S GROUPS ARE NAMED BY PURPOSE (the operator's pick, 2026-09-30: "Name by purpose" —
// "for example Bands, Space weather, Activity, Station. Display strings only."). The picker's groups
// were the build tiers that added the boxes ("Panels", "B2", "B3"), so Chase, the flagship, sat under
// "B2". The same groups head both lists a box offers: its picker and ⋯ ▸ Add a tab. The real frame
// and the real menu are rendered; the boxes' own bodies are not under test, so the context is empty.
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { PaneFrame } from './PaneFrame'
import { PANES } from './panes'
import { PANE_IDS, type PaneId } from '../../features/connectConfig'
import type { PaneContext } from './paneContext'

// THE BUDGET (2026-10-09). The slowest case here, "⋯ ▸ Add a tab: the same groups, in the same order, each…", takes
// 0.27 s and 0.30 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

/** The groups, in order, and every box in each, in the registry's order. */
const WANT: Array<[string, PaneId[]]> = [
  ['Bands', ['advisory', 'bandAdvisor', 'bandTiles', 'outlook', 'openings', 'openingsLog', 'bestband', 'beacons', 'bandHours', 'esNowcast']],
  ['Space weather', ['kpOutlook', 'spacewx', 'insights', 'greyline', 'measuredMuf']],
  ['Activity', ['selection', 'getout', 'activity', 'chase', 'chaseFeed', 'satPasses', 'contests', 'spots', 'pota', 'needed']],
  ['Station', ['rotor', 'amp', 'scope', 'clock']],
]
const titleOf = (id: PaneId) => PANES.find((p) => p.id === id)!.title

beforeAll(() => {
  // Radix DropdownMenu measures and captures pointers; jsdom has neither (TopBar.help.test.tsx).
  Element.prototype.hasPointerCapture = () => false
  Element.prototype.setPointerCapture = () => {}
  Element.prototype.releasePointerCapture = () => {}
  Element.prototype.scrollIntoView = () => {}
})
afterEach(cleanup)

function frame() {
  return render(
    <PaneFrame
      slotId="left1"
      paneId="outlook"
      ctx={{ prop: null, getout: null, selectedCall: null, pathOpen: [], outlookOpen: [] } as unknown as PaneContext}
      onAssign={() => {}}
      onTextScale={() => {}}
      addable={PANE_IDS.filter((p) => p !== 'outlook')}
      onAddTab={() => {}}
    />,
  ).container
}

describe('the box menu’s groups are named by purpose', () => {
  it('every box is in exactly one of the four groups, and the census covers the whole registry', () => {
    const listed = WANT.flatMap(([, ids]) => ids)
    expect(new Set(listed).size, 'no box in two groups').toBe(listed.length)
    expect([...listed].sort()).toEqual([...PANE_IDS].sort())
  })

  it('the picker: Bands, Space weather, Activity, Station, each holding its boxes', () => {
    const select = frame().querySelector('.pane-pick') as HTMLSelectElement
    const groups = [...select.querySelectorAll('optgroup')]
    expect(groups.map((g) => g.label)).toEqual(WANT.map(([name]) => name))
    for (const [i, [name, ids]] of WANT.entries())
      expect([...groups[i].querySelectorAll('option')].map((o) => o.value), name).toEqual(ids)
    expect(select.options.length, 'every box is offered once').toBe(PANE_IDS.length)
  })

  it('⋯ ▸ Add a tab: the same groups, in the same order, each holding the boxes it can take', () => {
    const c = frame()
    fireEvent.pointerDown(within(c).getByRole('button', { name: /^Options for / }), { button: 0, ctrlKey: false, pointerType: 'mouse' })
    fireEvent.click(within(screen.getByRole('menu')).getByRole('menuitem', { name: /Add a tab/ }))
    const sub = screen.getAllByRole('menu')[1]
    expect(sub, 'control: the submenu opened').toBeTruthy()
    const heads = [...sub.querySelectorAll('.box-menu-label')].map((l) => l.textContent)
    expect(heads).toEqual(WANT.map(([name]) => name))
    for (const [name, ids] of WANT) {
      const group = [...sub.querySelectorAll('[role="group"]')].find((g) => g.querySelector('.box-menu-label')?.textContent === name)!
      const offered = ids.filter((id) => id !== 'outlook').map(titleOf)
      expect(within(group as HTMLElement).getAllByRole('menuitem').map((m) => m.textContent), name).toEqual(offered)
    }
  })
})
