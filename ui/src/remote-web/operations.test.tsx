// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { LoggingAuthority, RemoteLogEntry } from './operations'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import type { AppSnapshot } from '../types'
vi.mock('../api', () =>
  Object.fromEntries(
    [
      'fdLogManual',
      'logQso',
      'getLog',
      'lookupPark',
      'lookupParkLive',
      'qrzLookup',
      'resolveEntity',
      'searchParks',
      'setCwPeerInfo'
    ].map((n) => [n, vi.fn(async () => null)])
  )
)
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})
const snap = {
  radio: { band: '20m', dialMhz: 14.25 },
  hunt: null,
  mygrid: 'FN31',
  stations: []
} as unknown as AppSnapshot
it.each(['receipt', 'checked log'])(
  'resolving by %s in another tab clears only the submitted draft',
  async (how) => {
    // This fixture replies to explicit actions only. Keep automatic heartbeat
    // intervals deterministic so a slow render cannot receive the QSO outcome
    // as the reply to an unrelated state poll. Client and browser tests exercise
    // the real heartbeat separately; waitFor keeps its real timeout here.
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
    const sent: Record<string, any>[] = [],
      client = new OperationClient(
        (s) => sent.push(JSON.parse(s)),
        true,
        () => 1000
      )
    const reply = (value: unknown) =>
      client.receive({
        type: 'operationResponse',
        requestId: sent[sent.length - 1].request.requestId,
        value
      })
    client.open()
    reply({
      stationBootId: crypto.randomUUID(),
      allowed: true,
      phase: 'controlling',
      leaseId: crypto.randomUUID(),
      revision: 1,
      commandWindowId: crypto.randomUUID(),
      nextSequence: 1,
      leaseRemainingMs: 5000,
      actions: ['log.manual'],
      txArmed: false
    })
    try {
      render(
        <>
          <div data-testid="cw">
            <RemoteLogEntry client={client} snap={snap} mode="CW" />
          </div>
          <div data-testid="phone">
            <RemoteLogEntry client={client} snap={snap} mode="SSB" />
          </div>
        </>
      )
      const cw = within(screen.getByTestId('cw')),
        phone = within(screen.getByTestId('phone')),
        own = cw.getByPlaceholderText('Call') as HTMLInputElement,
        other = phone.getByPlaceholderText('Call') as HTMLInputElement
      fireEvent.change(own, { target: { value: 'W1AW' } })
      fireEvent.change(other, { target: { value: 'K2ABC' } })
      fireEvent.click(cw.getByRole('button', { name: 'Log' }))
      expect(cw.getByText('Saving QSO at the station…')).toBeTruthy()
      expect(cw.queryByRole('button', { name: 'Check submitted QSO result' })).toBeNull()
      const operationId = sent[sent.length - 1].request.requestId
      await act(async () =>
        reply({ outcome: 'unknown', reason: 'persistenceUnconfirmed', operationId })
      )
      expect(own.value).toBe('W1AW')
      expect(other.value).toBe('K2ABC')
      if (how === 'receipt') {
        fireEvent.click(phone.getByRole('button', { name: 'Check submitted QSO result' }))
        await act(async () =>
          reply({
            outcome: 'applied',
            evidence: 'fileSynced',
            uploads: 'stationPipeline',
            operationId
          })
        )
      } else
        fireEvent.click(
          phone.getByRole('button', { name: 'I checked the station log — finish this check' })
        )
      await waitFor(() => expect(own.value).toBe(''))
      expect(other.value).toBe('K2ABC')
      expect(sent.filter((v) => v.request.type === 'logManual')).toHaveLength(1)
    } finally {
      client.disconnected()
    }
  }
)

it('shows every retained submitted field after reload without making a new entry', () => {
  const record = {
    call: 'W1AW',
    grid: 'FN31',
    country: 'United States',
    state: 'CT',
    band: '20m',
    freqMhz: 14.25,
    mode: 'SSB',
    rstSent: '59',
    rstRcvd: '57',
    name: 'Joe',
    qth: 'Newington',
    comment: 'Shared comment',
    notes: 'Unconfirmed contact notes',
    whenUnix: null,
    confirmed: false as const,
    awardConfirmed: false as const,
    ota: { theirProgram: 'POTA' as const, theirRef: 'US-1234' }
  }
  const send = vi.fn(),
    client = new OperationClient(send, true, () => 1000, {
      read: () => crypto.randomUUID(),
      readDraft: () => record,
      write: () => {}
    })
  render(<RemoteLogEntry client={client} snap={snap} mode="CW" />)
  for (const text of [
    'W1AW',
    'FN31',
    'United States',
    'CT',
    'Joe',
    'Newington',
    'Shared comment',
    'Unconfirmed contact notes',
    'US-1234'
  ])
    expect(screen.getByText(text)).toBeTruthy()
  expect((screen.getByPlaceholderText('Call') as HTMLInputElement).value).toBe('')
  expect((screen.getByRole('button', { name: 'Log' }) as HTMLButtonElement).disabled).toBe(true)
  expect(send).not.toHaveBeenCalled()
  client.disconnected()
})

