// @vitest-environment jsdom
//
// ⊞ PANELS ▸ ARRANGE (layout L3), by itself: the three columns with the panes on screen, the four
// moves as named buttons, what a pinned pane may do, and that a press is one undoable step in the
// REAL panel record (usePanelLayout). Where the moved panes then render is PhoneCockpit.arrange's.
// THE LEFT SIDE (2026-10-03): Phone's fourth place, first in the menu, with and without room for it.
import { useRef } from 'react'
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup, within } from '@testing-library/react'
import { ArrangePanes } from './ArrangePanes'
import { boxLabels } from './CockpitBox'
import { BOX_IDS, OPERATE_ARRANGE, OPERATE_PANELS, PHONE_PANELS, boxEntries, placeOf, usePanelLayout, type OperatePanelId, type PanelLayoutApi, type PhonePanelId } from '../../features/panelState'
import { placedColumns } from '../../features/panelPlace'
import { SHARED_PANES } from '../../features/sharedPanes'
import { t } from '../../i18n'

// THE BUDGET (2026-10-09). The slowest case here, "with six on screen every one is disabled, and says why", takes
// 0.10 s and 0.54 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

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
  ...boxLabels({}),
}
// Phone's region with Spots, Needed and the rig scope strip hidden, as on a stock screen with no
// native scope.
const SHOWN = new Set<PhonePanelId>(['bandActivity', 'voiceKeyer', 'receiver', 'transmitter'])
const shown = (id: PhonePanelId) => SHOWN.has(id)

