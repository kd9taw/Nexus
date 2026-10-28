// @vitest-environment jsdom
//
// CONTACTS WAITING FOR LOG QSO ARE COUNTED BESIDE THE LOG BUTTON.
//
// With Auto-log and the log popup both off, a finished contact that a click on another station,
// Call CQ or a mode change replaced waits for Log QSO, one press each. Nothing showed that it was
// there, and after Call CQ the Log button was disabled (no station on screen), so the press that
// takes it could not be made. The strip now shows how many wait, on the Log button's corner, out of
// the layout, and Log stays live while any wait.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, cleanup, fireEvent } from '@testing-library/react'
import { OperateQsoStrip } from './OperateQsoStrip'
import { t } from '../i18n'
import type { QsoStatus } from '../types'

afterEach(cleanup)

const calling = {
  state: 'CallingCq', dxcall: null, rxReport: null, running: true, cqRunning: true,
} as unknown as QsoStatus

function strip(logWaiting: number | undefined) {
  const onLog = vi.fn()
  const noop = vi.fn()
  const { container } = render(
    <OperateQsoStrip
      qso={calling}
      onSetMode={noop}
      onResend={noop}
      onFreetext={noop}
      onLog={onLog}
      logWaiting={logWaiting}
    />,
  )
  return { container, onLog, log: container.querySelector<HTMLButtonElement>('.cq-log')! }
}

describe('contacts waiting for Log QSO', () => {
  it('none waiting: no count, and Log is off with no station on screen', () => {
    for (const n of [undefined, 0]) {
      const { container, log } = strip(n)
      expect(container.querySelector('.cq-log-waiting')).toBeNull()
      expect(log.disabled).toBe(true)
      // The status is there before any contact waits, empty, so the first count is announced.
      const status = container.querySelector('.cq-free [role="status"]')
      expect(status, 'the polite status is mounted').not.toBeNull()
      expect(status!.textContent).toBe('')
      expect(log.getAttribute('aria-label')).toBeNull()
      expect(log.getAttribute('title')).toBe(t('operate.strip.log.title'))
      cleanup()
    }
  })

  it('two waiting: the count says so beside Log, and Log takes one', () => {
    const { container, log, onLog } = strip(2)
    const count = container.querySelector('.cq-log-waiting')
    expect(count, 'no count').not.toBeNull()
    expect(count!.textContent).toBe('2')
    // The badge takes no pointer, so it carries no tooltip and is hidden from a screen reader:
    // Log's own tooltip and spoken name say the count, and a polite status announces it.
    expect(count!.getAttribute('aria-hidden')).toBe('true')
    expect(count!.getAttribute('title')).toBeNull()
    expect(log.getAttribute('title')).toBe(t('operate.strip.log.titleWaiting', { count: 2 }))
    expect(log.getAttribute('aria-label')).toBe(t('operate.strip.log.ariaWaiting', { count: 2 }))
    expect(t('operate.strip.log.ariaWaiting', { count: 2 })).toBe('Log, 2 waiting')
    expect(t('operate.strip.log.waiting', { count: 2 })).toBe('2 waiting for Log')
    const status = container.querySelector('.cq-free [role="status"]')
    expect(status?.textContent).toBe(t('operate.strip.log.waiting', { count: 2 }))
    expect(status?.getAttribute('aria-live')).toBe('polite')
    // Out of the button: its label, and so its name and width, are what they were.
    expect(log.textContent).toBe(t('operate.strip.log.label'))
    expect(log.disabled).toBe(false)
    fireEvent.click(log)
    expect(onLog).toHaveBeenCalledTimes(1)
  })
})