it.each(['applied', 'rejected', 'unknown'] as const)(
  'shows a recovered %s receipt after reload and retains an unconfirmed submitted record',
  async (outcome) => {
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
    const operationId = crypto.randomUUID()
    const record = {
      call: 'W1AW',
      grid: 'FN31',
      country: null,
      state: null,
      band: '20m',
      freqMhz: 14.25,
      mode: 'CW',
      rstSent: '599',
      rstRcvd: '579',
      name: null,
      qth: null,
      comment: null,
      notes: 'Retain this unconfirmed note',
      whenUnix: null,
      confirmed: false as const,
      awardConfirmed: false as const
    }
    let saved: string | null = operationId
    const sent: Record<string, any>[] = []
    const client = new OperationClient(
      (raw) => sent.push(JSON.parse(raw)),
      true,
      () => 1000,
      {
        read: () => saved,
        readDraft: () => record,
        write: (next) => {
          saved = next
        }
      }
    )
    const reply = (value: unknown) =>
      client.receive({
        type: 'operationResponse',
        requestId: sent[sent.length - 1].request.requestId,
        value
      })
    client.open()
    reply({
      stationBootId: crypto.randomUUID(),
      allowed: true,
      phase: 'available',
      leaseId: null,
      revision: 1,
      commandWindowId: null,
      nextSequence: null,
      leaseRemainingMs: null,
      actions: ['log.manual'],
      txArmed: false
    })
    try {
      render(<RemoteLogEntry client={client} snap={snap} mode="CW" />)
      expect(screen.getByText(record.notes)).toBeTruthy()
      fireEvent.click(screen.getByRole('button', { name: 'Check submitted QSO result' }))
      expect(sent[sent.length - 1].request.type).toBe('result')
      await act(async () =>
        reply(
          outcome === 'applied'
            ? { outcome, operationId, evidence: 'fileSynced', uploads: 'stationPipeline' }
            : {
                outcome,
                operationId,
                reason: outcome === 'rejected' ? 'alreadyPresent' : 'persistenceUnconfirmed'
              }
        )
      )
      if (outcome === 'applied') {
        expect(screen.getByText(/QSO saved to the station log file/)).toBeTruthy()
        expect(saved).toBeNull()
      } else {
        expect(screen.getByText(record.notes)).toBeTruthy()
        expect(saved).toBe(operationId)
        expect(client.getSnapshot().unresolved).toBe(operationId)
      }
      expect(sent.filter((value) => value.request.type === 'logManual')).toHaveLength(0)
    } finally {
      client.disconnected()
    }
  }
)

const NOT_SENT = 'Not sent. Nothing reached the station, so nothing changed there. Try again.'
const NOT_CONFIRMED = 'The command was not confirmed. Check station control permission and the current readings.'
const LOG_NOT_SENT = 'Not sent. Nothing reached the station, so this QSO was not logged. Try again.'
const BUSY = 'The station was busy and nothing changed. Try again.'
const LOG_BUSY = 'The station was busy, so this QSO was not logged. Try again.'
const LOG_REFUSED = 'The station did not confirm this entry. Check logging control and the station’s current mode, then try again.'
const memory = () => {
  const values = new Map<string, string>()
  return { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
}
const controlling = (extra: Record<string, unknown>) => ({
  stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
  commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false, ...extra
})

it.each([true, false])('says whether a failed station control reached the station (request sent: %s)', async (sendsRequest) => {
  vi.useFakeTimers()
  let now = 1000
  const sent: Record<string, any>[] = [], data = memory()
  const client = new OperationClient((s) => sent.push(JSON.parse(s)), true, () => now, undefined, 2,
    pendingControlStorage(() => data, 'station', async (_key, run) => run()))
  client.open()
  client.receive({ type: 'operationResponse', requestId: sent[0].request.requestId,
    value: controlling({ controls: { context: { radioId: 1, radioConnection: 1, ampConnection: 1, ampReadSequence: 1 }, capabilities: ['amplifier'] } }) })
  const amp = { action: 'amplifier.operate', expectedOperate: false, operate: true } as const
  try {
    if (sendsRequest) {
      const attempt = client.control(amp).catch(() => {})
      await act(async () => { await Promise.resolve(); await Promise.resolve() })
      const request = sent.filter((w) => w.request.type === 'stationControl')
      expect(request).toHaveLength(1)
      client.receive({ type: 'operationResponse', requestId: request[0].request.requestId, error: 'staleContext' })
      await attempt
    } else {
      now += 1000
      await vi.advanceTimersByTimeAsync(250)
      expect(sent[sent.length - 1].request.type).toBe('heartbeat')
      const attempt = client.control(amp).catch(() => {})
      now += 250
      await vi.advanceTimersByTimeAsync(250)
      await attempt
      expect(sent.filter((w) => w.request.type === 'stationControl')).toHaveLength(0)
    }
    render(<LoggingAuthority client={client} />)
    expect(screen.getByRole('alert').textContent).toBe(sendsRequest ? NOT_CONFIRMED : NOT_SENT)
  } finally {
    client.disconnected()
  }
})

it.each([true, false])('says whether a failed manual QSO reached the station (request sent: %s)', async (sendsRequest) => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  let now = 1000
  const sent: Record<string, any>[] = []
  let openLock = () => {}
  const lock = new Promise<void>((resolve) => { openLock = resolve })
  // The click is admitted while fresh, then waits for the browser receipt lock (another tab).
  const client = new OperationClient((s) => sent.push(JSON.parse(s)), true, () => now, {
    read: () => null,
    write: () => {},
    exclusive: async (run) => { await lock; return run() }
  })
  client.open()
  client.receive({ type: 'operationResponse', requestId: sent[0].request.requestId, value: controlling({ actions: ['log.manual'] }) })
  try {
    render(<RemoteLogEntry client={client} snap={snap} mode="CW" />)
    fireEvent.change(screen.getByPlaceholderText('Call'), { target: { value: 'W1AW' } })
    fireEvent.click(screen.getByRole('button', { name: 'Log' }))
    // Without a request: the click's command window closes before the lock is released.
    if (!sendsRequest) now = 2300
    await act(async () => { openLock(); await Promise.resolve(); await Promise.resolve() })
    if (sendsRequest) {
      const request = sent.filter((w) => w.request.type === 'logManual')
      expect(request).toHaveLength(1)
      await act(async () => client.receive({ type: 'operationResponse', requestId: request[0].request.requestId, error: 'staleContext' }))
    }
    await waitFor(() => expect(screen.getByRole('alert').textContent).toBe(sendsRequest ? LOG_REFUSED : LOG_NOT_SENT))
    expect(sent.filter((w) => w.request.type === 'logManual')).toHaveLength(sendsRequest ? 1 : 0)
  } finally {
    client.disconnected()
  }
})

