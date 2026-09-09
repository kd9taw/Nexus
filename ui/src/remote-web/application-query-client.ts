import { APPLICATION_TIMEOUT_MS } from './application-protocol'
import { QUERY_ERRORS, queryPage, queryRequest } from './application-query-protocol'
import type { QueryArgs, QueryPage } from './application-query-protocol'

type Job = { args: QueryArgs; resolve: (page: QueryPage) => void; reject: (error: Error) => void }
export class ApplicationQueryClient {
  private queue: Job[] = []
  private current: (Job & { id: string; at: number }) | null = null
  private deadline: ReturnType<typeof setTimeout> | undefined
  private pacing: ReturnType<typeof setTimeout> | undefined
  constructor(private readonly send: (message: string) => void, private readonly fail: () => void) {}
  read(args: Record<string, unknown>): Promise<QueryPage> {
    try { queryRequest({ ...args, type: 'applicationQuery', requestId: crypto.randomUUID() }) }
    catch { return Promise.reject(new Error('applicationUnsupported')) }
    if (this.queue.length >= 16) return Promise.reject(new Error('applicationBusy'))
    const result = new Promise<QueryPage>((resolve, reject) => this.queue.push({ args: args as QueryArgs, resolve, reject }))
    this.pump()
    return result
  }
  receive(message: Record<string, unknown>): void {
    const pending = this.current
    if (!pending || pending.id !== message.requestId) throw new Error('unexpectedApplicationPage')
    const error = message.type === 'applicationQueryError' && Object.keys(message).length === 3 && QUERY_ERRORS.includes(message.error as never)
    const page = error ? null : queryPage(message)
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
  private pump(): void {
    if (this.current || this.pacing) return
    const job = this.queue.shift()
    if (!job) return
    this.current = { ...job, id: crypto.randomUUID(), at: performance.now() }
    this.deadline = setTimeout(this.fail, APPLICATION_TIMEOUT_MS)
    try { this.send(JSON.stringify({ type: 'applicationQuery', requestId: this.current.id, ...job.args })) }
    catch { this.fail() }
  }
}