let api: PanelLayoutApi<PhonePanelId> | null = null
function Host({ sideRoom = false }: { sideRoom?: boolean }) {
  const panels = usePanelLayout(PHONE_PANELS, 'main')
  const ref = useRef(panels)
  ref.current = panels
  api = panels
  return (
    <>
      <ArrangePanes spec={PHONE_PANELS.arrange!} layout={panels.layout} shown={shown} labels={LABELS} sideRoom={sideRoom} onMove={(id, m) => panels.movePane!(id, m, shown, sideRoom)} />
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
const last = <T,>(xs: readonly T[]): T | undefined => xs[xs.length - 1]
const cols = () => placedColumns(PHONE_PANELS.arrange!, api!.layout.place)
const groups = () => screen.getAllByRole('group').filter((g) => g.className === 'panels-arrange-col')
const group = (head: string) => groups().find((g) => g.querySelector('.panels-arrange-colhead')!.textContent === head)!
const names = (g: HTMLElement) => [...g.querySelectorAll('.panels-arrange-name')].map((n) => n.textContent)

describe('⊞ Arrange', () => {
  it('lists the left side and the three columns and, in each, the panes on screen, top to bottom', () => {
    render(<Host />)
    expect(groups().map((g) => g.querySelector('.panels-arrange-colhead')!.textContent)).toEqual(['Left side', 'Column 1', 'Column 2', 'Log column'])
    expect(names(groups()[0]), 'nothing is on the left side yet').toEqual([])
    expect(names(groups()[1])).toEqual(['Band Activity', 'Voice Keyer'])
    expect(names(groups()[2])).toEqual(['Receiver', 'Transmitter'])
    expect(names(groups()[3]), 'the log form has no entry').toEqual([])
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
    // At the column's foot: after the boxes too, which stand hidden at the foot of column 1.
    expect(cols().a).toEqual(['bandActivity', 'voiceKeyer', 'spots', ...BOX_IDS, 'receiver'])
    expect(within(group('Column 1')).getByText('Receiver')).toBeTruthy()
    fireEvent.click(btn('Undo'))
    expect(api!.layout.place).toBeUndefined()
    fireEvent.click(btn('Move Transmitter up'))
    fireEvent.click(btn('Move Band Activity down'))
    expect(cols().a).toEqual(['voiceKeyer', 'bandActivity', 'spots', ...BOX_IDS])
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

describe('⊞ Arrange ▸ Left side (2026-10-03)', () => {
  it('with room for it: ◀ in Column 1 puts a listed pane there, and only a listed one', () => {
    render(<Host sideRoom />)
    // Its three feeds by name, and the boxes as one: "Box" six times would be a list of nothing.
    expect(group('Left side').textContent, 'the side says what it takes').toMatch(/Band Activity, Spots, Needed,? or a box here/)
    const toSide = btn('Move Band Activity to the left side') as HTMLButtonElement
    expect(toSide.disabled).toBe(false)
    // The voice keyer has no ◀ at all (pinned); a rig strip moved into Column 1 has one, disabled.
    expect(screen.queryByRole('button', { name: /Move Voice Keyer to/ })).toBeNull()
    fireEvent.click(btn('Move Receiver to the column on the left'))
    expect((btn('Move Receiver to the column on the left') as HTMLButtonElement).disabled, 'a rig strip cannot go on the left side').toBe(true)
    fireEvent.click(toSide)
    expect(api!.layout.leftSide).toEqual(['bandActivity'])
    expect(names(group('Left side'))).toEqual(['Band Activity'])
    expect(names(group('Column 1')), 'it is listed where it is: on the side, not in its column').toEqual(['Voice Keyer', 'Receiver'])
    // Its place in the columns is kept, for a narrower window and for ▶.
    expect(cols().a).toContain('bandActivity')
  })

  it('on the side: ▲ ▼ among the panes there, ▶ back to its column, and no ◀', () => {
    localStorage.setItem('nexus.panels.phone.main', JSON.stringify({ v: 2, state: {}, share: {}, leftSide: ['bandActivity'] }))
    SHOWN.add('spots')
    try {
      render(<Host sideRoom />)
      fireEvent.click(btn('Move Spots to the left side'))
      expect(api!.layout.leftSide).toEqual(['bandActivity', 'spots'])
      expect(screen.queryByRole('button', { name: 'Move Spots to the left side' }), 'nothing stands left of the side').toBeNull()
      fireEvent.click(btn('Move Spots up'))
      expect(api!.layout.leftSide).toEqual(['spots', 'bandActivity'])
      expect((btn('Move Spots up') as HTMLButtonElement).disabled).toBe(true)
      fireEvent.click(btn('Move Band Activity from the left side back to its column'))
      expect(api!.layout.leftSide).toEqual(['spots'])
      expect(names(group('Column 1'))[0], 'back where it stood').toBe('Band Activity')
      fireEvent.click(btn('Move Spots from the left side back to its column'))
      expect(api!.layout.leftSide, 'an empty side is no entry at all').toBeUndefined()
    } finally {
      SHOWN.delete('spots')
    }
  })

  it('each move is one undoable step, and Reset clears the side with everything else', () => {
    render(<Host sideRoom />)
    fireEvent.click(btn('Move Band Activity to the left side'))
    fireEvent.click(btn('Undo'))
    expect(api!.layout.leftSide).toBeUndefined()
    fireEvent.click(btn('Move Band Activity to the left side'))
    fireEvent.click(btn('Reset'))
    expect(api!.layout.leftSide).toBeUndefined()
  })

  it('too narrow for it: the stored side is named and KEPT — its panes move in their columns, and no move reaches it', () => {
    localStorage.setItem('nexus.panels.phone.main', JSON.stringify({ v: 2, state: {}, share: {}, leftSide: ['bandActivity'] }))
    render(<Host />)
    expect(group('Left side').textContent).toMatch(/Band Activity stands here on a window about 1280 px wide or wider/)
    expect(names(group('Left side')), 'no rows: the panes are listed where they are on screen').toEqual([])
    expect(names(group('Column 1'))).toEqual(['Band Activity', 'Voice Keyer'])
    expect(screen.queryByRole('button', { name: /to the left side|from the left side/ })).toBeNull()
    // Every enabled button of every listed pane, pressed: the side is exactly as stored.
    for (let i = 0; i < 3; i++) {
      for (const b of screen.getAllByRole('button').filter((x) => (x as HTMLButtonElement).dataset.arrange && !(x as HTMLButtonElement).disabled)) {
        fireEvent.click(b)
      }
    }
    expect(api!.layout.place, 'the moves did happen').toBeDefined()
    expect(api!.layout.leftSide).toEqual(['bandActivity'])
    expect(JSON.parse(localStorage.getItem('nexus.panels.phone.main')!).leftSide).toEqual(['bandActivity'])
  })

  it('a move to the side pressed from a focused button keeps focus with that pane, on its ▶ there', () => {
    render(<Host sideRoom />)
    const toSide = btn('Move Band Activity to the left side')
    toSide.focus()
    fireEvent.click(toSide)
    expect(document.activeElement).toBe(btn('Move Band Activity from the left side back to its column'))
  })
})

describe('⊞ Arrange ▸ + Add a box (any pane in any area, 2026-10-07)', () => {
  /** The host a grid cockpit is: the boxes on screen are what the record shows, named for what they show. */
  function BoxHost({ sideRoom = false }: { sideRoom?: boolean }) {
    const panels = usePanelLayout(PHONE_PANELS, 'main')
    api = panels
    const entries = boxEntries(PHONE_PANELS, panels.layout)
    const isShown = (id: PhonePanelId) => SHOWN.has(id) || entries[id] != null
    return (
      <>
        <ArrangePanes
          spec={PHONE_PANELS.arrange!}
          layout={panels.layout}
          shown={isShown}
          labels={{ ...LABELS, ...boxLabels(entries) }}
          sideRoom={sideRoom}
          onMove={(id, m) => panels.movePane!(id, m, isShown, sideRoom)}
          onAddBox={(area) => panels.addBox!(area)}
          boxesFull={BOX_IDS.every((b) => panels.stateOf(b) !== 'removed')}
        />
        <button type="button" onClick={panels.undo}>Undo</button>
      </>
    )
  }
  const title = (i: number) => {
    const e = SHARED_PANES[i]
    return boxLabels({ box1: e.id }).box1
  }
  const add = (area: 'a' | 'b' | 'log' | 'side') => btn(t(`panels.box.add.${area}.aria` as const))

  it('each column has its own, named for it; the box it adds stands at that column’s foot, named for what it shows', () => {
    render(<BoxHost />)
    for (const area of ['a', 'b', 'log'] as const) expect(add(area).textContent).toBe(t('panels.box.add'))
    fireEvent.click(add('b'))
    expect(last(cols().b)).toBe('box1')
    expect(last(names(group('Column 2')))).toBe(title(0))
    fireEvent.click(add('log'))
    expect(names(group('Log column'))).toEqual([title(1)])
    // …and it moves like a pane: ◀ from the log column takes it to column 2's foot.
    fireEvent.click(btn(`Move ${title(1)} to the column on the left`))
    expect(last(cols().b)).toBe('box2')
  })

  it('the left side has one only while the window has room for the side', () => {
    render(<BoxHost />)
    expect(screen.queryByRole('button', { name: t('panels.box.add.side.aria') })).toBeNull()
    cleanup()
    render(<BoxHost sideRoom />)
    fireEvent.click(add('side'))
    expect(api!.layout.leftSide).toEqual(['box1'])
    expect(names(group('Left side'))).toEqual([title(0)])
  })

  it('with six on screen every one is disabled, and says why', () => {
    render(<BoxHost />)
    for (let i = 0; i < 6; i++) fireEvent.click(add('a'))
    for (const area of ['a', 'b', 'log'] as const) {
      const b = add(area) as HTMLButtonElement
      expect(b.disabled, area).toBe(true)
      expect(document.getElementById(b.getAttribute('aria-describedby') ?? '')?.textContent).toBe(t('panels.box.full'))
    }
  })

  it('the sixth add, pressed from a focused button, keeps focus in that column: on the new box’s last live move', () => {
    render(<BoxHost />)
    for (let i = 0; i < 5; i++) fireEvent.click(add('a'))
    const sixth = add('a') as HTMLButtonElement
    sixth.focus()
    fireEvent.click(sixth)
    expect(sixth.disabled).toBe(true)
    const now = document.activeElement as HTMLButtonElement
    expect(now, 'focus fell to the page').not.toBe(document.body)
    expect(now.disabled).toBe(false)
    expect(now.getAttribute('data-arrange')).toMatch(/^box6 /)
    expect(group('Column 1').contains(now)).toBe(true)
  })

  it('an add is one undoable step', () => {
    render(<BoxHost />)
    fireEvent.click(add('a'))
    expect(api!.stateOf('box1')).toBe('docked')
    fireEvent.click(btn('Undo'))
    expect(api!.stateOf('box1')).toBe('removed')
  })

  it('a cockpit that offers no boxes here shows no add at all (the hosted Remote page)', () => {
    render(<Host />)
    expect(screen.queryByRole('button', { name: t('panels.box.add.a.aria') })).toBeNull()
  })
})

describe('⊞ Arrange ▸ a cockpit’s own columns (FT, 2026-10-07)', () => {
  // A cockpit whose columns are not Phone's names them, in their order on screen, and says what a narrower
  // window does: ◀ ▶ follow that order, each column's "+ Add a box" carries its own name, and there is no
  // log form to mention. Drawn over FT's Roster layout with its rail on the left (rail | main column).
  const COLUMNS = [
    { col: 'log' as const, name: 'Side rail', addAria: 'Add a box to the side rail' },
    { col: 'a' as const, name: 'Main column', addAria: 'Add a box to the main column' },
  ]
  const FT_LABELS = { bandActivity: 'Band Activity', callRoster: 'Call Roster', rxfreq: 'Rx Frequency', recall: 'Callsign card', ...boxLabels({}) } as Record<OperatePanelId, string>
  const ftShown = (id: OperatePanelId) => ['bandActivity', 'callRoster', 'rxfreq'].includes(id)
  let ft: PanelLayoutApi<OperatePanelId> | null = null
  function Named() {
    const panels = usePanelLayout(OPERATE_PANELS, 'main')
    ft = panels
    return (
      <ArrangePanes
        spec={OPERATE_ARRANGE.roster}
        layout={{ ...panels.layout, place: placeOf(OPERATE_PANELS, panels.layout, 'roster') }}
        shown={ftShown}
        labels={FT_LABELS}
        onMove={(id, m) => panels.movePane!(id, m, ftShown, false, { layout: 'roster', order: COLUMNS.map((c) => c.col) })}
        onAddBox={(area) => panels.addBox!(area, 'roster')}
        columns={COLUMNS}
        narrow="On a narrower window, the columns stand one above the other."
      />
    )
  }

  it('lists the columns it is given, in that order, with their names and their own add buttons', () => {
    render(<Named />)
    expect(groups().map((g) => g.querySelector('.panels-arrange-colhead')!.textContent)).toEqual(['Side rail', 'Main column'])
    expect(names(group('Side rail'))).toEqual(['Band Activity', 'Rx Frequency'])
    expect(names(group('Main column'))).toEqual(['Call Roster'])
    expect(within(group('Side rail')).getByRole('button', { name: 'Add a box to the side rail' })).toBeTruthy()
    expect(within(group('Main column')).getByRole('button', { name: 'Add a box to the main column' })).toBeTruthy()
    expect(screen.queryByText(t('panels.arrange.logForm')), 'a log form the cockpit does not have').toBeNull()
    expect(screen.getByText('On a narrower window, the columns stand one above the other.')).toBeTruthy()
    expect(screen.queryByText(t('panels.arrange.narrow'))).toBeNull()
  })

  it('◀ ▶ follow the order on screen: the main column’s ◀ goes to the rail beside it, its ▶ nowhere', () => {
    render(<Named />)
    const left = btn(t('panels.arrange.left.aria', { pane: 'Call Roster' })) as HTMLButtonElement
    const right = btn(t('panels.arrange.right.aria', { pane: 'Call Roster' })) as HTMLButtonElement
    expect([left.disabled, right.disabled]).toEqual([false, true])
    fireEvent.click(left)
    expect(placedColumns(OPERATE_ARRANGE.roster, placeOf(OPERATE_PANELS, ft!.layout, 'roster')).log).toContain('callRoster')
    // The button that pressed it stays live (now ▶, back to the main column), and the rail's own ◀ goes nowhere.
    expect((btn(t('panels.arrange.right.aria', { pane: 'Call Roster' })) as HTMLButtonElement).disabled).toBe(false)
    expect((btn(t('panels.arrange.left.aria', { pane: 'Call Roster' })) as HTMLButtonElement).disabled).toBe(true)
  })
})
