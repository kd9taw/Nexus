// @vitest-environment jsdom
//
// #346 — A RIGHT-CLICK MENU ON A STATION'S ROW, on the Call Roster and in Band Activity: put the call
// on the watch list or take it off, hide it on this band or every band for an hour, two hours, the
// rest of the UTC day or for good, or hide its country. The keyboard opens the same menu (the Menu
// key, Shift+F10) on the focused row.
//
// What this holds, on both panes:
//   · a right-click and the Menu key each open the menu, headed by the call, and a click in it never
//     selects or works the row underneath;
//   · Add puts exactly that call on the watch list the Settings manager edits; Remove takes it off;
//   · each hide takes the row away on the scope it names and no wider: "on this band" leaves the
//     same call on another band, an hour's hide comes back after the hour, and a hide for good on
//     every band is an entry of the chip's list, where it can be undone;
//   · DISPLAY ONLY — nothing here touches the block list the auto-responder honours.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor, within, act } from '@testing-library/react'
import { OperateRoster } from './OperateRoster'
import { OperateDecodes } from './OperateDecodes'
import { loadWatchlist, saveWatchlist } from '../watchlist'
import { HIDE_CALLS_KEY, loadHideCalls, loadScopedHides } from '../features/hideCalls'
import { COUNTRY_EXCLUDE_ENTITIES_KEY } from '../features/countryExclude'
import type { DecodeRow, Station } from '../types'

vi.mock('../api', () => ({
  getDeclination: vi.fn(() => Promise.resolve(0)),
  openQrzPage: vi.fn(),
  setBlockedCalls: vi.fn(),
}))
import { setBlockedCalls } from '../api'

const SLOT = 100
const station = (call: string, over: Partial<Station> = {}): Station => ({
  call,
  grid: 'EN52',
  snr: -10,
  lastHeardSlot: SLOT,
  heardCount: 1,
  presence: 'active',
  worked: false,
  country: 'United States',
  ...over,
})

function mountRoster(
  props: { band?: string; onSelect?: (c: string) => void; onCall?: (c: string) => void; onToggleIgnore?: (c: string) => void } = {},
) {
  return render(
    <OperateRoster
      stations={[station('W1AW'), station('DL1ABC', { country: 'Fed. Rep. of Germany', grid: 'JO62' })]}
      myGrid="EN52"
      currentSlot={SLOT}
      needByCall={new Map()}
      needAlertsByCall={new Map()}
      band={props.band ?? '20m'}
      feedMode="FT8"
      selectedCall={null}
      onSelect={props.onSelect ?? (() => {})}
      onCall={props.onCall ?? (() => {})}
      onToggleIgnore={props.onToggleIgnore}
    />,
  )
}

const decode = (from: string, freqHz: number, over: Partial<DecodeRow> = {}): DecodeRow => ({
  from,
  snr: -12,
  dtSec: 0.2,
  freqHz,
  message: `CQ ${from} EN52`,
  isCq: true,
  directedToMe: false,
  worked: false,
  tier: 'FT8',
  rv: 0,
  country: 'United States',
  ...over,
})

function mountDecodes(props: { band?: string; onCall?: (c: string) => void; onSelectDecode?: (c: string) => void } = {}) {
  return render(
    <OperateDecodes
      decodes={[decode('W1AW', 1000), decode('DL1ABC', 1500, { country: 'Fed. Rep. of Germany' })]}
      slot={100}
      rxOffsetHz={1200}
      band={props.band ?? '20m'}
      tier="FT8"
      harqRescues={0}
      onCall={props.onCall ?? (() => {})}
      onSelectDecode={props.onSelectDecode}
    />,
  )
}

const rosterRow = (call: string): HTMLElement | null =>
  (screen.queryByText(call)?.closest('[role="row"]') as HTMLElement | null | undefined) ?? null
const decodeRow = (call: string): HTMLElement | null =>
  (screen.queryAllByText(new RegExp(`\\b${call}\\b`))[0]?.closest('.decode-row') as HTMLElement | null | undefined) ?? null

async function openMenu(row: HTMLElement): Promise<HTMLElement> {
  fireEvent.contextMenu(row)
  return screen.findByRole('menu')
}
const item = (menu: HTMLElement, name: RegExp) => within(menu).getByRole('menuitem', { name })
async function choose(row: HTMLElement, ...path: RegExp[]) {
  let menu = await openMenu(row)
  for (const [i, name] of path.entries()) {
    const it = item(menu, name)
    if (i === path.length - 1) {
      fireEvent.click(it)
      return
    }
    fireEvent.keyDown(it, { key: 'ArrowRight' })
    menu = await waitFor(() => {
      const menus = screen.getAllByRole('menu')
      expect(menus.length).toBeGreaterThan(1)
      return menus[menus.length - 1]
    })
  }
}

beforeEach(() => localStorage.clear())
afterEach(() => {
  cleanup()
  vi.useRealTimers()
  vi.clearAllMocks()
})

const PANES = [
  { name: 'Call Roster', mount: mountRoster, row: rosterRow },
  { name: 'Band Activity', mount: mountDecodes, row: decodeRow },
] as const

