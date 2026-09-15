// @vitest-environment jsdom
//
// #229 (pa0kgb): Band Activity's Countries picker gains a "Hide whole continents" submenu. The
// hook's own behaviour is pinned in features/countryExclude.continent.test.tsx; this pins the
// picker end to end — the submenu opens from the keyboard, a tick hides every station on that
// continent, the main list keeps its 18, and the chip says what is being hidden.
import { describe, it, expect, beforeAll, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup, within } from '@testing-library/react'
import { OperateDecodes } from './OperateDecodes'
import type { DecodeRow } from '../types'

vi.mock('../api', () => ({
  openQrzPage: vi.fn(),
  getDxccEntityContinents: vi.fn(() =>
    Promise.resolve([
      ['Fed. Rep. of Germany', 'EU'],
      ['France', 'EU'],
      ['Japan', 'AS'],
    ]),
  ),
}))

const decode = (from: string, message: string, freqHz: number, country: string): DecodeRow => ({
  from,
  snr: -12,
  dtSec: 0.2,
  freqHz,
  message,
  isCq: true,
  directedToMe: false,
  worked: false,
  tier: 'FT8',
  rv: 0,
  country,
})

function mount() {
  return render(
    <OperateDecodes
      decodes={[
        decode('DL1ABC', 'CQ DL1ABC JO31', 800, 'Fed. Rep. of Germany'),
        decode('F5XYZ', 'CQ F5XYZ JN18', 1500, 'France'),
        decode('JA1ABC', 'CQ JA1ABC PM95', 2000, 'Japan'),
      ]}
      slot={100}
      rxOffsetHz={1200}
      band="20m"
      tier="FT8"
      harqRescues={0}
      onCall={() => {}}
    />,
  )
}

const showsCall = (call: string) =>
  screen
    .queryAllByRole('option')
    .some((r) => (r.textContent ?? '').includes(call))

beforeAll(() => {
  // Radix Popper observes its elements with a ResizeObserver jsdom lacks.
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})

afterEach(() => {
  cleanup()
  localStorage.clear()
})

describe('Band Activity — hide whole continents (#229)', () => {
  it('ticks Europe from the submenu and hides every European station, and says so', async () => {
    mount()
    // Positive control: all three are on screen before anything is ticked.
    for (const c of ['DL1ABC', 'F5XYZ', 'JA1ABC']) expect(showsCall(c), c).toBe(true)

    fireEvent.keyDown(screen.getByRole('button', { name: /countries/i }), { key: 'Enter' })
    const menu = screen.getByRole('menu')
    // The main list keeps exactly its 18 quick picks; the continents sit one level down.
    expect(within(menu).getAllByRole('menuitemcheckbox')).toHaveLength(18)
    const sub = within(menu).getByRole('menuitem', { name: /hide whole continents/i })
    sub.focus()
    fireEvent.keyDown(sub, { key: 'ArrowRight' })
    fireEvent.click(await screen.findByRole('menuitemcheckbox', { name: 'Europe (EU)' }))

    expect(localStorage.getItem('nexus.decodes.countryExclude.continents')).toBe('["EU"]')
    await vi.waitFor(() => expect(showsCall('DL1ABC')).toBe(false))
    expect(showsCall('F5XYZ'), 'France is in Europe too').toBe(false)
    expect(showsCall('JA1ABC'), 'Asia is untouched').toBe(true)
    expect(screen.getByTestId('od-hidden').textContent).toContain('1 continent hidden')
  })
})
