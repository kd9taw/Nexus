// @vitest-environment jsdom
//
// ⊞ PANELS ▸ ARRANGE (layout L3), by itself: the three columns with the panes on screen, the four
// moves as named buttons, what a pinned pane may do, and that a press is one undoable step in the
// REAL panel record (usePanelLayout). Where the moved panes then render is PhoneCockpit.arrange's.
import { useRef } from 'react'
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, within } from '@testing-library/react'
import { ArrangePanes } from './ArrangePanes'
import { PHONE_PANELS, usePanelLayout, type PanelLayoutApi, type PhonePanelId } from '../../features/panelState'
import { placedColumns } from '../../features/panelPlace'

const LABELS: Record<PhonePanelId, string> = {
  scope: 'Scope',
  rigscope: 'Rig scope',
  txmeters: 'TX meters',
  receiver: 'Receiver',
  transmitter: 'Transmitter',
  bandActivity: 'Band Activity',
  voiceKeyer: 'Voice Keyer',
  spots: 'Spots',
  needed: 'Needed',
}
// Phone's region with Spots, Needed and the rig scope strip hidden, as on a stock screen with no
// native scope.
const SHOWN = new Set<PhonePanelId>(['bandActivity', 'voiceKeyer', 'receiver', 'transmitter'])
const shown = (id: PhonePanelId) => SHOWN.has(id)

let api: PanelLayoutApi<PhonePanelId> | null = null
function Host() {
  const panels = usePanelLayout(PHONE_PANELS, 'main')
  const ref = useRef(panels)
  ref.current = panels
  api = panels
  return (
    <>
      <ArrangePanes spec={PHONE_PANELS.arrange!} layout={panels.layout} shown={shown} labels={LABELS} onMove={(id, m) => panels.movePane!(id, m, shown)} />
      <button type="button" onClick={panels.undo}>Undo</button>
      <button type="button" onClick={panels.reset}>Reset</button>
    </>
  )
}

beforeEach(() => {
  localStorage.clear()
  api = null
})
afterEach(cleanup)

const btn = (name: string) => screen.getByRole('button', { name })
const cols = () => placedColumns(PHONE_PANELS.arrange!, api!.layout.place)

describe('⊞ Arrange', () => {
  it('lists the three columns and, in each, the panes on screen, top to bottom', () => {
    render(<Host />)
    const groups = screen.getAllByRole('group').filter((g) => g.className === 'panels-arrange-col')
    expect(groups.map((g) => g.querySelector('.panels-arrange-colhead')!.textContent)).toEqual(['Column 1', 'Column 2', 'Log column'])
    const names = (g: HTMLElement) => [...g.querySelectorAll('.panels-arrange-name')].map((n) => n.textContent)
    expect(names(groups[0])).toEqual(['Band Activity', 'Voice Keyer'])
    expect(names(groups[1])).toEqual(['Receiver', 'Transmitter'])
    expect(names(groups[2]), 'the log form has no entry').toEqual([])
  })

  it('names every move by pane and direction, and disables one that would do nothing', () => {
    render(<Host />)
    expect((btn('Move Band Activity up') as HTMLButtonElement).disabled).toBe(true)
    expect((btn('Move Band Activity to the column on the left') as HTMLButtonElement).disabled).toBe(true)
    expect((btn('Move Band Activity down') as HTMLButtonElement).disabled).toBe(false)
    expect((btn('Move Transmitter down') as HTMLButtonElement).disabled, 'only hidden panes below it').toBe(true)
    expect((btn('Move Receiver to the column on the right') as HTMLButtonElement).disabled).toBe(false)
  })

  it('a move is stored in the record and shown at once; Undo takes it back; Reset gives the stock grouping', () => {
    render(<Host />)
    fireEvent.click(btn('Move Receiver to the column on the left'))
    expect(cols().a).toEqual(['bandActivity', 'voiceKeyer', 'spots', 'receiver'])
    const first = screen.getAllByRole('group').find((g) => g.className === 'panels-arrange-col')!
    expect(within(first).getByText('Receiver')).toBeTruthy()
    fireEvent.click(btn('Undo'))
    expect(api!.layout.place).toBeUndefined()
    fireEvent.click(btn('Move Transmitter up'))
    fireEvent.click(btn('Move Band Activity down'))
    expect(cols().a).toEqual(['voiceKeyer', 'bandActivity', 'spots'])
    fireEvent.click(btn('Reset'))
    expect(api!.layout.place).toBeUndefined()
  })

  it('the voice keyer moves up and down only, and says why', () => {
    render(<Host />)
    expect(screen.queryByRole('button', { name: 'Move Voice Keyer to the column on the left' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Move Voice Keyer to the column on the right' })).toBeNull()
    const up = btn('Move Voice Keyer up')
    const note = document.getElementById(up.getAttribute('aria-describedby')!)
    expect(note?.textContent).toMatch(/stays in its column/i)
    fireEvent.click(up)
    expect(cols().a[0]).toBe('voiceKeyer')
  })

  // A press from the keyboard is a focused button's click. Chrome drops focus to <body> when the
  // focused button goes disabled or its row is rebuilt; these check where focus lands instead.
  it('a move pressed from a focused button keeps focus with that pane', () => {
    render(<Host />)
    // To the top of its column: ▲ goes disabled, so focus takes ▼.
    const up = btn('Move Transmitter up')
    up.focus()
    fireEvent.click(up)
    expect(cols().b.filter(shown)).toEqual(['transmitter', 'receiver'])
    expect(document.activeElement).toBe(btn('Move Transmitter down'))
    // Into Column 1: the row is rebuilt there and ◀ is disabled, so focus takes ▶ in the new row.
    const left = btn('Move Receiver to the column on the left')
    left.focus()
    fireEvent.click(left)
    expect(cols().a).toContain('receiver')
    expect(document.activeElement).toBe(btn('Move Receiver to the column on the right'))
    // Into Column 2, which has a column to its right: focus stays on ▶, in the rebuilt row.
    const right = btn('Move Band Activity to the column on the right')
    right.focus()
    fireEvent.click(right)
    expect(cols().b).toContain('bandActivity')
    expect(document.activeElement).toBe(btn('Move Band Activity to the column on the right'))
  })

  it('a press from a button that does not have focus leaves focus where it is', () => {
    render(<Host />)
    const undo = btn('Undo')
    undo.focus()
    fireEvent.click(btn('Move Transmitter up'))
    expect(cols().b.filter(shown)).toEqual(['transmitter', 'receiver'])
    expect(document.activeElement).toBe(undo)
  })
})
