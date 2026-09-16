import { APPLICATION_TIMEOUT_MS } from './application-protocol'
import { QUERY_ERRORS, queryPage, queryRequest } from './application-query-protocol'
import type { QueryArgs, QueryPage } from './application-query-protocol'

type Job = { args: QueryArgs; version: number; resolve: (page: QueryPage) => void; reject: (error: Error) => void }
export class ApplicationQueryClient {
  private queue: Job[] = []
  /** `abandoned` marks a read whose deadline has already failed it for its caller. The slot stays
   * held until its late answer arrives, because the relay is still holding its own slot for the
   * same read and a fresh query arriving on top of that is what it refuses. */
  private current: (Job & { id: string; at: number; abandoned?: boolean }) | null = null
  private deadline: ReturnType<typeof setTimeout> | undefined
  private pacing: ReturnType<typeof setTimeout> | undefined
  constructor(private readonly send: (message: string) => void, private readonly fail: () => void) {}
  read(args: Record<string, unknown>, version = 3): Promise<QueryPage> {
    try { queryRequest({ ...args, type: 'applicationQuery', requestId: crypto.randomUUID() }, version) }
    catch { return Promise.reject(new Error('applicationUnsupported')) }
    if (this.queue.length >= 16) return Promise.reject(new Error('applicationBusy'))
    const result = new Promise<QueryPage>((resolve, reject) => this.queue.push({ args: args as QueryArgs, version, resolve, reject }))
    this.pump()
    return result
  }
  receive(message: Record<string, unknown>): void {
    const pending = this.current
    if (!pending || pending.id !== message.requestId) throw new Error('unexpectedApplicationPage')
    // Its caller was told this read failed when the deadline passed; the answer is merely late.
    // ACK so the relay releases its slot, drop the contents unread, and let the lane carry on.
    if (pending.abandoned) { clearTimeout(this.deadline); this.current = null; this.release(pending.id); return }
    const error = message.type === 'applicationQueryError' && Object.keys(message).length === 3 && QUERY_ERRORS.includes(message.error as never)
    const page = error ? null : queryPage(message, pending.version)
    if (page && (page.collection !== pending.args.collection || page.ageMs + performance.now() - pending.at >= APPLICATION_TIMEOUT_MS ||
      (pending.args.cursor === null ? page.offset !== 0 : `${page.snapshotId}:${page.offset}` !== pending.args.cursor))) throw new Error('invalidApplicationPage')
    clearTimeout(this.deadline); this.current = null
    this.send(JSON.stringify({ type: 'applicationQueryAck', requestId: pending.id }))
    if (page) pending.resolve(page)
    else pending.reject(new Error(String(message.error)))
    this.pacing = setTimeout(() => { this.pacing = undefined; this.pump() }, 125)
  }
  disconnected(): void {
    clearTimeout(this.deadline); clearTimeout(this.pacing); this.pacing = undefined
    this.current?.reject(new Error('applicationUnavailable')); this.current = null
    for (const job of this.queue.splice(0)) job.reject(new Error('applicationUnavailable'))
  }
  /** A read the station did not answer in time fails THAT read, never the session. Collections are
   * background reads - the needs board, the hunter feed, the Pounce alerts, the decode history - and
   * a station whose log is slow to page is still a station the operator can work. What says the
   * station is gone is the INSTRUMENT STREAM, which carries `get_snapshot` and keeps its own
   * deadline; this lane failing the session as well only ever disabled every control and dropped the
   * audio lease for a station that was answering the lane the controls depend on. Deliberately no
   * consecutive-failure threshold: the stream already detects a dead session within the same window,
   * so a second, weaker detector here would add no coverage and one more way to end a working one. */
  private expire(): void {
    const pending = this.current
    if (!pending) return
    pending.abandoned = true
    pending.reject(new Error('applicationUnavailable'))
  }
  /** Release the relay's single-flight slot for a read whose answer is no longer wanted. A send
   * that fails here costs nothing: the relay expires the slot on its own deadline. */
  private release(id: string): void {
    try { this.send(JSON.stringify({ type: 'applicationQueryAck', requestId: id })) } catch { /* socket gone */ }
    this.pacing = setTimeout(() => { this.pacing = undefined; this.pump() }, 125)
  }
  private pump(): void {
    if (this.current || this.pacing) return
    const job = this.queue.shift()
    if (!job) return
    this.current = { ...job, id: crypto.randomUUID(), at: performance.now() }
    this.deadline = setTimeout(() => this.expire(), APPLICATION_TIMEOUT_MS)
    try { this.send(JSON.stringify({ type: 'applicationQuery', requestId: this.current.id, ...job.args })) }
    catch { this.fail() }
  }
}