for (const pane of PANES) {
  describe(`#346 the ${pane.name} row menu`, () => {
    it('a right-click opens it, headed by the call; the Menu key and Shift+F10 open it too', async () => {
      pane.mount()
      const menu = await openMenu(pane.row('W1AW')!)
      expect(menu.textContent).toContain('W1AW')
      fireEvent.keyDown(menu, { key: 'Escape' })
      await waitFor(() => expect(screen.queryByRole('menu')).toBeNull())
      fireEvent.keyDown(pane.row('W1AW')!, { key: 'ContextMenu' })
      expect(await screen.findByRole('menu')).not.toBeNull()
      fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' })
      await waitFor(() => expect(screen.queryByRole('menu')).toBeNull())
      fireEvent.keyDown(pane.row('W1AW')!, { key: 'F10', shiftKey: true })
      expect(await screen.findByRole('menu')).not.toBeNull()
    })

    it('Add puts exactly the call on the watch list, and Remove takes it off again', async () => {
      saveWatchlist([{ id: 'keep', kind: 'dxcc', value: 'Bouvet' }])
      pane.mount()
      await choose(pane.row('W1AW')!, /Add to watch list/)
      expect(loadWatchlist().map((f) => [f.kind, f.value])).toEqual([
        ['dxcc', 'Bouvet'],
        ['call', 'W1AW'],
      ])
      await waitFor(() => expect(screen.queryByRole('menu')).toBeNull())
      await choose(pane.row('W1AW')!, /Remove from watch list/)
      expect(loadWatchlist().map((f) => f.value)).toEqual(['Bouvet'])
    })

    it('hide on this band for an hour takes the row away, leaves the other row, and is a scoped hide', async () => {
      pane.mount()
      await choose(pane.row('W1AW')!, /Hide on 20m/, /For 1 hour/)
      await waitFor(() => expect(pane.row('W1AW')).toBeNull())
      expect(pane.row('DL1ABC'), 'the control row').not.toBeNull()
      const [h] = loadScopedHides()
      expect(h.call).toBe('W1AW')
      expect(h.band).toBe('20m')
      expect(h.until! - Date.now()).toBeGreaterThan(3590_000)
      expect(h.until! - Date.now()).toBeLessThanOrEqual(3600_000)
      // The plain list is untouched: this hide ends by itself.
      expect(loadHideCalls()).toEqual([])
    })

    it('a hide on 20m does not hide the same call on another band', async () => {
      pane.mount()
      await choose(pane.row('W1AW')!, /Hide on 20m/, /Until I bring it back/)
      await waitFor(() => expect(pane.row('W1AW')).toBeNull())
      cleanup()
      pane.mount({ band: '40m' })
      expect(pane.row('W1AW'), 'hidden on 40m by a 20m hide').not.toBeNull()
    })

    it('hide on every band for good is an entry of the chip’s list', async () => {
      pane.mount()
      await choose(pane.row('W1AW')!, /Hide on every band/, /Until I bring it back/)
      await waitFor(() => expect(pane.row('W1AW')).toBeNull())
      expect(localStorage.getItem(HIDE_CALLS_KEY)).toBe('W1AW')
      expect(loadScopedHides()).toEqual([])
    })

    it('hide its country adds the entity to the country exclusion', async () => {
      pane.mount()
      await choose(pane.row('DL1ABC')!, /Hide every station in Fed\. Rep\. of Germany/)
      expect(JSON.parse(localStorage.getItem(COUNTRY_EXCLUDE_ENTITIES_KEY) ?? '[]')).toEqual(['Fed. Rep. of Germany'])
      await waitFor(() => expect(pane.row('DL1ABC')).toBeNull())
    })

    it('never touches the block list the auto-responder honours', async () => {
      pane.mount()
      await choose(pane.row('W1AW')!, /Hide on every band/, /For 2 hours/)
      expect(setBlockedCalls).not.toHaveBeenCalled()
    })
  })
}

describe('#346 the menu is display only and leaves the row’s own gestures alone', () => {
  it('choosing in the Call Roster’s menu neither selects nor works the station', async () => {
    const onSelect = vi.fn()
    const onCall = vi.fn()
    mountRoster({ onSelect, onCall })
    await choose(rosterRow('W1AW')!, /Add to watch list/)
    expect(onSelect).not.toHaveBeenCalled()
    expect(onCall).not.toHaveBeenCalled()
  })

  it('choosing in Band Activity’s menu neither selects nor works the station', async () => {
    const onSelectDecode = vi.fn()
    const onCall = vi.fn()
    mountDecodes({ onSelectDecode, onCall })
    await choose(decodeRow('W1AW')!, /Add to watch list/)
    expect(onSelectDecode).not.toHaveBeenCalled()
    expect(onCall).not.toHaveBeenCalled()
  })

  it('an hour’s hide lifts by itself after the hour', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    mountRoster()
    await choose(rosterRow('W1AW')!, /Hide on 20m/, /For 1 hour/)
    await waitFor(() => expect(rosterRow('W1AW')).toBeNull())
    await act(async () => {
      vi.advanceTimersByTime(3601_000)
    })
    expect(rosterRow('W1AW'), 'still hidden after the hour').not.toBeNull()
  })
})
