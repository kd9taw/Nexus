// @vitest-environment jsdom
//
// THE SPOTS BOX'S MIDDLE COLUMN SET KEEPS ITS SORTED HEADING WHOLE, computed over the real sheets.
// The board sorts on the age by default and marks the sorted heading with its arrow, so every Spots
// box in the middle set (`data-fit='m'`, then from 360 px) headed its age column "Age ▲" — and Chrome
// measured that 43 px in the column's fixed 40 ("Age…", the arrow gone). The column is now as wide as its heading
// needs, and never under the 40 px it had. jsdom lays nothing out: this is the cascade winner
// (cssCascade.testkit); the widths are the real-browser census's.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import type { SpotRow } from '../../types'
import { css, loadSheets } from '../../cssCascade.testkit'
import { SpotsBox } from './SpotsBox'

const ROW = {
  call: 'K1CW', entity: 'United States', zone: 5, state: null, band: '20m', freqMhz: 14.025, mode: 'CW', submode: 'CW',
  spotter: 'W3LPL', corroborators: [], ageSecs: 30, comment: 'up 1', licensed: true, spotterLocal: true,
} as unknown as SpotRow

const width = { px: 0 }

async function boxAt(px: number): Promise<HTMLElement> {
  width.px = px
  await act(async () => {
    render(
      <div className="pane-body">
        <SpotsBox feed={{ rows: [ROW], board: { bandPlan: [], selectedCall: null, myGrid: 'EN52', onSelect: vi.fn(), onWork: vi.fn() } }} />
      </div>,
    )
  })
  return document.querySelector<HTMLElement>('.cn-spots')!
}

beforeAll(() => loadSheets())
beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
  // jsdom's clientWidth is always 0: shadow it for the box alone.
  Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
    configurable: true,
    get(this: HTMLElement) {
      return this.classList.contains('cn-spots') ? width.px : 0
    },
  })
})
afterEach(() => {
  cleanup()
  delete (HTMLElement.prototype as { clientWidth?: number }).clientWidth
})

describe('the Spots box’s middle column set', () => {
  it('sizes its age column to its heading, the sort arrow included', async () => {
    const box = await boxAt(530) // the narrowest box that shows the middle set
    expect(box.getAttribute('data-fit'), 'control: the middle column set').toBe('m')
    const heading = box.querySelector<HTMLElement>('.np-header [data-col="age"]')!
    expect(heading.textContent, 'control: the age is the sorted heading').toBe('Age ▲')
    const cols = css(box.querySelector('.np-header')!, 'grid-template-columns') ?? ''
    expect(cols, 'the age column is a fixed 40 px, 3 px short of “Age ▲”').toMatch(/^minmax\(40px, max-content\) /)
  })
})
