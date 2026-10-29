// @vitest-environment jsdom
// The notice for saved keys held back until the operator confirms them. It asks only in the
// desktop app; with nothing waiting it says nothing; with keys waiting it asks about each one, by
// address, in the catalog's words, and Use and Remove answer for that key alone; on a computer
// that is refused the question it says nothing and stops asking.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import type { KeyToConfirm } from '../api'
import { EN } from '../i18n/en'

let desktop = true
const mockKeys = vi.fn<() => Promise<KeyToConfirm[]>>()
const mockAnswer = vi.fn<(radioId: number | null, useIt: boolean) => Promise<void>>()
const mockShown = vi.fn<(address: string) => void>()
vi.mock('../api', () => ({
  isTauri: () => desktop,
  keysToConfirm: () => mockKeys(),
  answerSavedKey: (radioId: number | null, useIt: boolean, address: string) => {
    mockShown(address)
    return mockAnswer(radioId, useIt)
  },
}))
import { KeyConfirmBanner, KEYS_TO_CONFIRM_EVERY_MS } from './KeyConfirmBanner'

const fill = (text: string, values: Record<string, string>) =>
  text.replace(/\{\{(\w+)\}\}/g, (_, name: string) => values[name] ?? '')
const CLOUDLOG: KeyToConfirm = { radioId: null, radio: '', address: 'https://log.example/' }
const IC9700: KeyToConfirm = { radioId: 1, radio: 'IC-9700', address: '192.168.1.20' }
const settle = async () => {
  await act(async () => {
    await Promise.resolve()
  })
}

beforeEach(() => {
  vi.useFakeTimers()
  desktop = true
  mockKeys.mockReset()
  mockAnswer.mockReset()
  mockShown.mockReset()
  mockAnswer.mockResolvedValue(undefined)
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('KeyConfirmBanner', () => {
  it('asks nothing and shows nothing outside the desktop app', async () => {
    desktop = false
    mockKeys.mockResolvedValue([CLOUDLOG])
    const { container } = render(<KeyConfirmBanner />)
    await settle()
    expect(mockKeys).not.toHaveBeenCalled()
    expect(container.innerHTML).toBe('')
  })

  it('shows nothing while no key waits', async () => {
    mockKeys.mockResolvedValue([])
    const { container } = render(<KeyConfirmBanner />)
    await settle()
    expect(mockKeys).toHaveBeenCalledTimes(1)
    expect(container.innerHTML).toBe('')
  })

  it('asks about each waiting key by its address, and Use and Remove answer for that key', async () => {
    mockKeys.mockResolvedValue([CLOUDLOG, IC9700])
    render(<KeyConfirmBanner />)
    await settle()
    const group = screen.getByRole('group', { name: EN['keyConfirm.label'] })
    expect(within(group).getByText(EN['keyConfirm.why'])).toBeTruthy()
    const cloudlog = within(group).getByText(fill(EN['keyConfirm.cloudlog'], { address: 'https://log.example/' }))
    const radio = within(group).getByText(
      fill(EN['keyConfirm.icomLan'], { radio: 'IC-9700', address: '192.168.1.20' }),
    )
    const row = (q: HTMLElement) => q.parentElement as HTMLElement

    mockKeys.mockResolvedValue([IC9700])
    fireEvent.click(within(row(cloudlog)).getByRole('button', { name: EN['keyConfirm.use'] }))
    await settle()
    await settle()
    expect(mockAnswer).toHaveBeenCalledWith(null, true)
    expect(screen.queryByText(fill(EN['keyConfirm.cloudlog'], { address: 'https://log.example/' }))).toBeNull()

    mockKeys.mockResolvedValue([])
    fireEvent.click(within(row(radio)).getByRole('button', { name: EN['keyConfirm.remove'] }))
    await settle()
    await settle()
    expect(mockAnswer).toHaveBeenLastCalledWith(1, false)
    expect(mockAnswer).toHaveBeenCalledTimes(2)
    expect(screen.queryByRole('group')).toBeNull()
  })

  it('answers with the address it showed, so the station can refuse a Use for another', async () => {
    mockKeys.mockResolvedValue([CLOUDLOG, IC9700])
    render(<KeyConfirmBanner />)
    await settle()
    const radio = screen.getByText(fill(EN['keyConfirm.icomLan'], { radio: 'IC-9700', address: '192.168.1.20' }))
    fireEvent.click(within(radio.parentElement as HTMLElement).getByRole('button', { name: EN['keyConfirm.use'] }))
    await settle()
    expect(mockAnswer).toHaveBeenCalledWith(1, true)
    expect(mockShown).toHaveBeenCalledWith('192.168.1.20')
  })

  it('asks again while it shows nothing, so a restore made later is seen', async () => {
    mockKeys.mockResolvedValue([])
    render(<KeyConfirmBanner />)
    await settle()
    mockKeys.mockResolvedValue([CLOUDLOG])
    await act(async () => {
      vi.advanceTimersByTime(KEYS_TO_CONFIRM_EVERY_MS)
    })
    await settle()
    expect(mockKeys).toHaveBeenCalledTimes(2)
    expect(screen.getByRole('group', { name: EN['keyConfirm.label'] })).toBeTruthy()
  })

  it('on a computer refused the question, shows nothing and stops asking', async () => {
    mockKeys.mockRejectedValue(new Error('Credentials are set at the shack only.'))
    const { container } = render(<KeyConfirmBanner />)
    await settle()
    await act(async () => {
      vi.advanceTimersByTime(KEYS_TO_CONFIRM_EVERY_MS * 3)
    })
    await settle()
    expect(container.innerHTML).toBe('')
    expect(mockKeys).toHaveBeenCalledTimes(1)
  })
})
