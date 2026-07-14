import type { RadioStatus } from '../types'

/** Transmit meters (SWR / ALC / Po / COMP) — the mirror image of the RX S-meter: shown only
 *  while transmitting, and only the meters the rig actually reports over CAT (each is
 *  independently capability-gated, so a rig that reports just SWR shows just SWR). Values are
 *  already in engineering units from the backend; the tiny helpers below turn each into a bar
 *  fraction, a display value, and a severity zone for color. */

type Zone = 'ok' | 'warn' | 'hot'

const ZONE_COLOR: Record<Zone, string> = {
  ok: 'var(--ok, #2fbf71)',
  warn: 'var(--state-weak, #e0a030)',
  hot: 'var(--danger, #e5484d)',
}

/** SWR ratio → bar (1.0→0 %, 3.0→100 %); warn ≥ 1.5, hot ≥ 2.0 (the "retune / back off" line). */
export function swrBar(swr: number): { frac: number; value: string; zone: Zone } {
  const frac = Math.max(0, Math.min(1, (swr - 1) / 2))
  const zone: Zone = swr >= 2.0 ? 'hot' : swr >= 1.5 ? 'warn' : 'ok'
  return { frac, value: `${swr.toFixed(1)}:1`, zone }
}

/** ALC 0–1 → bar. On SSB some ALC action is normal; a pegged meter means the mic gain is
 *  overdriving the transmitter, so warn as it nears the ceiling and flag hot when pinned. */
export function alcBar(alc: number): { frac: number; value: string; zone: Zone } {
  const frac = Math.max(0, Math.min(1, alc))
  const zone: Zone = alc >= 0.95 ? 'hot' : alc >= 0.8 ? 'warn' : 'ok'
  return { frac, value: `${Math.round(alc * 100)}%`, zone }
}

/** Output power (watts) → bar, scaled to a 100 W reference (2 m full on the IC-9700). */
export function poBar(watts: number): { frac: number; value: string; zone: Zone } {
  const frac = Math.max(0, Math.min(1, watts / 100))
  return { frac, value: `${Math.round(watts)} W`, zone: 'ok' }
}

/** Speech compression (dB) → bar, scaled to ~25 dB full scale; warn past 20 dB (heavy comp). */
export function compBar(db: number): { frac: number; value: string; zone: Zone } {
  const frac = Math.max(0, Math.min(1, db / 25))
  const zone: Zone = db >= 20 ? 'warn' : 'ok'
  return { frac, value: `${Math.round(db)} dB`, zone }
}

export function TxMeters({ radio }: { radio: RadioStatus }) {
  if (!radio.transmitting) return null
  const rows: { label: string; title: string; bar: ReturnType<typeof swrBar> }[] = []
  if (radio.txSwr != null)
    rows.push({ label: 'SWR', title: 'Antenna match — keep it under 2:1', bar: swrBar(radio.txSwr) })
  if (radio.txAlc != null)
    rows.push({
      label: 'ALC',
      title: 'ALC — set mic gain so SSB peaks just tickle the zone, never peg it',
      bar: alcBar(radio.txAlc),
    })
  if (radio.txPoW != null)
    rows.push({ label: 'PO', title: 'Actual output power', bar: poBar(radio.txPoW) })
  if (radio.txCompDb != null)
    rows.push({ label: 'COMP', title: 'Speech compression', bar: compBar(radio.txCompDb) })
  if (rows.length === 0) return null

  return (
    <div className="ph-txmeters" role="group" aria-label="Transmit meters">
      {rows.map((r) => (
        <div key={r.label} className="ph-txmeter" title={r.title}>
          <span className="ph-txmeter-label">{r.label}</span>
          <div className="ph-txmeter-track">
            <div
              className="ph-txmeter-fill"
              style={{ width: `${Math.round(r.bar.frac * 100)}%`, background: ZONE_COLOR[r.bar.zone] }}
            />
          </div>
          <span className="ph-txmeter-value">{r.bar.value}</span>
        </div>
      ))}
    </div>
  )
}
