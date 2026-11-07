// @vitest-environment jsdom
//
// THE SPOTS VIEW REMEMBERS ITS MODES AND BANDS ACROSS A RESTART. A restart here is what quitting Nexus does to a
// window's storage: sessionStorage is emptied, localStorage is kept, and the board mounts again.
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { cleanup, fireEvent, render } from '@testing-library/react'
import { SpotsPanel, type SpotsPane } from './SpotsPanel'
import { t } from '../i18n'
import type { SpotRow } from '../types'

const spot = (over: Partial<SpotRow>): SpotRow => ({
  call: 'W1AW',
  entity: 'United States',
  zone: 5,
  band: '20m',
  freqMhz: 14.025,
  mode: 'CW',
  spotter: 'K3LR',
  corroborators: [],
  ageSecs: 30,
  comment: '',
  licensed: true,
  ...over,
})

const SPOTS: SpotRow[] = [
  spot({ call: 'K1CW', band: '20m', mode: 'CW', freqMhz: 14.025 }),
  spot({ call: 'K2FT', band: '20m', mode: 'Digital', submode: 'FT8', freqMhz: 14.074 }),
  spot({ call: 'K3SSB', band: '20m', mode: 'Phone', freqMhz: 14.25 }),
  spot({ call: 'K4CW', band: '40m', mode: 'CW', freqMhz: 7.025 }),
]

function board(spots: SpotRow[] = SPOTS, pane?: SpotsPane) {
  return render(<SpotsPanel spots={spots} bandPlan={[]} selectedCall={null} onSelect={() => {}} onWork={() => {}} pane={pane} />).container
}
/** Quit Nexus and start it again: the mounted board goes, and so does the window's session. */
function restart() {
  cleanup()
  sessionStorage.clear()
}
const calls = (root: HTMLElement) => [...root.querySelectorAll('.np-call')].map((c) => c.textContent).sort()
const chips = (root: HTMLElement) => [...root.querySelectorAll<HTMLButtonElement>('.np-filters .np-chip')]
const chip = (root: HTMLElement, name: string) => chips(root).find((c) => c.textContent === name)
const isLit = (root: HTMLElement, name: string) => chip(root, name)?.classList.contains('active')

