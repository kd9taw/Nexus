// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { UpdateBanner } from './UpdateBanner'
import type { SelfUpdate } from '../useSelfUpdate'
import { t } from '../i18n'

function upd(over: Partial<SelfUpdate> = {}): SelfUpdate {
  return {
    phase: 'ready',
    version: '0.17.17',
    blockReason: null,
    progress: null,
    error: null,
    install: vi.fn(),
    dismiss: vi.fn(),
    downloadInstead: vi.fn(),
    ...over,
  }
}

afterEach(cleanup)

describe('UpdateBanner', () => {
  // Downloading happens silently. Asking nothing of the operator until it is actually ready is
  // the whole point — a progress bar for something they did not request is just noise.
  it('stays silent until the update is ready', () => {
    for (const phase of ['idle', 'available', 'downloading'] as const) {
      const { container } = render(<UpdateBanner update={upd({ phase })} />)
      expect(container.querySelector('.update-banner')).toBeNull()
      cleanup()
    }
  })

  it('offers the install once ready, naming the version', () => {
    render(<UpdateBanner update={upd()} />)
    expect(screen.getByText(/0\.17\.17 is ready to install/)).toBeTruthy()
    expect(screen.getByRole('button', { name: /install and restart/i })).toBeTruthy()
  })

  // THE SAFETY GATE, at the UI. Installing restarts the app; a restart mid-QSO loses the
  // contact. The button must refuse AND explain — a disabled control with no reason reads as
  // broken, and the operator would just keep clicking it.
  it('refuses to install while the radio is busy, and says why', () => {
    const install = vi.fn()
    render(
      <UpdateBanner
        update={upd({ install, blockReason: 'In a QSO with 3Y0X — finish or abandon it first' })}
      />,
    )
    const btn = screen.getByRole('button', { name: /in a qso with 3y0x/i })
    expect((btn as HTMLButtonElement).disabled).toBe(true)
    fireEvent.click(btn)
    expect(install).not.toHaveBeenCalled()
  })

  it('installs when nothing blocks', () => {
    const install = vi.fn()
    render(<UpdateBanner update={upd({ install })} />)
    fireEvent.click(screen.getByRole('button', { name: /install and restart/i }))
    expect(install).toHaveBeenCalled()
  })

  // Dismissing is "not now", not "never" — the download is kept, so a later press is instant.
  it('can be dismissed without installing', () => {
    const install = vi.fn()
    const dismiss = vi.fn()
    render(<UpdateBanner update={upd({ install, dismiss })} />)
    fireEvent.click(screen.getByRole('button', { name: /not now/i }))
    expect(dismiss).toHaveBeenCalled()
    expect(install).not.toHaveBeenCalled()
  })

  // Announced politely, unlike Pounce's assertive alert: a new build must never talk over a
  // screen reader mid-QSO.
  it('announces politely, not assertively', () => {
    const { container } = render(<UpdateBanner update={upd()} />)
    const el = container.querySelector('.update-banner')
    expect(el?.getAttribute('role')).toBe('status')
    expect(el?.getAttribute('aria-live')).toBe('polite')
  })

  it('surfaces a failure instead of failing silently', () => {
    render(<UpdateBanner update={upd({ phase: 'error', error: 'signature mismatch' })} />)
    expect(screen.getByText('Update failed')).toBeTruthy()
    expect(screen.getByText(/signature mismatch/)).toBeTruthy()
  })

  // The banner is now the only update prompt where Nexus updates itself (2026-09-29), so a
  // failed install must not strand the operator: the error offers the download page, as the old
  // notice's Download button did.
  it('a failed update offers the download page instead', () => {
    const downloadInstead = vi.fn()
    render(
      <UpdateBanner
        update={upd({ phase: 'error', error: 'Read-only file system (os error 30)', downloadInstead })}
      />,
    )
    const btn = screen.getByRole('button', { name: t('update.download') })
    expect(btn.getAttribute('title')).toBe(t('update.downloadInstead.title'))
    fireEvent.click(btn)
    expect(downloadInstead).toHaveBeenCalledTimes(1)
  })

  it('offers the download page only once installing has failed', () => {
    for (const phase of ['ready', 'installing'] as const) {
      render(<UpdateBanner update={upd({ phase })} />)
      expect(screen.queryByRole('button', { name: t('update.download') }), phase).toBeNull()
      cleanup()
    }
  })
})
