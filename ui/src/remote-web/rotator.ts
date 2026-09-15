// Where the station's rotator is pointing (application v17), as one bounded read-only page.
//
// ⛔ **UNKNOWN IS NOT ZERO.** The station answers `azimuthDeg: null` both when no rotator is
// configured and when rotctld does not answer; `configured` says which. Neither ever becomes a
// number here — a needle drawn from a bearing nobody read is the same defect class as a dial that
// reads 0 Hz because the radio was never asked, and the mast is the one the operator cannot see.
//
// Nothing polls on its own: `useRotatorHeading` reads only while a heading is actually on screen,
// and the station shares one rotctld exchange across every browser watching.
import { useContext, useEffect, useState } from 'react'
import type { QueryPage } from './application-query-protocol'
import { ROTATOR_COMMAND } from './application-query-protocol'
import { RemoteCollectionsContext } from './collections'
import { useStationData } from '../stationAccess'

export type RotatorRead = { configured: boolean; azimuthDeg: number | null }
/** The desktop's own rotor poll interval (RotorStrip, RotorPane). */
export const ROTATOR_POLL_MS = 2_000
const object = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)

export function parseRotator(page: QueryPage): RotatorRead {
  const meta = page.meta
  const source = object(meta) ? meta.source : null
  if (page.collection !== 'rotator' || page.offset !== 0 || page.nextCursor !== null || page.rows.length !== 0 ||
    page.total !== 0 || page.retained !== 0 || !object(meta) || !Number.isSafeInteger(meta.capturedAgeMs) ||
    Number(meta.capturedAgeMs) < 0 || !object(source) || Object.keys(source).length !== 2 ||
    typeof source.configured !== 'boolean' ||
    (source.azimuthDeg !== null && (typeof source.azimuthDeg !== 'number' || !Number.isFinite(source.azimuthDeg) ||
      source.azimuthDeg < 0 || source.azimuthDeg >= 360)) ||
    // A bearing from a station that says it has no rotator is not a reading; it is a contradiction.
    (!source.configured && source.azimuthDeg !== null)) throw new Error('invalidRotator')
  return { configured: source.configured, azimuthDeg: source.azimuthDeg as number | null }
}

/**
 * The station's rotator heading while `active`. Off-screen it reads nothing at all: no interval is
 * armed, and the last reading is dropped so a re-shown strip never paints a bearing from before.
 * A station that does not offer the read (an older Nexus, or an older hosted service) reports
 * `supported: false`, and the strip keeps the honest "—" it has shown until now.
 */
export function useRotatorHeading(active: boolean): RotatorRead & { supported: boolean } {
  const source = useContext(RemoteCollectionsContext), available = useStationData()
  const supported = source?.client.supports(ROTATOR_COMMAND) ?? false
  const [read, setRead] = useState<RotatorRead | null>(null)
  useEffect(() => {
    setRead(null)
    if (!active || !source || !available || !supported) return
    let live = true, timer: ReturnType<typeof setTimeout> | undefined
    const poll = () => {
      source.page({ collection: 'rotator', cursor: null, search: '', unconfirmed: false, after: null })
        .then(page => { if (live) setRead(parseRotator(page)) })
        // rotctld may be down, the link may be busy: the readout just shows —, never a stale bearing.
        .catch(() => { if (live) setRead(null) })
        .finally(() => { if (live) timer = setTimeout(poll, ROTATOR_POLL_MS) })
    }
    poll()
    return () => { live = false; clearTimeout(timer) }
  }, [active, source, available, supported])
  return { supported, configured: read?.configured ?? false, azimuthDeg: read?.azimuthDeg ?? null }
}
