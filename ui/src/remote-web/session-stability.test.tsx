// @vitest-environment jsdom
// The session banner sits above every cockpit, so any element it adds or removes moves every
// control below it. Operators saw that as flicker: a command result cleared the station state in
// the same update that showed the outcome, Release and the pending buttons unmounted until the next
// heartbeat, and the freshness gap between heartbeats flipped the authority label on and off.
// jsdom never lays out, so the detector counts what changes layout here: element insertions and
// removals and `hidden` toggles inside the banner. Text and `disabled` changes are allowed.
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { SessionStatus } from './SessionStatus'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import type { OperationState } from './operation-protocol'

const clients: OperationClient[] = []
afterEach(() => { cleanup(); clients.splice(0).forEach(client => client.disconnected()); vi.useRealTimers() })

function fixture() {
  vi.useFakeTimers()
  let now = 1000
  const sent: { request: { type: string; requestId: string; leaseId?: string } }[] = []
  const values = new Map<string, string>()
  const storage = { getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value) }, removeItem: (key: string) => { values.delete(key) } }
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => now, undefined, 3,
    pendingControlStorage(() => storage, 'stability-test', async (_key, run) => run()))
  clients.push(client)
  const state: OperationState = {
    stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(),
    revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 1, ampConnection: 1, ampReadSequence: 1 }, capabilities: ['amplifier'] }
  }
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1]!.request.requestId, value })
  client.open(); reply(state)
  const view = (stale = false) => <SessionStatus client={client} stale={stale} disconnect={() => {}} />
  const ui = render(view())
  const advance = async (ms: number) => { now += ms; await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }
  return { ...ui, view, client, state, sent, reply, advance }
}

/** Records every change inside the banner that can move what is below it. */
function layoutDetector(root: Element) {
  const changes: string[] = []
  const describe = (node: Node) => node instanceof Element ? `${node.tagName.toLowerCase()}.${node.className || ''}[${node.textContent}]` : ''
  const record = (records: MutationRecord[]) => {
    for (const r of records) {
      if (r.type === 'attributes') changes.push(`hidden ${describe(r.target)}`)
      for (const node of r.addedNodes) if (node instanceof Element) changes.push(`added ${describe(node)}`)
      for (const node of r.removedNodes) if (node instanceof Element) changes.push(`removed ${describe(node)}`)
    }
  }
  const observer = new MutationObserver(record)
  observer.observe(root, { childList: true, subtree: true, attributes: true, attributeFilter: ['hidden'] })
  // takeRecords() drains synchronously, so an assertion never races the observer's microtask.
  return { flush: () => { record(observer.takeRecords()); return changes }, stop: () => observer.disconnect() }
}
const banner = () => document.querySelector('.remote-application-status')!
const release = () => screen.getByRole('button', { name: 'Release station control' }) as HTMLButtonElement

it('keeps the banner still and its controls mounted across a command result and the station re-read', async () => {
  const h = fixture()
  const releaseButton = release()
  const detector = layoutDetector(banner())
  let pending!: Promise<unknown>
  await act(async () => {
    pending = h.client.control({ action: 'amplifier.operate', expectedOperate: false, operate: true })
    void pending.catch(() => {})
    await Promise.resolve()
  })
  const command = h.sent[h.sent.length - 1]!.request
  expect(command.type).toBe('stationControl')
  await act(async () => {
    h.reply({ operation: 'stationControl', operationId: command.requestId, outcome: 'applied', evidence: 'stationState' })
    await pending
  })
  // The gap: the command spent the state's window and the re-read has not answered yet. The state
  // itself is held (operator ruling 2026-09-16: a control stays lit while it confirms), so nothing
  // in the banner is disabled; the client's own refusal keeps a second command off the spent window.
  expect(h.client.getSnapshot()).toMatchObject({ fresh: false, controlRefreshing: true, state: { phase: 'controlling' } })
  expect(screen.getByText('The station confirmed the command.')).toBeTruthy()
  expect(screen.getByText('Updating station controls…')).toBeTruthy()
  expect(release()).toBe(releaseButton)
  expect(release().disabled).toBe(false)
  await h.advance(250)
  const reread = h.sent[h.sent.length - 1]!.request
  expect(reread.type).toBe('heartbeat')
  act(() => h.reply({ ...h.state, revision: 2, nextSequence: 2 }))
  expect(release()).toBe(releaseButton)
  expect(release().disabled).toBe(false)
  expect(detector.flush()).toEqual([])
  // Positive control: a real loss of authority does remove Release, and the detector sees it.
  act(() => h.client.disconnected())
  expect(screen.queryByRole('button', { name: 'Release station control' })).toBeNull()
  expect(detector.flush().some(change => change.startsWith('removed button'))).toBe(true)
  detector.stop()
})

