// THE IOTA ISLAND-GROUP LIST THIS WINDOW HAS — the names shown beside a reference.
//
// The list is IOTA Ltd's (© RSGB and © Islands On The Air Ltd). A copy ships with Nexus by IOTA Ltd's
// permission and the station checks for a newer one at most monthly (`iota_list.rs`); this store only
// reads what the station uses. With the list off every reference is checked by its shape alone, and no
// name is shown. One store for the window, so the Logbook, the contact card, Spots and the offer chip
// agree, and the list is read from the station once, not by every row.
import { getIotaGroups, getIotaListStatus, setIotaListEnabled, type IotaListStatus } from '../api'

/** IOTA Ltd's own site, the link every island name carries. */
export const IOTA_SITE = 'https://www.iota-world.org/'
/** The shape of a reference: a continent code and three digits. */
export const IOTA_SHAPE = /^(AF|AN|AS|EU|NA|OC|SA)-\d{3}$/

let names: Map<string, string> | null = null
let status: IotaListStatus | null = null
let loading: Promise<void> | null = null
const listeners = new Set<() => void>()
const notify = () => listeners.forEach((l) => l())

/**
 * The station's word that the list in use changed (a download it made on its own, or the switch
 * turned in another window): `iota_list.rs` emits it, and this window reads the list again.
 */
export const IOTA_LIST_EVENT = 'iota-list-changed'
let listening = false
function listenForChanges(): void {
  // The global bridge api.ts uses for invoke (`withGlobalTauri`), as `useStationAlerts` does. A
  // window without it (a Remote browser) hears nothing and keeps what it read.
  const listen = window.__TAURI__?.event?.listen
  if (listening || !listen) return
  listening = true
  void (async () => {
    try {
      await listen(IOTA_LIST_EVENT, () => void reloadIotaGroups())
    } catch {
      listening = false
    }
  })()
}

/** Read the station's list and status once (again after `reloadIotaGroups`, or the station's word). */
export function loadIotaGroups(): Promise<void> {
  listenForChanges()
  loading ??= (async () => {
    try {
      const [st, groups] = await Promise.all([getIotaListStatus(), getIotaGroups()])
      status = st
      names = groups.length > 0 ? new Map(groups.map((g) => [g.reference, g.name])) : null
      notify()
    } catch {
      // A station that cannot answer (an older one, a Remote browser) has no list: the shape is
      // the check, and nothing is asked again until `reloadIotaGroups`.
    }
  })()
  return loading
}

/** Read the list again: after the Settings switch changed. */
export function reloadIotaGroups(): Promise<void> {
  loading = null
  return loadIotaGroups()
}

/** The Settings switch; the list is read again after it. */
export async function setIotaListOn(enabled: boolean): Promise<IotaListStatus> {
  const st = await setIotaListEnabled(enabled)
  status = st
  notify()
  await reloadIotaGroups()
  return st
}

/** The group's name, when the list is held and has it. */
export function iotaName(reference: string | null | undefined): string | null {
  if (!reference) return null
  return names?.get(reference.trim().toUpperCase()) ?? null
}

/** Whether a list is in use (the switch is on). */
export function iotaListHeld(): boolean {
  return names !== null
}

/** The station's status for the list, once read. */
export function iotaListStatus(): IotaListStatus | null {
  return status
}

/**
 * Whether a reference is one: by its shape always, and by the list too when one is in use — a
 * reference the list lacks ("EU-999") is no island group. With the list off, the shape is the check.
 */
export function iotaKnown(reference: string): boolean {
  const r = reference.trim().toUpperCase()
  if (!IOTA_SHAPE.test(r)) return false
  return names === null || names.has(r)
}

/** Told whenever the list or the status changes (for `useSyncExternalStore`). */
export function subscribeIotaGroups(l: () => void): () => void {
  listeners.add(l)
  return () => {
    listeners.delete(l)
  }
}

/** A change counter for `useSyncExternalStore`: the store's snapshot is its version. */
let version = 0
listeners.add(() => {
  version += 1
})
export function iotaGroupsVersion(): number {
  return version
}

/** Tests only: forget everything this window holds. */
export function resetIotaGroupsForTest(groups: { reference: string; name: string }[] | null = null, st: IotaListStatus | null = null): void {
  names = groups && groups.length > 0 ? new Map(groups.map((g) => [g.reference, g.name])) : null
  status = st
  loading = groups || st ? Promise.resolve() : null
  listening = false
  notify()
}
