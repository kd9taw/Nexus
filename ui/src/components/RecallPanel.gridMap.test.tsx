// @vitest-environment jsdom
//
// #483: the grid square on a caller's card opens that square on a map in the browser, through the
// app's one external-link opener (`openExternalUrl`), the way the photo opens QRZ. The card's
// text is unchanged ("Ferrol (IN52TK) · Spain"); only the square becomes a button.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, cleanup, fireEvent } from '@testing-library/react'
import { RecallPanel } from './RecallPanel'
import { gridMapUrl } from '../grid'
import { t } from '../i18n'

const api = vi.hoisted(() => ({
  openQrzPage: vi.fn(async () => {}),
  openExternalUrl: vi.fn(async (_url: string) => {}),
  rotatorBearingToCall: vi.fn(() => Promise.reject(new Error('unknownStation'))),
}))
vi.mock('../api', () => api)

afterEach(() => {
  cleanup()
  api.openExternalUrl.mockClear()
})

const hist = { qsos: [], count: 0, workedBefore: false, dupeThisBand: false, lastUnix: null } as never

describe('the caller card’s grid square (#483)', () => {
  it('is a button that opens the square on OpenStreetMap, and the card reads as before', () => {
    const { container } = render(
      <RecallPanel call="EC1DD" band="20m" qth="Ferrol" grid="IN52TK" country="Spain" hist={hist} />,
    )
    expect(container.querySelector('.recall-where')?.textContent).toBe('Ferrol (IN52TK) · Spain')
    const btn = container.querySelector('.recall-where button.recall-grid') as HTMLButtonElement
    expect(btn?.textContent).toBe('IN52TK')
    expect(btn.title).toBe(t('recall.grid.map.title', { grid: 'IN52TK' }))
    fireEvent.click(btn)
    expect(api.openExternalUrl).toHaveBeenCalledWith(gridMapUrl('IN52TK'))
  })

  it('CONTROL: a grid that is not a locator stays plain text', () => {
    const { container } = render(<RecallPanel call="EC1DD" band="20m" grid="XX99" country="Spain" hist={hist} />)
    expect(container.querySelector('.recall-where')?.textContent).toBe('(XX99) · Spain')
    expect(container.querySelector('button.recall-grid')).toBeNull()
  })

  it('no grid, no button', () => {
    const { container } = render(<RecallPanel call="EC1DD" band="20m" qth="Ferrol" country="Spain" hist={hist} />)
    expect(container.querySelector('.recall-where')?.textContent).toBe('Ferrol · Spain')
    expect(container.querySelector('button.recall-grid')).toBeNull()
  })
})
