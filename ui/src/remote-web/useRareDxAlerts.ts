// Browser alerts for rare DX: the station's own Pounce ("new one") alerts, relayed. NOTIFY ONLY.
//
// The browser decides nothing. It shows exactly the alerts the desktop's Pounce detector raised
// (src-tauri/src/pouncer.rs, deciding with propagation::pounce::PounceGate::admit): the station's
// threshold, its per-call/band/mode cooldown and its spot freshness all apply as they do on the
// desktop, and each alert is worded the way the desktop's notification words it (usePounce.ts).
// The read is one argument-free query (application v16); an older station is never asked.
import { useState } from 'react'
import { t } from '../i18n'
import type { PounceAlert } from '../usePounce'
import type { RemoteCollections } from './collections'
import { parsePounce, type PounceRead, type PounceThreshold } from './pounce'
import { postAlert, useAlertOptIn, useAlertPoll, type StationAlertControl } from './browser-alerts'

const STORAGE_KEY = 'nexus.remote.rareDxAlerts'
export const RARE_ALERT_POLL_MS = 15_000
/** One raised alert, keyed as the desktop's own duplicate guard keys it (usePounce.ts). */
const alertKey = (a: PounceAlert): string => `${a.call}|${a.band}|${a.mode}|${a.atUnix}`

/** First read (seen null) is a baseline. After that every alert the station raised since is news
 * once. The station keeps a bounded, append-only list, so the current keys are the whole memory. */
export function freshPounces(seen: Set<string> | null, alerts: PounceAlert[]): { fresh: PounceAlert[]; seen: Set<string> } {
  const next = new Set<string>(), fresh: PounceAlert[] = []
  for (const a of alerts) {
    const key = alertKey(a)
    if (seen && !seen.has(key) && !next.has(key)) fresh.push(a)
    next.add(key)
  }
  return { fresh, seen: next }
}

function announce(fresh: PounceAlert[]): void {
  for (const a of fresh) {
    postAlert(t('remote.b3.rareAlertTitle', { station: a.entity || a.call }),
      a.freqMhz ? t('remote.b3.rareAlertBodyFreq', { call: a.call, freq: a.freqMhz.toFixed(3), mode: a.mode })
        : t('remote.b3.rareAlertBodyBand', { call: a.call, band: a.band, mode: a.mode }),
      `pounce-${a.call}-${a.band}-${a.mode}`)
  }
}

/** `ready`: the station link is current. `offered`: the station serves the v16 alert read. */
export function useRareDxAlerts(source: RemoteCollections | null, ready: boolean, offered: boolean): StationAlertControl {
  const control = useAlertOptIn(STORAGE_KEY)
  const [threshold, setThreshold] = useState<PounceThreshold | null>(null)
  const active = control.enabled && ready && offered && !!source
  useAlertPoll<PounceRead, Set<string>>(active, source, RARE_ALERT_POLL_MS,
    async () => parsePounce(await source!.page({ collection: 'pounce', cursor: null, search: '', unconfirmed: false, after: null })),
    (seen, value) => {
      setThreshold(value.threshold)
      const result = freshPounces(seen, value.alerts)
      announce(result.fresh)
      return result.seen
    })
  return { ...control, offered, note: active && threshold === 'off' ? 'stationOff' : null }
}
