// @vitest-environment jsdom
//
// THE ONE RENDERER OF A BOX'S BODY (PaneFrame `PaneBody`): every entry of the shared list
// (features/sharedPanes) draws through it — its full panel, or its one-line state while that has
// nothing to show — and the boards draw the board itself whenever the host lends its wiring.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import type { ReactNode } from 'react'
import type { AppSnapshot, NeedAlert, OtaSpot, SpotRow } from '../../types'

vi.mock('../../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  return {
    ...auto,
    getKpForecast: vi.fn(async () => ({ points: [] })),
    getSolarIndices: vi.fn(async () => ({ days: [] })),
    getOpeningsLog: vi.fn(async () => []),
    getSatellites: vi.fn(async () => null),
    getContests: vi.fn(async () => []),
    getSpectrumRow: vi.fn(() => Promise.reject(new Error('no spectrum here'))),
    getSettings: vi.fn(async () => ({ rotatorModel: 0, rotatorHost: '' })),
    readRotatorState: vi.fn(async () => null),
    getOtaSpots: vi.fn(async (): Promise<OtaSpot[]> => []),
    getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
    parksCount: vi.fn(async () => 0),
    huntedParksCount: vi.fn(async () => 0),
  }
})

import { PaneBody } from './PaneFrame'
import { paneById } from './panes'
import { SHARED_PANES } from '../../features/sharedPanes'
import type { PaneContext } from './paneContext'
import type { PaneId } from '../../features/connectConfig'

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

const ctx = (over: Partial<PaneContext> = {}): PaneContext =>
  ({
    myGrid: 'EN52',
    prop: null,
    scales: null,
    alerts: [],
    muf: [],
    getout: null,
    selectedCall: null,
    pathOpen: [],
    outlookOpen: [],
    needAlerts: [],
    needByCall: new Map(),
    dxpedWindows: new Map(),
    onSelectCall: () => {},
    toggleFocusBand: () => {},
    ...over,
  }) as unknown as PaneContext

const SPOT = {
  call: 'K1CW', entity: 'United States', zone: 5, state: null, band: '20m', freqMhz: 14.025, mode: 'CW', submode: 'CW',
  spotter: 'W3LPL', corroborators: [], ageSecs: 30, comment: 'up 1', licensed: true, spotterLocal: true,
} as unknown as SpotRow

const BOARDS = ['spots', 'pota', 'needed'] as const satisfies readonly PaneId[]

/** The boxes whose panel is a component of its own. The frame never sees that component draw
 *  nothing, so each one draws its box's one line itself. */
const SELF_DRAWN = ['openingsLog', 'chase', 'chaseFeed', 'satPasses', 'contests', 'rotor', 'amp'] as const

/** What `node` draws on its own, settled. */
async function drawn(node: ReactNode): Promise<string> {
  const { container, unmount } = render(<>{node}</>)
  await act(async () => {})
  const html = container.innerHTML
  unmount()
  return html
}

describe('PaneBody — the body of every shared box', () => {
  it('hosts each entry: exactly what its box draws, or its one line exactly when the box draws no panel', async () => {
    // One instant for both draws. The beacons box shows the beacon on the air now, a new one every 10 s, and
    // each box is drawn twice here; a slot change between the two draws read as a mismatch. Frozen 1 ms before a
    // slot change, so a clock that moves again fails this every run, not once in a hundred.
    vi.useFakeTimers({ toFake: ['Date'] })
    vi.setSystemTime(new Date(Date.UTC(2026, 9, 8, 12, 0, 9, 999)))
    const fellBack: string[] = []
    for (const e of SHARED_PANES) {
      const def = paneById(e.pane)!
      const c = ctx()
      const panel = def.expert(c)
      const body = await drawn(<PaneBody pane={e.pane} ctx={c} />)
      if (panel == null) {
        fellBack.push(e.id)
        expect(body, `"${e.id}" returned no panel, so its body is its one line`).toBe(await drawn(<p className="pane-basic">{def.basic(c)}</p>))
      } else {
        expect(body, `"${e.id}" is drawn as its box draws it, and nothing else`).toBe(await drawn(panel))
      }
    }
    // Both halves were read: some boxes fell back (the boards among them, with nothing lent) and some did not.
    expect(fellBack).toEqual(expect.arrayContaining(['spotsBoard', 'pota', 'neededBoard']))
    expect(fellBack.length, 'every box fell back: the panel half was never read').toBeLessThan(SHARED_PANES.length)
    // CONTROL: something that is no box draws nothing.
    expect(await drawn(<PaneBody pane={'nothing' as PaneId} ctx={ctx()} />)).toBe('')
  })

  it('never draws an empty body: a box with nothing to show says so in its one line', async () => {
    // CONTROL: the check can see an empty body — something that is no box draws exactly that.
    expect(await drawn(<PaneBody pane={'nothing' as PaneId} ctx={ctx()} />)).toBe('')
    const blank: string[] = []
    for (const e of SHARED_PANES) if ((await drawn(<PaneBody pane={e.pane} ctx={ctx()} />)) === '') blank.push(e.id)
    expect(blank, 'these boxes drew an empty body').toEqual([])
    // The same words as the box's one line, from the same keys.
    for (const id of SELF_DRAWN) {
      const e = SHARED_PANES.find((x) => x.id === id)!
      const c = ctx()
      expect(await drawn(<PaneBody pane={e.pane} ctx={c} />), `"${id}" says its one line`).toBe(
        await drawn(<p className="pane-basic">{paneById(e.pane)!.basic(c)}</p>),
      )
    }
  })

  it('draws the boards themselves when the host lends their wiring', async () => {
    const lent = ctx({
      spotsFeed: { rows: [SPOT], board: { bandPlan: [], selectedCall: null, myGrid: 'EN52', onSelect: () => {}, onWork: () => {} } },
      otaBoard: { snap: { hunt: null, radio: { dialMhz: 14.285 }, logTick: 1 } as unknown as AppSnapshot, onHunt: () => {}, onSnap: () => {} },
      neededBoard: {
        alerts: [{ call: 'K1CW', entity: 'United States', band: '20m', zone: 5, tags: ['NewBand'], priority: 50, headline: 'New band', mode: 'CW', freqMhz: 14.025 } as NeedAlert],
        bandPlan: [], selectedCall: null, myGrid: 'EN52', onQsy: () => {}, onSelect: () => {}, onWork: () => {},
      },
    })
    for (const pane of BOARDS) {
      const { container, unmount } = render(<PaneBody pane={pane} ctx={lent} />)
      await act(async () => {})
      expect(container.querySelector('.pane-basic'), `"${pane}" fell back to its one line with its board lent`).toBeNull()
      expect(container.childElementCount).toBeGreaterThan(0)
      unmount()
    }
  })
})
