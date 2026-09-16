// Browser alerts for new POTA activations: the desktop's "New POTA activation" alert
// (Settings ▸ Spots & Alerts), relayed. NOTIFY ONLY: nothing here fetches, hunts, tunes or logs.
//
// The decision is the desktop's own: `newlySpottedRefs` (features/potaAlert.ts), a set-diff on
// park reference, over the same station cache the desktop alert polls (SharedOtaSpots "POTA").
// The browser reads that cache through the existing hunter-feed query (application v9), so this
// adds no station command. A station read can only ever see the cache, never fill it: when the
// station has no fresh POTA spots, the alert says so and takes a new baseline when they return,
// so parks that came on the air while nobody could see them never arrive as a burst.
import { useMemo, useState } from 'react'
import { t } from '../i18n'
import { newlySpottedRefs } from '../features/potaAlert'
import type { OtaSpot } from '../types'
import type { RemoteCollections } from './collections'
import { parseOta } from './ota'
import { postAlert, useAlertOptIn, useAlertPoll, type StationAlertControl } from './browser-alerts'

const STORAGE_KEY = 'nexus.remote.potaAlerts'
/** The desktop alert's own poll cadence (App.tsx). */
export const POTA_ALERT_POLL_MS = 120_000
/** Alerts shown one by one per read; the rest are counted in a single summary. */
const INDIVIDUAL = 3

function announce(fresh: string[], spots: OtaSpot[]): void {
  const parks = [...new Set(fresh)].flatMap(reference => spots.find(s => s.reference === reference) ?? [])
  for (const s of parks.slice(0, INDIVIDUAL)) {
    postAlert(t('remote.b3.potaAlertTitle', { reference: s.reference }),
      t('remote.b3.potaAlertBody', { activator: s.activator, name: s.name, freq: (s.freqKhz / 1000).toFixed(3), mode: s.mode }),
      `nexus-pota-${s.reference}`)
  }
  if (parks.length > INDIVIDUAL) postAlert(t('remote.b3.potaAlertMoreTitle'), t('remote.b3.potaAlertMore', { count: parks.length - INDIVIDUAL }), 'nexus-pota-more')
}

/** `ready`: the station link is current. `offered`: the station serves the hunter-feed read. */
export function usePotaAlerts(source: RemoteCollections | null, ready: boolean, offered: boolean): StationAlertControl {
  const control = useAlertOptIn(STORAGE_KEY)
  const [stale, setStale] = useState(false)
  const active = control.enabled && ready && offered && !!source
  useAlertPoll<OtaSpot[] | null, Set<string>>(active, source, POTA_ALERT_POLL_MS,
    async () => {
      const feed = parseOta(await source!.page({ collection: 'ota', cursor: null, search: '', unconfirmed: false, after: null })).feeds.find(f => f.program === 'POTA')
      return feed?.status === 'ready' ? feed.spots : null
    },
    (seen, spots) => {
      setStale(spots === null)
      if (!spots) return null
      const { fresh, next } = newlySpottedRefs(seen ?? new Set(), spots)
      if (seen) announce(fresh, spots)
      return next
    })
  // Stable identity — see the note in browser-alerts.ts: this object is a dependency of the memo
  // that feeds every cockpit, so a fresh literal here re-renders the workspace twice a second.
  const note: StationAlertControl['note'] = active && stale ? 'stale' : null
  return useMemo(() => ({ ...control, offered, note }), [control, offered, note])
}
