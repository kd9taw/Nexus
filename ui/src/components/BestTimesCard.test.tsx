// @vitest-environment jsdom
//
// The Best times card and the call menus' item: "Best times" is the first choice on the Call Roster /
// Band Activity menu and on a JS8 station row's menu; choosing it opens the card beside the row and
// never reaches the row (no select, no tune, no work); the card and the station card are never open
// together; with no card host in the window, and from See full day, the call goes to the Conditions
// window, which takes it once and only while fresh.
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { BestTimesView } from '../types'

const fx = vi.hoisted(() => ({ opened: [] as string[] }))
vi.mock('../api', async (importOriginal) => {
  const real = await importOriginal<typeof import('../api')>()
  return {
    ...real,
    getBestTimes: vi.fn(
      async (call: string): Promise<BestTimesView> => ({
        state: 'answer',
        call,
        entity: 'Canary Islands',
        basis: { kind: 'grid', from: 'heard', grid: 'IL18' },
        km: 5000,
        bearing: 105,
        engine: 'p533',
        answer: {
          engine: 'p533',
          mode: 'FT8',
          perMode: true,
          now: { kind: 'try', band: '20m', word: 'Good', allDay: true },
          windows: [],
          week: { flags: [] },
          chart: [],
          hours: [],
          nowHour: 0,
          longUsed: false,
        },
      }),
    ),
    openPanelWindow: vi.fn(async (panel: string) => {
      fx.opened.push(panel)
    }),
    qrzLookup: vi.fn(async () => ({ grid: null })),
  }
})

import { BestTimesCardHost, closeBestTimesCard, openBestTimesCard, seeFullDay } from './BestTimesCard'
import { CallCardHost, closeCallCard, openCallCard } from './CallCard'
import { CallRowMenu } from './CallRowMenu'
import { js8CallRowEntries } from './js8ListMenus'
import { handOff, takeHandoff } from '../features/bestTimesAsk'

beforeAll(() => {
  Element.prototype.hasPointerCapture = () => false
  Element.prototype.setPointerCapture = () => {}
  Element.prototype.releasePointerCapture = () => {}
  Element.prototype.scrollIntoView = () => {}
})
beforeEach(() => {
  fx.opened = []
  localStorage.clear()
})
afterEach(() => {
  closeBestTimesCard()
  closeCallCard()
})

const snap = { radio: { operatingMode: 'digital' }, link: { tier: 'FT8' } } as never

describe('the call menus', () => {
  it('put Best times first on the roster menu, and choosing it opens the card and reaches no row', async () => {
    const rowClick = vi.fn()
    render(
      <>
        <BestTimesCardHost />
        <div className="row" onClick={rowClick} onDoubleClick={rowClick} onKeyDown={rowClick}>
          <CallRowMenu call="EA8ABC" band="20m" country="Canary Islands" onClose={() => {}} />
        </div>
      </>,
    )
    const items = await screen.findAllByRole('menuitem')
    expect(items[0].textContent).toBe('Best times')
    fireEvent.click(items[0])
    // The test holds the menu open, and an open Radix menu hides the rest of the page from the
    // accessibility tree; the card is found by its own label instead.
    const card = () => document.querySelector<HTMLElement>('.bt-card[aria-label="Best times for EA8ABC"]')
    await waitFor(() => expect(card()).not.toBeNull())
    expect(rowClick).not.toHaveBeenCalled()
    await waitFor(() => expect(card()!.textContent).toMatch(/Now: try 20m/))
    // The card has no transmit, tune or work control.
    for (const b of card()!.querySelectorAll('button'))
      expect((b.textContent ?? '').trim()).toMatch(/^(FT8|FT4|CW|SSB|Look up|See full day|Use)$/)
  })

  it('put Best times first on a JS8 station row, and on no group row', () => {
    const base = {
      jump: null,
      hasMessages: false,
      onShowInbox: () => {},
      pinned: false,
      onPin: () => {},
      onAdd: () => {},
      canRemove: true,
      onRemove: () => {},
      blocked: false,
      canBlock: true,
      onBlock: () => {},
      clears: [],
    }
    const onBestTimes = vi.fn()
    const station = js8CallRowEntries({ ...base, group: false, onBestTimes })
    expect(station[0]).toMatchObject({ kind: 'item', label: 'Best times' })
    const group = js8CallRowEntries({ ...base, group: true, onBestTimes })
    expect(group.some((e) => e.kind === 'item' && e.label === 'Best times')).toBe(false)
  })
})

describe('one floating card at a time', () => {
  it('opening the Best times card closes the station card, and the other way round', async () => {
    const anchor = document.createElement('button')
    document.body.appendChild(anchor)
    render(
      <>
        <CallCardHost snap={snap} spots={[]} needAlertsByCall={new Map()} />
        <BestTimesCardHost />
      </>,
    )
    act(() => openCallCard('EA8ABC', anchor))
    expect(document.querySelectorAll('.callcard')).toHaveLength(1)
    expect(document.querySelector('.bt-card')).toBeNull()
    act(() => openBestTimesCard('EA8ABC', anchor))
    expect(document.querySelectorAll('.callcard')).toHaveLength(1)
    expect(document.querySelector('.bt-card')).not.toBeNull()
    act(() => openCallCard('EA8ABC', anchor))
    expect(document.querySelector('.bt-card')).toBeNull()
    expect(document.querySelectorAll('.callcard')).toHaveLength(1)
    anchor.remove()
  })
})

describe('See full day', () => {
  it('with no card host, the item goes straight to the Conditions window with the call', async () => {
    openBestTimesCard('ea8abc', document.body)
    await waitFor(() => expect(fx.opened).toEqual(['connect']))
    expect(takeHandoff()).toBe('EA8ABC')
    // Taken once.
    expect(takeHandoff()).toBeNull()
  })

  it('hands the call over, and a stale hand-off is not taken', () => {
    seeFullDay('K1ABC')
    expect(fx.opened).toEqual(['connect'])
    expect(takeHandoff()).toBe('K1ABC')
    const now = Date.now()
    const spy = vi.spyOn(Date, 'now').mockReturnValue(now - 60_000)
    handOff('K1ABC')
    spy.mockRestore()
    expect(takeHandoff()).toBeNull()
  })
})
