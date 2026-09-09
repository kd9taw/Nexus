// A single session serves all existing api.ts consumers. Repeated reads share an
// in-flight request and a short local cache; exact-base deltas reduce wire bytes.
// Traffic is demand-driven. Unmounting a pane stops its reads without another socket.
import type { ApplicationTransport } from '../applicationTransport'
import { APPLICATION_COMMANDS, APPLICATION_ERRORS, APPLICATION_TIMEOUT_MS, applicationCommand,
  applicationReply, applyApplicationReply } from './application-protocol'
import type { ApplicationCommand, ApplicationValue } from './application-protocol'
import { ApplicationStreamClient } from './application-stream-client'
import { STREAM_TOPICS, streamTopic } from './application-stream-protocol'
import type { StreamTopic } from './application-stream-protocol'

export type ApplicationPhase = 'connecting' | 'ready' | 'updateRequired' | 'unavailable'
type Job = { command: ApplicationCommand; resolve: (value: unknown) => void; reject: (reason: Error) => void }
type Current = Job & { requestId: string; at: number; timer: ReturnType<typeof setTimeout> }
const interval: Record<ApplicationCommand, number> = { get_snapshot: 500, get_spectrum_row: 100, get_meters: 200, get_settings: 1000, get_band_plan: 1000 }
export class ApplicationClient implements ApplicationTransport {
  readonly kind = 'remote' as const
  private phase: ApplicationPhase = 'connecting'
  private listeners = new Set<() => void>()
  private values = new Map<ApplicationCommand, ApplicationValue & { at: number }>()
  private jobs = new Map<ApplicationCommand, Promise<unknown>>()
  private queue: Job[] = []
  private current: Current | null = null
  private version = 0
  private stream: ApplicationStreamClient
  constructor(private readonly send: (message: string) => void, private readonly close: () => void, private readonly serviceVersion = 1) {
    this.stream = new ApplicationStreamClient(send, () => { this.disconnected(); close() })
  }
  supports(command: string): boolean { return this.phase === 'ready' && (this.version === 2 ? streamTopic(command) : applicationCommand(command)) }
  getPhase = (): ApplicationPhase => this.phase
  age(command: StreamTopic): number {
    if (this.version === 2) return this.stream.age(command)
    const value = applicationCommand(command) ? this.values.get(command) : null
    return value ? performance.now() - value.at : Infinity
  }
  subscribe = (listener: () => void): (() => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener) } }
  private setPhase(phase: ApplicationPhase): void { this.phase = phase; for (const listener of this.listeners) listener() }
  open(): void {
    this.disconnected()
    this.setPhase('connecting')
    this.send(JSON.stringify(this.serviceVersion === 2 ? { type: 'applicationHello', version: 2 } : { type: 'applicationHello' }))
  }
  disconnected(): void {
    this.stream.disconnected(); this.version = 0
    this.values.clear()
    const pending = this.current
    this.current = null
    if (pending) { clearTimeout(pending.timer); pending.reject(new Error('applicationUnavailable')) }
    for (const job of this.queue.splice(0)) job.reject(new Error('applicationUnavailable'))
    this.jobs.clear()
    this.setPhase('unavailable')
  }
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    if (this.phase === 'ready' && this.version === 2) {
      if (!streamTopic(command) || (args && Object.keys(args).length)) return Promise.reject(new Error('applicationUnsupported'))
      return this.stream.invoke<T>(command)
    }
    if (!applicationCommand(command) || (args && Object.keys(args).length)) return Promise.reject(new Error('applicationUnsupported'))
    if (this.phase !== 'ready') return Promise.reject(new Error(this.phase === 'updateRequired' ? 'stationUpdateRequired' : 'applicationUnavailable'))
    const previous = this.values.get(command)
    if (previous && performance.now() - previous.at < interval[command]) return Promise.resolve(structuredClone(previous.value) as T)
    const waiting = this.jobs.get(command)
    if (waiting) return waiting.then(value => structuredClone(value) as T)
    const promise = new Promise<unknown>((resolve, reject) => { this.queue.push({ command, resolve, reject }) })
    this.jobs.set(command, promise)
    void promise.then(() => { if (this.jobs.get(command) === promise) this.jobs.delete(command) },
      () => { if (this.jobs.get(command) === promise) this.jobs.delete(command) })
    this.pump()
    return promise.then(value => structuredClone(value) as T)
  }
  receive(message: Record<string, unknown>): void {
    if (message.type === 'applicationCapabilities') {
      const commands = message.version === 2 ? STREAM_TOPICS : APPLICATION_COMMANDS
      if (this.phase !== 'connecting' || Object.keys(message).length !== 3 || !(this.serviceVersion === 2 ? [0, 1, 2] : [0, 1]).includes(message.version as number) ||
        !Array.isArray(message.commands) || (message.version === 0 ? message.commands.length !== 0 :
          message.commands.length !== commands.length || !commands.every(command => (message.commands as unknown[]).includes(command)))) {
        throw new Error('invalidApplicationCapabilities')
      }
      this.version = message.version as number
      this.setPhase(this.version > 0 ? 'ready' : 'updateRequired')
      return
    }
    if (this.version === 2) { this.stream.receive(message); return }
    const pending = this.current
    if (!pending || pending.requestId !== message.requestId) throw new Error('unexpectedApplicationResult')
    if (message.type === 'applicationError' && Object.keys(message).length === 3 && APPLICATION_ERRORS.includes(message.error as never)) {
      clearTimeout(pending.timer); this.current = null
      this.send(JSON.stringify({ type: 'applicationAck', requestId: pending.requestId }))
      pending.reject(new Error(String(message.error))); this.pump(); return
    }
    const reply = applicationReply(message)
    const now = performance.now(), age = now - pending.at + reply.ageMs
    if (reply.command !== pending.command || age < 0 || age >= APPLICATION_TIMEOUT_MS) throw new Error('expiredApplicationResult')
    const current = applyApplicationReply(this.values.get(pending.command) ?? null, reply)
    this.values.set(pending.command, { ...current, at: now - age })
    clearTimeout(pending.timer); this.current = null
    this.send(JSON.stringify({ type: 'applicationAck', requestId: pending.requestId }))
    pending.resolve(current.value); this.pump()
  }
  private pump(): void {
    if (this.current || this.phase !== 'ready') return
    const job = this.queue.shift()
    if (!job) return
    const requestId = crypto.randomUUID()
    const timer = setTimeout(() => { this.disconnected(); this.close() }, APPLICATION_TIMEOUT_MS)
    this.current = { ...job, requestId, timer, at: performance.now() }
    try {
      this.send(JSON.stringify({ type: 'applicationRead', requestId, command: job.command,
        revision: this.values.get(job.command)?.revision ?? null }))
    } catch { this.disconnected(); this.close() }
  }
}
