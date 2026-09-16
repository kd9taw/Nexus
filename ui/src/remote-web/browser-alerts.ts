// What every Remote browser alert shares: the operator's opt-in, the site's notification
// permission, the post itself and the baseline poll. NOTIFY ONLY: nothing here tunes, works,
// logs or sends a station command, and clicking an alert only brings this tab forward.
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'

export type AlertToggle = { supported: boolean; blocked: boolean; enabled: boolean; toggle: () => void }
/** An alert whose news comes from a station read. `offered` is false when the station's Nexus is
 * too old to serve that read; `note` explains why an enabled alert is quiet. */
export type StationAlertControl = AlertToggle & { offered: boolean; note: 'stationOff' | 'stale' | null }

// Permission belongs to the site, not to one alert: a refusal on any toggle blocks them all.
const permissionListeners = new Set<() => void>()
const subscribePermission = (listener: () => void) => { permissionListeners.add(listener); return () => { permissionListeners.delete(listener) } }
const notificationsSupported = () => typeof Notification !== 'undefined'
const currentPermission = (): NotificationPermission => notificationsSupported() ? Notification.permission : 'denied'
const stored = (key: string): boolean => { try { return localStorage.getItem(key) === 'on' } catch { return false } }
const store = (key: string, on: boolean): void => { try { localStorage.setItem(key, on ? 'on' : 'off') } catch { /* this session only */ } }

/** Off until the operator clicks. The browser is asked for permission only on that click. */
export function useAlertOptIn(storageKey: string): AlertToggle {
  const supported = notificationsSupported()
  const permission = useSyncExternalStore(subscribePermission, currentPermission, currentPermission)
  const [wanted, setWanted] = useState(() => stored(storageKey))
  const enabled = supported && wanted && permission === 'granted'
  const toggle = useCallback(() => {
    if (!notificationsSupported()) return
    if (enabled) { setWanted(false); store(storageKey, false); return }
    void Promise.resolve(Notification.requestPermission()).then(result => {
      for (const listener of permissionListeners) listener()
      if (result === 'granted') { setWanted(true); store(storageKey, true) }
    }).catch(() => {})
  }, [enabled, storageKey])
  // STABLE IDENTITY, and it is load-bearing rather than tidiness. BrowserApplication's `status`
  // memo takes this object as a dependency, and that memo feeds the `remote` object every cockpit
  // reads; a fresh literal here changes identity on every one of its 500 ms ticks and re-renders
  // the whole workspace at 2 Hz, which reads as the page flashing and swallows clicks. See the
  // contract note above that memo — it was prose, so nothing stopped an unstable dep being added
  // underneath it in 1.13.0.
  const blocked = supported && permission === 'denied'
  return useMemo(() => ({ supported, blocked, enabled, toggle }), [supported, blocked, enabled, toggle])
}

export function postAlert(title: string, body: string, tag: string): void {
  try {
    const note = new Notification(title, { body, tag })
    note.onclick = () => { window.focus(); note.close() }
  } catch { /* a refused notification never takes the page down */ }
}

/** Polls while `active`. The first read after it becomes active (the operator turning the alert
 * on, the station link coming back, a different station source) starts from a null baseline, so
 * whatever is already there never notifies. `advance` returns the next baseline, or null to take
 * a fresh one on the next read. */
export function useAlertPoll<R, S>(active: boolean, source: unknown, intervalMs: number,
  read: () => Promise<R>, advance: (seen: S | null, result: R) => S | null): void {
  const latest = useRef({ read, advance })
  latest.current = { read, advance }
  useEffect(() => {
    if (!active) return
    let live = true, seen: S | null = null, timer: ReturnType<typeof setTimeout> | undefined
    const poll = async () => {
      try {
        const result = await latest.current.read()
        if (!live) return
        seen = latest.current.advance(seen, result)
      } catch { /* the page shows station data failures; the next poll tries again */ }
      if (live) timer = setTimeout(() => void poll(), intervalMs)
    }
    void poll()
    return () => { live = false; clearTimeout(timer) }
  }, [active, source, intervalMs])
}
