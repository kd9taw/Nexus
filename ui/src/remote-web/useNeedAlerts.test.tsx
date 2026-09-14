// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render } from '@testing-library/react'
import { NEED_ALERT_POLL_MS, newNeeds, useNeedAlerts } from './useNeedAlerts'
import { SessionStatus } from './SessionStatus'
import type { RemoteCollections } from './collections'
import type { NeedAlert } from '../types'

const need = (call: string, priority = 1, band = '20m'): NeedAlert => ({ call, entity: `Entity ${call}`, band, zone: 14,
  tags: ['NewEntity'], priority, headline: 'New DXCC', mode: 'Digital', freqMhz: 14.074 })
class FakeNotification {
  static permission: NotificationPermission = 'default'
  static answer: NotificationPermission = 'granted'
  static requestPermission = vi.fn(async () => { FakeNotification.permission = FakeNotification.answer; return FakeNotification.answer })
  static shown: FakeNotification[] = []
  onclick: (() => void) | null = null
  close = vi.fn()
  constructor(readonly title: string, readonly options: NotificationOptions) { FakeNotification.shown.push(this) }
}
let board: NeedAlert[] = []
const read = vi.fn(async () => ({ rows: structuredClone(board), meta: {} }))
// The alert hook is handed only a reader. There is nothing on it that could tune, work or log.
const source = { read } as unknown as RemoteCollections
function Harness({ ready }: { ready: boolean }) {
  return <SessionStatus stale={false} disconnect={() => {}} alerts={useNeedAlerts(source, ready, null)} />
}
const titles = () => FakeNotification.shown.map(n => n.title)
const flush = () => act(async () => {})
const poll = () => act(async () => { await vi.advanceTimersByTimeAsync(NEED_ALERT_POLL_MS) })
beforeEach(() => {
  localStorage.clear(); board = []; read.mockClear()
  Object.assign(FakeNotification, { permission: 'default', answer: 'granted', shown: [] }); FakeNotification.requestPermission.mockClear()
  vi.stubGlobal('Notification', FakeNotification)
})
afterEach(() => { cleanup(); vi.useRealTimers(); vi.unstubAllGlobals(); vi.restoreAllMocks() })

it('treats the first board as a baseline and each new call, band and mode as news once', () => {
  const first = newNeeds(null, [need('JA1AAA'), need('VK0XYZ')])
  expect(first.fresh).toEqual([])
  const second = newNeeds(first.seen, [need('JA1AAA'), need('VK0XYZ'), need('VK0XYZ', 1, '40m'), need('3Y0ABC', 9)])
  expect(second.fresh.map(a => `${a.call} ${a.band}`)).toEqual(['3Y0ABC 20m', 'VK0XYZ 40m'])
  expect(newNeeds(second.seen, [need('3Y0ABC', 9)]).fresh).toEqual([])
})

it('notifies only after the operator turns alerts on, never on a replayed board, and a click only focuses the tab', async () => {
  vi.useFakeTimers()
  board = [need('JA1AAA')]
  const view = render(<Harness ready />)
  await poll()
  expect(read).not.toHaveBeenCalled()
  expect(FakeNotification.requestPermission).not.toHaveBeenCalled()
  const button = view.container.querySelector<HTMLButtonElement>('.remote-need-alerts button')!
  expect(button.textContent).toBe('Alert me about new needs')
  await act(async () => { fireEvent.click(button) })
  expect(FakeNotification.requestPermission).toHaveBeenCalledOnce()
  await flush()
  expect(read).toHaveBeenCalledOnce()
  expect(titles()).toEqual([])
  expect(button.getAttribute('aria-pressed')).toBe('true')

  board = [need('JA1AAA'), need('VK0XYZ', 9)]
  await poll()
  expect(titles()).toEqual(['New need: Entity VK0XYZ'])
  expect(FakeNotification.shown[0].options.body).toBe('VK0XYZ on 14.074 MHz Digital. New DXCC')
  const focus = vi.spyOn(window, 'focus').mockImplementation(() => {})
  FakeNotification.shown[0].onclick!()
  expect(focus).toHaveBeenCalledOnce()
  expect(FakeNotification.shown[0].close).toHaveBeenCalledOnce()

  await poll()
  expect(titles()).toHaveLength(1)

  // The station link drops and returns with a need that arrived meanwhile: the first read back is
  // a baseline, so the replayed board is silent.
  view.rerender(<Harness ready={false} />)
  board = [...board, need('3Y0ABC', 10)]
  await poll()
  view.rerender(<Harness ready />)
  await flush()
  expect(titles()).toHaveLength(1)
  // Positive control for that silence: a genuinely new need after the baseline still notifies.
  board = [...board, need('FT8WW', 5)]
  await poll()
  expect(titles()).toEqual(['New need: Entity VK0XYZ', 'New need: Entity FT8WW'])
  expect(localStorage.getItem('nexus.remote.needAlerts')).toBe('on')

  await act(async () => { fireEvent.click(button) })
  const reads = read.mock.calls.length
  board = [...board, need('ZL9HR', 7)]
  await poll(); await poll()
  expect(read.mock.calls.length).toBe(reads)
  expect(titles()).toHaveLength(2)
  expect(localStorage.getItem('nexus.remote.needAlerts')).toBe('off')
})

it('shows the three strongest new needs and counts the rest in one summary', async () => {
  vi.useFakeTimers()
  FakeNotification.permission = 'granted'
  localStorage.setItem('nexus.remote.needAlerts', 'on')
  render(<Harness ready />)
  await flush()
  expect(read).toHaveBeenCalledOnce()
  board = [need('A1', 1), need('B2', 5), need('C3', 3), need('D4', 9), need('E5', 2)]
  await poll()
  expect(titles()).toEqual(['New need: Entity D4', 'New need: Entity B2', 'New need: Entity C3', 'More new needs'])
  expect(FakeNotification.shown[3].options.body).toBe("More new needs on the station's Needed board: 2.")
})

it('says so when the browser blocks or cannot show notifications, and reads nothing', async () => {
  FakeNotification.answer = 'denied'
  const view = render(<Harness ready />)
  await act(async () => { fireEvent.click(view.container.querySelector('.remote-need-alerts button')!) })
  expect(view.container.querySelector('.remote-need-alerts')?.textContent).toContain('Notifications are blocked for this site')
  expect(view.container.querySelector('.remote-need-alerts button')).toBeNull()
  cleanup()
  vi.stubGlobal('Notification', undefined)
  const unsupported = render(<Harness ready />)
  expect(unsupported.container.querySelector('.remote-need-alerts')?.textContent).toBe("This browser can't show notifications.")
  await flush()
  expect(read).not.toHaveBeenCalled()
})