it('keeps the authority label steady through the gap between heartbeats while the lease is held', async () => {
  const h = fixture()
  const detector = layoutDetector(banner())
  // Withhold the heartbeat reply: the 1.2 s command window lapses, the 5 s lease does not.
  await h.advance(1500)
  expect(h.sent[h.sent.length - 1]!.request.type).toBe('heartbeat')
  expect(h.client.getSnapshot().fresh).toBe(false)
  expect(screen.getByText('Station control active')).toBeTruthy()
  expect(screen.queryByText('Logging control status unavailable')).toBeNull()
  act(() => h.reply({ ...h.state }))
  expect(h.client.getSnapshot().fresh).toBe(true)
  expect(detector.flush()).toEqual([])
  // Positive control on the label: once the lease the station reported has run out with no reply,
  // the banner stops claiming control.
  await h.advance(1000)
  await h.advance(5000)
  expect(h.client.getSnapshot().fresh).toBe(false)
  expect(screen.getByText('Logging control status unavailable')).toBeTruthy()
  detector.stop()
})

it('shows station data loss without adding or removing anything in the banner', () => {
  const h = fixture()
  const detector = layoutDetector(banner())
  h.rerender(h.view(true))
  expect(screen.getByRole('alert').textContent).toContain('Station data unavailable')
  // The authority is still said beside it: a disconnect always coincides with stale data.
  expect(screen.getByText('Station control active')).toBeTruthy()
  h.rerender(h.view(false))
  expect(screen.getByRole('alert').textContent).toBe('')
  expect(detector.flush()).toEqual([])
  detector.stop()
})

// The state is held through the re-read gap (operator ruling 2026-09-16: a control stays lit while
// it confirms), so the banner's Release is live there and its click really leaves — it is bound to
// the lease, not to the command window the command just spent. A gesture still cannot capture that
// spent window; control.test.ts proves no command leaves on it.
it('keeps the banner live in the re-read gap: Release sends, a gesture cannot capture the spent window', async () => {
  const h = fixture()
  let pending!: Promise<unknown>
  await act(async () => {
    pending = h.client.control({ action: 'amplifier.operate', expectedOperate: false, operate: true })
    void pending.catch(() => {})
    await Promise.resolve()
  })
  const command = h.sent[h.sent.length - 1]!.request
  await act(async () => {
    h.reply({ operation: 'stationControl', operationId: command.requestId, outcome: 'applied', evidence: 'stationState' })
    await pending
  })
  expect(h.client.getSnapshot()).toMatchObject({ fresh: false, state: { phase: 'controlling' } })
  expect(() => h.client.prepareControl()).toThrow('notController')
  const before = h.sent.length
  expect(release().disabled).toBe(false)
  fireEvent.click(release())
  expect(h.sent).toHaveLength(before + 1)
  expect(h.sent[h.sent.length - 1]!.request.type).toBe('release')
})

// THE RELEASE BUTTON FLICKERED — it greyed on every heartbeat (~16 toggles in 10 s), because `busy`
// is true for ANY request in flight. The gate was not decoration: `release()` clears the lease
// locally before sending, and `request()` refuses while another request is in flight (the refusal
// is swallowed), so a Release clicked mid-heartbeat used to send NOTHING while the heartbeat's reply
// re-armed the lease — the operator believed they had released the station and still held it. So
// the fix is not "stop greying": Release waits out an automatic READ, then sends once.
it('keeps Release enabled while an automatic heartbeat is in flight', async () => {
  const h = fixture()
  await h.advance(1500)
  expect(h.sent[h.sent.length - 1]!.request.type).toBe('heartbeat')
  expect(h.client.getSnapshot().busy, 'scene: the read really is in flight').toBe(true)
  expect(release().disabled, 'a routine heartbeat must not grey Release').toBe(false)
})

it('a Release clicked during a heartbeat is sent once, after the read settles, and control does not come back', async () => {
  const h = fixture()
  await h.advance(1500)
  const hb = h.sent[h.sent.length - 1]!.request
  expect(hb.type, 'scene: an automatic read is in flight, unanswered').toBe('heartbeat')
  const before = h.sent.length
  // Twice: a double-click must still send exactly once.
  await act(async () => { fireEvent.click(release()); fireEvent.click(release()); await Promise.resolve() })
  expect(h.sent.length, 'nothing may leave while the read is in flight').toBe(before)
  // The heartbeat replies with the station still controlling — the reply that used to re-arm the lease.
  await act(async () => { h.client.receive({ type: 'operationResponse', requestId: hb.requestId, value: { ...h.state } }) })
  await h.advance(0)
  const releases = h.sent.slice(before).filter(x => x.request.type === 'release')
  expect(releases, 'exactly one release, sent after the read settled').toHaveLength(1)
  expect(releases[0]!.request.leaseId, 'with the lease the station holds').toBe(h.state.leaseId)
  act(() => h.reply({ ...h.state, phase: 'available', leaseId: null, commandWindowId: null, nextSequence: null, leaseRemainingMs: null }))
  await h.advance(0)
  expect(screen.getByRole('button', { name: /take station control/i }), 'control is released and stays released').toBeTruthy()
})

it('Release stays disabled while a station command is in flight — only an automatic READ is waited out', async () => {
  const h = fixture()
  await act(async () => {
    void h.client.control({ action: 'amplifier.operate', expectedOperate: false, operate: true }).catch(() => {})
    await Promise.resolve()
  })
  expect(h.sent[h.sent.length - 1]!.request.type).toBe('stationControl')
  expect(release().disabled, 'a command in flight still holds Release back').toBe(true)
})