describe('the Spots view after a restart', () => {
  beforeEach(() => {
    sessionStorage.clear()
    localStorage.clear()
  })
  afterEach(cleanup)

  it('comes back on the modes and band the operator chose', () => {
    const before = board()
    fireEvent.click(chip(before, 'Phone')!)
    fireEvent.click(chip(before, '20m')!)
    expect(calls(before), 'the picks apply').toEqual(['K1CW', 'K2FT'])

    restart()
    const after = board()
    expect(calls(after), 'the picks still apply').toEqual(['K1CW', 'K2FT'])
    expect(isLit(after, 'Phone'), 'the hidden mode is unlit').toBe(false)
    expect(isLit(after, 'CW')).toBe(true)
    expect(isLit(after, 'FT8')).toBe(true)
    expect(isLit(after, '20m'), 'the chosen band is lit').toBe(true)
  })

  it('opens with the drawer on screen, reading Filtered, so a remembered filter is never out of sight', () => {
    fireEvent.click(chip(board(), '20m')!)
    restart()
    const after = board()
    expect(after.querySelector('.np-filters'), 'the drawer is open at launch').not.toBeNull()
    expect(after.querySelector('.np-filter-toggle')?.textContent).toContain(t('spots.filter.toggle.active'))
  })

  it('keeps the hidden-set meaning: a mode first seen after the restart shows', () => {
    const before = board()
    fireEvent.click(chip(before, 'Phone')!)
    restart()
    const after = board([...SPOTS, spot({ call: 'K5RY', mode: 'Digital', submode: 'RTTY', freqMhz: 14.085 })])
    expect(calls(after)).toContain('K5RY')
    expect(isLit(after, 'RTTY')).toBe(true)
    expect(calls(after)).not.toContain('K3SSB')
  })

  it('leaves the search, the sort and the chips that start on as this run only', () => {
    const before = board()
    fireEvent.change(before.querySelector('input[type="search"]')!, { target: { value: 'K1' } })
    fireEvent.click(before.querySelector('.np-th[data-col="call"]')!)
    fireEvent.click(chips(before).find((c) => c.title === t('spots.filter.worked.title'))!)
    fireEvent.click(chips(before).find((c) => c.title === t('spots.filter.local.title'))!)
    restart()
    const after = board()
    expect(after.querySelector<HTMLInputElement>('input[type="search"]')?.value).toBe('')
    expect(after.querySelector('.np-th.active')?.textContent).toContain(t('spots.column.age'))
    expect(chips(after).find((c) => c.title === t('spots.filter.worked.title'))?.getAttribute('aria-pressed')).toBe('true')
    expect(chips(after).find((c) => c.title === t('spots.filter.local.title'))?.getAttribute('aria-pressed')).toBe('true')
  })

  it('Clear is remembered too', () => {
    const before = board()
    fireEvent.click(chip(before, 'Phone')!)
    fireEvent.click(chip(before, '20m')!)
    fireEvent.click(before.querySelector('.np-chip-clear')!)
    restart()
    const after = board()
    expect(isLit(after, 'Phone')).toBe(true)
    expect(isLit(after, '20m')).toBe(false)
  })

  it.each([
    ['not JSON', 'nexus.spots.bands', '{oops'],
    ['not a list', 'nexus.spots.bands', '{"band":"20m"}'],
    ['not a list', 'nexus.spots.hiddenModes', '"CW"'],
    ['items that are not strings', 'nexus.spots.hiddenModes', '[1,null,{"a":1}]'],
    ['items that are not bands', 'nexus.spots.bands', '["banana","","20","<b>"]'],
    ['an empty mode name', 'nexus.spots.hiddenModes', '["","   "]'],
  ])('a stored value that is %s (%s) is dropped, and the board shows everything', (_why, key, stored) => {
    localStorage.setItem(key, stored)
    expect(calls(board())).toEqual(['K1CW', 'K2FT', 'K3SSB', 'K4CW'])
    expect(JSON.parse(localStorage.getItem(key)!), 'the junk is gone from the store').toEqual([])
  })

  it('keeps the good picks beside the junk', () => {
    localStorage.setItem('nexus.spots.bands', '["banana","40m",7,"40m"]')
    const after = board()
    expect(calls(after)).toEqual(['K4CW'])
    expect(JSON.parse(localStorage.getItem('nexus.spots.bands')!)).toEqual(['40m'])
  })

  it('a remembered band with no chip of its own gets one: lit, with the board saying why it is empty, one click from back', () => {
    localStorage.setItem('nexus.spots.bands', '["2m"]')
    const after = board()
    expect(calls(after)).toEqual([])
    expect(isLit(after, '2m'), 'the chip that explains the empty board').toBe(true)
    expect(after.querySelector('.np-empty')?.textContent).toBe(t('spots.empty.filtered'))
    fireEvent.click(chip(after, '2m')!)
    expect(calls(after)).toEqual(['K1CW', 'K2FT', 'K3SSB', 'K4CW'])
  })

  it('a remembered hidden mode that is all the feed holds right now still has its chip', () => {
    localStorage.setItem('nexus.spots.hiddenModes', '["CW"]')
    const only = SPOTS.filter((s) => s.mode === 'CW')
    const after = board(only)
    expect(calls(after)).toEqual([])
    expect(isLit(after, 'CW'), 'the unlit chip that explains the empty board').toBe(false)
    expect(after.querySelector('.np-empty')?.textContent).toBe(t('spots.empty.filtered'))
    fireEvent.click(chip(after, 'CW')!)
    expect(calls(after)).toEqual(['K1CW', 'K4CW'])
  })

  it('a hidden mode not in the feed right now stays hidden, shown as an unlit chip', () => {
    localStorage.setItem('nexus.spots.hiddenModes', '["RTTY"]')
    const after = board()
    expect(isLit(after, 'RTTY')).toBe(false)
    expect(calls(after)).toEqual(['K1CW', 'K2FT', 'K3SSB', 'K4CW'])
  })

  it('moves a value an older build left in the session across, once', () => {
    sessionStorage.setItem('nexus.spots.hiddenModes', '["Phone"]')
    sessionStorage.setItem('nexus.spots.bands', '["20m"]')
    expect(calls(board())).toEqual(['K1CW', 'K2FT'])
    expect(JSON.parse(localStorage.getItem('nexus.spots.hiddenModes')!)).toEqual(['Phone'])
    expect(JSON.parse(localStorage.getItem('nexus.spots.bands')!)).toEqual(['20m'])
    expect(sessionStorage.getItem('nexus.spots.hiddenModes'), 'the old copy is gone').toBeNull()
    expect(sessionStorage.getItem('nexus.spots.bands')).toBeNull()
  })

  it('what is remembered wins over a session copy from an older build', () => {
    localStorage.setItem('nexus.spots.bands', '["40m"]')
    sessionStorage.setItem('nexus.spots.bands', '["20m"]')
    expect(calls(board())).toEqual(['K4CW'])
    expect(sessionStorage.getItem('nexus.spots.bands')).toBeNull()
  })
})

describe('a pane of the board keeps its filters for the run, as before', () => {
  beforeEach(() => {
    sessionStorage.clear()
    localStorage.clear()
  })
  afterEach(cleanup)
  const openDrawer = (root: HTMLElement) => fireEvent.click(root.querySelector('.np-filter-toggle')!)

  it("Connect's box: its copy goes at exit, and neither reads nor writes the view's remembered picks", () => {
    localStorage.setItem('nexus.spots.bands', '["40m"]')
    const before = board(SPOTS, { scope: 'connect' })
    openDrawer(before)
    expect(isLit(before, '40m'), "the view's picks are not the box's").toBe(false)
    fireEvent.click(chip(before, 'Phone')!)
    expect(JSON.parse(sessionStorage.getItem('nexus.spots.hiddenModes.connect')!)).toEqual(['Phone'])
    expect(localStorage.getItem('nexus.spots.hiddenModes.connect')).toBeNull()
    expect(localStorage.getItem('nexus.spots.hiddenModes'), "the view's key is untouched").toBeNull()
    expect(localStorage.getItem('nexus.spots.bands')).toBe('["40m"]')

    restart()
    const after = board(SPOTS, { scope: 'connect' })
    openDrawer(after)
    expect(isLit(after, 'Phone'), 'gone at exit').toBe(true)
  })

  it('a cockpit pane: its band pick goes at exit, and it still opens on its own modes', () => {
    const pane: SpotsPane = { scope: 'phone', modes: ['Phone'], band: '20m' }
    const before = board(SPOTS, pane)
    openDrawer(before)
    fireEvent.click(chip(before, '40m')!)
    expect(JSON.parse(sessionStorage.getItem('nexus.spots.bands.phone')!)).toEqual(['40m'])
    expect(localStorage.getItem('nexus.spots.bands.phone')).toBeNull()

    restart()
    const after = board(SPOTS, pane)
    openDrawer(after)
    expect(isLit(after, '40m'), 'gone at exit').toBe(false)
    expect(calls(after)).toEqual(['K3SSB'])
  })
})
