// NOAA geomagnetic WATCH heads-up — one quiet toast per watch, ever.
//
// A watch is NOAA's forecasters saying a storm is expected on named UT days (a CME or a
// coronal-hole stream on its way). It is announced once, quietly (an info toast, no beep), the
// first time this computer sees its serial number; the map's countdown chip does the rest. The
// serials already announced are kept in localStorage, so a restart, a second window or the next
// poll never repeats one — and a NEW watch (NOAA reissues one when the forecast moves, with a
// new serial) is news and does announce.
//
// ⚠️ NOTIFY, NEVER ACT (stormAlert.ts's rule): nothing here touches the radio.
import { pushToast } from './toast'
import { t } from './i18n'
import type { ArrivalOutlook } from './types'
import { watchDays } from './components/prop/ArrivalChip'

/** Where the announced serials are kept. */
export const SEEN_KEY = 'nexus.arrival.watchSeen'
/** Enough history to outlast any watch's life; the oldest fall off. */
const SEEN_MAX = 50

function readSeen(): number[] {
  try {
    const raw = localStorage.getItem(SEEN_KEY)
    const v: unknown = raw ? JSON.parse(raw) : []
    return Array.isArray(v) ? v.filter((n): n is number => typeof n === 'number') : []
  } catch {
    return []
  }
}

/** Toast a watch in force the first time its serial is seen. Call on every outlook answer. */
export function processArrivalWatch(o: ArrivalOutlook | null | undefined, nowSec = Date.now() / 1000): void {
  const w = o?.watch
  if (!w?.window || w.window[1] <= nowSec) return
  const seen = readSeen()
  if (seen.includes(w.serial)) return
  try {
    localStorage.setItem(SEEN_KEY, JSON.stringify([...seen, w.serial].slice(-SEEN_MAX)))
  } catch {
    // Storage refused (private mode, quota): this window still announces once per serial below.
  }
  if (sessionSeen.has(w.serial)) return
  sessionSeen.add(w.serial)
  pushToast(t('prop.arrivalAlert.watch', { g: w.maxG, days: watchDays(w.window) }), 'info', 12000, { alert: true })
}

/** This window's own record, for when storage refuses a write. */
const sessionSeen = new Set<number>()

/** Test hook. */
export function resetArrivalAlerts(): void {
  sessionSeen.clear()
}
