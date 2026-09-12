import { APPLICATION_TIMEOUT_MS, applyApplicationReply } from './application-protocol'
import type { ApplicationValue } from './application-protocol'
import { STREAM_INTEREST_MS, STREAM_INTERVAL, streamExact, streamUpdates } from './application-stream-protocol'
import type { StreamTopic, StreamVersion } from './application-stream-protocol'

type Waiting = { promise: Promise<unknown>; resolve: (value: unknown) => void; reject: (error: Error) => void }
// api.ts polling expresses local interest only. One socket subscription serves
// every mounted consumer; expired interest removes work from the station union.
export class ApplicationStreamClient {
  private version: StreamVersion = 2
  private values = new Map<StreamTopic, ApplicationValue & { at: number }>()
  private interests = new Map<StreamTopic, number>()
  private waiting = new Map<StreamTopic, Waiting>()
  private topics: StreamTopic[] = []
  private credit: { id: string; at: number } | null = null
  private cancelled: string[] = []
  private sweep: ReturnType<typeof setInterval> | undefined
  private flushTimer: ReturnType<typeof setTimeout> | undefined
  private deadline: ReturnType<typeof setTimeout> | undefined
  constructor(private readonly send: (message: string) => void, private readonly fail: () => void) {}
  negotiate(version: StreamVersion): void { this.version = version }
  age(command: StreamTopic): number { const value = this.values.get(command); return value ? performance.now() - value.at : Infinity }
  invoke<T>(command: StreamTopic): Promise<T> {
    this.interests.set(command, performance.now())
    if (!this.sweep) this.sweep = setInterval(() => this.flush(), 500)
    if (!this.flushTimer) this.flushTimer = setTimeout(() => { this.flushTimer = undefined; this.flush() }, 0)
    const value = this.values.get(command)
    if (value && this.age(command) < STREAM_INTERVAL[command]) return Promise.resolve(structuredClone(value.value) as T)
    let waiting = this.waiting.get(command)
    if (!waiting) {
      let resolve!: Waiting['resolve'], reject!: Waiting['reject']
      const promise = new Promise<unknown>((yes, no) => { resolve = yes; reject = no })
      waiting = { promise, resolve, reject }; this.waiting.set(command, waiting)
    }
    return waiting.promise.then(v => structuredClone(v) as T)
  }
  receive(message: Record<string, unknown>): void {
    streamExact(message, ['type', 'requestId', 'updates'])
    if (message.type !== 'applicationFrame') throw new Error('invalidApplicationFrame')
    if (this.cancelled.includes(String(message.requestId))) return
    if (!this.credit || message.requestId !== this.credit.id) throw new Error('unexpectedApplicationFrame')
    const now = performance.now(), elapsed = now - this.credit.at
    const updates = streamUpdates(message.updates, this.credit.id, this.version)
    // Validate the complete frame before any consumer can see a partial commit.
    const next = updates.map(update => {
      if (update.type === 'applicationError') return { update, value: null }
      const age = elapsed + update.ageMs
      if (age < 0 || age >= APPLICATION_TIMEOUT_MS) throw new Error('expiredApplicationResult')
      return { update, value: { ...applyApplicationReply(this.values.get(update.command) ?? null, update), at: now - age } }
    })
    for (const { update, value } of next) {
      const waiting = this.waiting.get(update.command)
      this.waiting.delete(update.command)
      if (value) { this.values.set(update.command, value); waiting?.resolve(value.value) }
      else { this.values.delete(update.command); waiting?.reject(new Error(update.type === 'applicationError' ? update.error : 'applicationUnavailable')) }
    }
    const previous = this.credit.id
    this.newCredit()
    this.send(JSON.stringify({ type: 'applicationFrameAck', requestId: previous, nextRequestId: this.credit!.id }))
  }
  disconnected(): void {
    clearInterval(this.sweep); clearTimeout(this.flushTimer); clearTimeout(this.deadline)
    this.sweep = undefined; this.flushTimer = undefined; this.deadline = undefined
    this.credit = null; this.topics = []; this.cancelled = []
    this.values.clear(); this.interests.clear()
    for (const waiting of this.waiting.values()) waiting.reject(new Error('applicationUnavailable'))
    this.waiting.clear()
  }
  private newCredit(): void {
    clearTimeout(this.deadline)
    this.credit = { id: crypto.randomUUID(), at: performance.now() }
    this.deadline = setTimeout(this.fail, APPLICATION_TIMEOUT_MS)
  }
  private flush(): void {
    const now = performance.now()
    for (const [topic, at] of this.interests) if (now - at >= STREAM_INTEREST_MS && !this.waiting.has(topic)) this.interests.delete(topic)
    const topics = [...this.interests.keys()].sort()
    if (JSON.stringify(topics) === JSON.stringify(this.topics)) return
    this.topics = topics
    if (topics.length && !this.credit) this.newCredit()
    if (!topics.length) {
      if (this.credit) this.cancelled = [...this.cancelled.slice(-7), this.credit.id]
      this.credit = null; clearTimeout(this.deadline); clearInterval(this.sweep); this.sweep = undefined
    }
    try { this.send(JSON.stringify({ type: 'applicationSubscribe', topics, requestId: this.credit?.id ?? null })) }
    catch { this.fail() }
  }
}
