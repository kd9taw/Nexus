// Transport selection belongs below the existing application API. The native
// shell and LAN TV path retain their existing behavior. A hosted session owns and
// disposes its adapter; changing views cannot create another station connection.
export interface ApplicationTransport {
  readonly kind: 'remote'
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>
}
let remote: ApplicationTransport | null = null
let generation = 0
const listeners = new Set<() => void>()
export function applicationSessionGeneration(): number { return generation }
export function onApplicationSessionChange(listener: () => void): () => void {
  listeners.add(listener)
  return () => { listeners.delete(listener) }
}
function changed(): void { generation++; for (const listener of listeners) listener() }
export function remoteApplicationTransport(): ApplicationTransport | null { return remote }
export function installApplicationTransport(transport: ApplicationTransport): () => void {
  if (remote && remote !== transport) throw new Error('applicationSessionAlreadyActive')
  remote = transport
  changed()
  return () => { if (remote === transport) { remote = null; changed() } }
}