it.each(['error reply', 'rejected outcome'] as const)('says the station was busy for a stationBusy %s, not "not confirmed"', async (form) => {
  vi.useFakeTimers()
  const sent: Record<string, any>[] = [], data = memory()
  const client = new OperationClient((s) => sent.push(JSON.parse(s)), true, () => 1000, undefined, 2,
    pendingControlStorage(() => data, 'station', async (_key, run) => run()))
  client.open()
  client.receive({ type: 'operationResponse', requestId: sent[0].request.requestId,
    value: controlling({ controls: { context: { radioId: 1, radioConnection: 1, ampConnection: 1, ampReadSequence: 1 }, capabilities: ['amplifier'] } }) })
  const amp = { action: 'amplifier.operate', expectedOperate: false, operate: true } as const
  try {
    const attempt = client.control(amp).catch(() => {})
    await act(async () => { await Promise.resolve(); await Promise.resolve() })
    const request = sent.filter((w) => w.request.type === 'stationControl')
    expect(request).toHaveLength(1)
    client.receive(form === 'error reply'
      ? { type: 'operationResponse', requestId: request[0].request.requestId, error: 'stationBusy' }
      : { type: 'operationResponse', requestId: request[0].request.requestId, value: { operation: 'stationControl', operationId: request[0].request.requestId, outcome: 'rejected', reason: 'stationBusy' } })
    await attempt
    render(<LoggingAuthority client={client} />)
    // The banner keeps its own logging status beside the command result; read the result itself.
    const result = document.querySelector<HTMLElement>('.remote-control-result')!
    const shown = within(result).getByRole(form === 'error reply' ? 'alert' : 'status')
    expect(shown.textContent).toBe(BUSY)
    if (form === 'error reply') expect(shown.getAttribute('data-control-failure')).toBe('busy')
  } finally {
    client.disconnected()
  }
})

it('says the station was busy when it refused a manual QSO with stationBusy', async () => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  const sent: Record<string, any>[] = []
  const client = new OperationClient((s) => sent.push(JSON.parse(s)), true, () => 1000, { read: () => null, write: () => {}, exclusive: async (run) => run() })
  client.open()
  client.receive({ type: 'operationResponse', requestId: sent[0].request.requestId, value: controlling({ actions: ['log.manual'] }) })
  try {
    render(<RemoteLogEntry client={client} snap={snap} mode="CW" />)
    fireEvent.change(screen.getByPlaceholderText('Call'), { target: { value: 'W1AW' } })
    fireEvent.click(screen.getByRole('button', { name: 'Log' }))
    await act(async () => { await Promise.resolve(); await Promise.resolve() })
    const request = sent.filter((w) => w.request.type === 'logManual')
    expect(request).toHaveLength(1)
    await act(async () => client.receive({ type: 'operationResponse', requestId: request[0].request.requestId, error: 'stationBusy' }))
    await waitFor(() => expect(screen.getByRole('alert').textContent).toBe(LOG_BUSY))
  } finally {
    client.disconnected()
  }
})
