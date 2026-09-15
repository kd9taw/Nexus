// Browser alerts for new needs on the station's Needed board. NOTIFY ONLY: nothing here
// tunes, works, logs or moves the radio, and clicking an alert only brings this tab forward.
// Reads ride the same bounded `needs` collection the board already polls, so enabling alerts
// adds no station command. The operator turns them on with a click; the browser asks first.
import type { NeedAlert, Settings } from '../types'
import { visibleNeeds } from '../features/needs'
import { readEnabledModes } from '../useFeatures'
import { t } from '../i18n'
import type { RemoteCollections } from './collections'
import { postAlert, useAlertOptIn, useAlertPoll, type AlertToggle } from './browser-alerts'

const STORAGE_KEY = 'nexus.remote.needAlerts'
export const NEED_ALERT_POLL_MS = 30_000
/** Alerts shown one by one per read; the rest are counted in a single summary. */
const INDIVIDUAL = 3
const SEEN_LIMIT = 4096

export type NeedAlertsControl = AlertToggle
export const needKey = (a: NeedAlert): string => `${a.call.trim().toUpperCase()}|${a.band}|${a.mode}`

/** The first read after alerts start, or after the station link comes back, is a baseline:
 * what is already on the board is not news, so a replayed board never notifies. After that
 * a call/band/mode notifies once for the session, strongest need first. */
export function newNeeds(seen: Set<string> | null, rows: NeedAlert[]): { fresh: NeedAlert[]; seen: Set<string> } {
  const next = new Set(seen ?? [])
  const fresh: NeedAlert[] = []
  for (const row of rows) {
    const key = needKey(row)
    if (next.has(key)) continue
    next.add(key)
    if (seen) fresh.push(row)
  }
  for (const key of next) { if (next.size <= SEEN_LIMIT) break; next.delete(key) }
  return { fresh: fresh.sort((a, b) => b.priority - a.priority), seen: next }
}

function announce(fresh: NeedAlert[]): void {
  for (const a of fresh.slice(0, INDIVIDUAL)) {
    postAlert(t('remote.b3.needAlertTitle', { station: a.entity || a.call }),
      a.freqMhz ? t('remote.b3.needAlertBodyFreq', { call: a.call, freq: a.freqMhz.toFixed(3), mode: a.mode, need: a.headline })
        : t('remote.b3.needAlertBodyBand', { call: a.call, band: a.band, mode: a.mode, need: a.headline }),
      `nexus-need-${needKey(a)}`)
  }
  if (fresh.length > INDIVIDUAL) postAlert(t('remote.b3.needAlertMoreTitle'), t('remote.b3.needAlertMore', { count: fresh.length - INDIVIDUAL }), 'nexus-need-more')
}

/** `ready` is true only while the station link is current and offers the board. */
export function useNeedAlerts(source: RemoteCollections | null, ready: boolean, settings: Settings | null): NeedAlertsControl {
  const control = useAlertOptIn(STORAGE_KEY)
  // The same band scopes the board and roster honour (Settings ▸ Spots & Alerts).
  const scopes = { dxcc: settings?.alertDxccBands, grid: settings?.alertGridBands, rareGrid: settings?.alertRareGridBands }
  useAlertPoll<Awaited<ReturnType<RemoteCollections['read']>>, Set<string>>(control.enabled && ready && !!source, source, NEED_ALERT_POLL_MS,
    () => source!.read('needs'),
    (seen, { rows }) => {
      const result = newNeeds(seen, visibleNeeds(rows as unknown as NeedAlert[], readEnabledModes(), scopes))
      announce(result.fresh)
      return result.seen
    })
  return control
}
