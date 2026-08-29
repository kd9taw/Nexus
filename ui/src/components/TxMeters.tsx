// ⚠️ THIS FILE IS ON THE **MIGRATED** LIST (i18n/hardcoded-strings.test.ts): the meter names
// are the rig's own front-panel vocabulary and stay as the constants below, every reading is a
// measurement built here (SWR ratio, ALC %, watts, dB), and the prose around them — the four
// tooltips, the group's name and the idle line — is in the catalog under `meters.tx.*`.
//
// These are READOUTS, not transmit controls: nothing here keys, gates or stops anything.
import { useRef } from 'react'
import type { RadioStatus } from '../types'
import { t } from '../i18n'

/** The rig's own meter names, as they are printed on a radio's front panel — technical tokens
 *  exactly as a mode name is, gathered here so the catalog guard reads them as the deliberate
 *  constants they are. */
const SWR = 'SWR'
const ALC = 'ALC'
const PO = 'PO'
const COMP = 'COMP'

/** Transmit meters (SWR / ALC / Po / COMP) — the mirror image of the RX S-meter: only the
 *  meters the rig actually reports over CAT (each independently capability-gated, so a rig
 *  that reports just SWR shows just SWR). Values are already in engineering units from the
 *  backend; the tiny helpers below turn each into a bar fraction, a display value, and a
 *  severity zone for color.
 *
 *  Two mount modes: default (Phone/CW) appears only while keyed — a manual TX lasts as long
 *  as the operator talks/keys, so the flash-in is fine. `pinned` (the FT Operate cockpit)
 *  renders a compact strip PERMANENTLY: live while transmitting, the last-read values dimmed
 *  between overs. FT cycles key every other slot, and a strip that mounts/unmounts every
 *  15 s made the whole cockpit jump — the operator's "too much movement on screen". */

/** WHEN these meters have anything to show. The panel's own idle line and every ⊞ Panels
 *  entry that offers this panel read this one string, so the menu cannot drift from what
 *  the panel does — and the default (unpinned) variant, which renders nothing at all on
 *  receive, stops looking like a checkbox that does nothing.
 *
 *  ⚠️ DELIBERATELY NOT IN THE CATALOG YET, on the batch-21 ruling: the other ⊞ menu notes on
 *  the same menu live in `features/panelHost.ts` and `waterfall.ts`, and the three cockpits
 *  that pass this one in are not this batch's files either — moving one note of five would
 *  leave a single menu speaking two languages. The idle line below interpolates it whole, so
 *  the panel and the menu still cannot drift, and it moves when that menu does. */
export const TX_METERS_WHEN = 'readings appear on transmit'

type Zone = 'ok' | 'warn' | 'hot'

/* Three zones, three DISTINCT theme tokens. `--ok` and `--danger` were never defined by
   either theme, so both painted their literal fallback; `--state-weak` IS defined, and it is
   the sheet's red — so `warn` rendered #ec5b57 against `hot`'s #e5484d and the two zones were
   indistinguishable, which is the whole job of a warn band. The amber the `#e0a030` fallback
   was reaching for is `--alert-warning`. */
const ZONE_COLOR: Record<Zone, string> = {
  ok: 'var(--state-good)',
  warn: 'var(--alert-warning)',
  hot: 'var(--state-weak)',
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

type MeterRow = { label: string; title: string; bar: ReturnType<typeof swrBar> }

export function TxMeters({
  radio,
  pinned = false,
  inline = false,
}: {
  radio: RadioStatus
  pinned?: boolean
  /** Inline telemetry-cell variant (the Operate strip): pinned semantics — always
   *  rendered, live while keyed, dimmed last readings between overs — inside a
   *  FIXED-WIDTH cell, so the TX cycle causes zero geometry change at zero
   *  vertical cost (the 722ef273 anti-bounce ruling without the dead row). */
  inline?: boolean
}) {
  // Retain the last live readings so the pinned strip has something to show between overs.
  // A plain ref (not state): the poll re-renders us anyway, and retention must never
  // itself cause a render. Unconditional hook — declared before any early return.
  const lastRows = useRef<MeterRow[]>([])

  const rows: MeterRow[] = []
  if (radio.txSwr != null)
    rows.push({ label: SWR, title: t('meters.tx.swr.title'), bar: swrBar(radio.txSwr) })
  if (radio.txAlc != null)
    rows.push({
      label: ALC,
      title: t('meters.tx.alc.title'),
      bar: alcBar(radio.txAlc),
    })
  if (radio.txPoW != null)
    rows.push({ label: PO, title: t('meters.tx.po.title'), bar: poBar(radio.txPoW) })
  if (radio.txCompDb != null)
    rows.push({ label: COMP, title: t('meters.tx.comp.title'), bar: compBar(radio.txCompDb) })

  // ON AIR via the ARBITER, not the FT slot flag (#57): `transmitting` is written only by
  // the slot/beacon path, so a voice or CW over — the overs Phone/CW actually key — never
  // lit this pane even though the SWR/ALC/Po poll runs whenever Nexus keys the rig and the
  // readings were sitting in the snapshot unshown. `txBusyReason` is Some for all seven
  // TX owners.
  const onAir = radio.transmitting || radio.txBusyReason != null || radio.rigKeyed === true
  const live = onAir && rows.length > 0
  if (live) lastRows.current = rows

  const pin = pinned || inline
  const variant = `${pin ? ' pinned' : ''}${inline ? ' inline' : ''}`
  if (!pin) {
    // Default (Phone/CW): appear only while keyed, exactly as before.
    if (!onAir || rows.length === 0) return null
  } else if (!live && lastRows.current.length === 0) {
    // Pinned but no reading has EVER arrived (rig reports no meters, or hasn't keyed yet):
    // a fixed-height hint keeps the panel discoverable without inventing numbers.
    return (
      <div className={`ph-txmeters${variant} idle`} role="group" aria-label={t('meters.tx.aria')}>
        <span className="ph-txmeters-hint">{t('meters.tx.idle', { when: TX_METERS_WHEN })}</span>
      </div>
    )
  }

  const shown = live || !pin ? rows : lastRows.current
  return (
    <div
      className={`ph-txmeters${variant}${!live && pin ? ' idle' : ''}`}
      role="group"
      aria-label={t('meters.tx.aria')}
    >
      {shown.map((r) => (
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
