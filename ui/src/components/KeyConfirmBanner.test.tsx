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
const CLOUDLOG: KeyToConfirm = { radioId: null, radio: '', address: 'https://log.example/', removalFailed: false }
const IC9700: KeyToConfirm = { radioId: 1, radio: 'IC-9700', address: '192.168.1.20', removalFailed: false }
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

  it('offers a key whose removal failed for Remove alone, and says so', async () => {
    mockKeys.mockResolvedValue([
      { ...CLOUDLOG, removalFailed: true },
      { ...IC9700, removalFailed: true },
    ])
    render(<KeyConfirmBanner />)
    await settle()
    const group = screen.getByRole('group', { name: EN['keyConfirm.label'] })
    const cloudlog = within(group).getByText(EN['keyConfirm.notRemoved.cloudlog'])
    const radio = within(group).getByText(fill(EN['keyConfirm.notRemoved.icomLan'], { radio: 'IC-9700' }))
    for (const q of [cloudlog, radio]) {
      const row = q.parentElement as HTMLElement
      expect(within(row).queryByRole('button', { name: EN['keyConfirm.use'] })).toBeNull()
      expect(within(row).getByRole('button', { name: EN['keyConfirm.remove'] })).toBeTruthy()
    }
    expect(within(group).queryByText(EN['keyConfirm.why'])).toBeNull()
    expect(within(group).queryByText(fill(EN['keyConfirm.cloudlog'], { address: 'https://log.example/' }))).toBeNull()

    fireEvent.click(within(radio.parentElement as HTMLElement).getByRole('button', { name: EN['keyConfirm.remove'] }))
    await settle()
    expect(mockAnswer).toHaveBeenCalledWith(1, false)
  })

  it('CONTROL: beside a key held back for its address, that one still offers Use, with the reason', async () => {
    mockKeys.mockResolvedValue([CLOUDLOG, { ...IC9700, removalFailed: true }])
    render(<KeyConfirmBanner />)
    await settle()
    const group = screen.getByRole('group', { name: EN['keyConfirm.label'] })
    expect(within(group).getByText(EN['keyConfirm.why'])).toBeTruthy()
    const cloudlog = within(group).getByText(fill(EN['keyConfirm.cloudlog'], { address: 'https://log.example/' }))
    expect(
      within(cloudlog.parentElement as HTMLElement).getByRole('button', { name: EN['keyConfirm.use'] }),
    ).toBeTruthy()
    expect(within(group).getAllByRole('button', { name: EN['keyConfirm.use'] })).toHaveLength(1)
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

  it('an older ask answering last never brings back a key already answered', async () => {
    mockKeys.mockResolvedValue([CLOUDLOG])
    const { container } = render(<KeyConfirmBanner />)
    await settle()
    let answered = () => {}
    mockAnswer.mockReturnValue(new Promise<void>((resolve) => (answered = resolve)))
    fireEvent.click(screen.getByRole('button', { name: EN['keyConfirm.remove'] }))
    // A routine ask goes out while the Remove is on its way, and answers after the one that
    // follows the Remove.
    let stale = (_: KeyToConfirm[]) => {}
    mockKeys.mockReturnValueOnce(new Promise<KeyToConfirm[]>((resolve) => (stale = resolve)))
    await act(async () => {
      vi.advanceTimersByTime(KEYS_TO_CONFIRM_EVERY_MS)
    })
    mockKeys.mockResolvedValue([])
    answered()
    await settle()
    await settle()
    expect(container.innerHTML).toBe('')
    stale([CLOUDLOG])
    await settle()
    expect(mockKeys).toHaveBeenCalledTimes(3)
    expect(container.innerHTML).toBe('')
  })

  it('an older ask refused last neither hides the newer rows nor stops asking', async () => {
    let refuseOld = (_: Error) => {}
    mockKeys.mockReturnValueOnce(new Promise<KeyToConfirm[]>((_, reject) => (refuseOld = reject)))
    render(<KeyConfirmBanner />)
    mockKeys.mockResolvedValue([IC9700])
    await act(async () => {
      vi.advanceTimersByTime(KEYS_TO_CONFIRM_EVERY_MS)
    })
    await settle()
    const radio = fill(EN['keyConfirm.icomLan'], { radio: 'IC-9700', address: '192.168.1.20' })
    expect(screen.getByText(radio)).toBeTruthy()
    refuseOld(new Error('Credentials are set at the shack only.'))
    await settle()
    expect(screen.getByText(radio)).toBeTruthy()
    await act(async () => {
      vi.advanceTimersByTime(KEYS_TO_CONFIRM_EVERY_MS)
    })
    await settle()
    expect(mockKeys).toHaveBeenCalledTimes(3)
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
