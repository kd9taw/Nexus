// ⚠️ THIS FILE IS ON THE **MIGRATED** LIST (i18n/hardcoded-strings.test.ts): the meter names
// are the rig's own front-panel vocabulary and stay as the constants below, every reading is a
// measurement built here (SWR ratio, ALC %, watts, dB), and the prose around them — the five
// tooltips, the group's name and the idle line — is in the catalog under `meters.tx.*`.
//
// These are READOUTS, not transmit controls: nothing here keys, gates or stops anything.
import { useRef, useState } from 'react'
import type { KeyboardEvent, PointerEvent } from 'react'
import type { RadioStatus } from '../types'
import { isOnAir } from '../types'
import { t } from '../i18n'
import { fmtTempC, useUnits } from '../units'
import { useTxMeterStyle } from '../txMeterStyle'
import { NEEDLE_KEYS, TxNeedles, type NeedleKey, type NeedleRow } from './TxNeedles'

/** The rig's own meter names, as they are printed on a radio's front panel — technical tokens
 *  exactly as a mode name is, gathered here so the catalog guard reads them as the deliberate
 *  constants they are. */
const SWR = 'SWR'
const ALC = 'ALC'
const PO = 'PO'
const COMP = 'COMP'
/** The radio's speech-processor switch, as Yaesu's and Kenwood's front panels print it. */
const PROC = 'PROC'
/** The temperature rows: the radio's own PA when it is the only one, `RIG` beside an
 *  amplifier's so the two cannot be mistaken for each other, and the amplifier's. */
const TEMP = 'TEMP'
const RIG = 'RIG'
const AMP = 'AMP'
/** Arrow-key step for the processor level, in percent — the cockpit's other level controls'. */
const COMP_KEY_STEP = 5

/** The SWR token with the mark that says Nexus cannot vouch for this rig's scale. It rides on
 *  the LABEL because that is the one part of the row whose width nothing else is pinned to:
 *  the label is a flex item over a min-width and the track beside it is `flex: 1`, so one more
 *  character can only shorten the bar by its own width. The row's height, the value column and
 *  the Operate cell's fixed width are all untouched, which is what the anti-bounce ruling below
 *  is actually about. Locale-independent, exactly as the token it marks is. */
const SWR_UNSCALED = 'SWR?'

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

/** `unknown` is not a fourth severity — it is the ABSENCE of a severity, for a reading whose
 *  scale Nexus cannot stand behind (see the SWR row below). */
type Zone = 'ok' | 'warn' | 'hot' | 'unknown'

/* Three zones, three DISTINCT theme tokens. `--ok` and `--danger` were never defined by
   either theme, so both painted their literal fallback; `--state-weak` IS defined, and it is
   the sheet's red — so `warn` rendered #ec5b57 against `hot`'s #e5484d and the two zones were
   indistinguishable, which is the whole job of a warn band. The amber the `#e0a030` fallback
   was reaching for is `--alert-warning`. `--state-pending` is defined by BOTH themes too
   (checked against that same trap) and already means "no verdict yet" across the sheet. */
const ZONE_COLOR: Record<Zone, string> = {
  ok: 'var(--state-good)',
  warn: 'var(--alert-warning)',
  hot: 'var(--state-weak)',
  unknown: 'var(--state-pending)',
}

/** SWR ratio → bar (1.0→0 %, 3.0→100 %); warn ≥ 1.5, hot ≥ 2.0 (the "retune / back off" line).
 *  Those two thresholds are true of an SWR, not of every rig's REPORTED SWR — the caller
 *  discards the zone where the scale is unverified; see the row below. */
export function swrBar(swr: number): { frac: number; value: string; zone: Zone } {
  const frac = Math.max(0, Math.min(1, (swr - 1) / 2))
  const zone: Zone = swr >= 2.0 ? 'hot' : swr >= 1.5 ? 'warn' : 'ok'
  return { frac, value: `${swr.toFixed(1)}:1`, zone }
}

/** ALC 0–1 → bar. On SSB some ALC action is normal; a pegged meter means the mic gain is
 *  overdriving the transmitter, so warn as it nears the ceiling and flag hot when pinned.
 *
 *  `zoneTop` is where the radio's own ALC zone ends on this bar (`txAlcZone`): 1.0 where the
 *  reading's 1.0 is the zone top (Hamlib's ALC scales, Nexus's own Icom path), 0.5 on the
 *  FT-991A, FT-710 and FTDX10, read on their own full bar with the zone ending at half scale.
 *  The colours keep their place in the zone (amber from 80 % of it, red from 95 %), so they sit
 *  where the radio's own meter marks the zone. The bar itself stays where the radio's bar is. */
export function alcBar(alc: number, zoneTop = 1): { frac: number; value: string; zone: Zone } {
  const frac = Math.max(0, Math.min(1, alc))
  const top = zoneTop > 0 ? zoneTop : 1
  const zone: Zone = alc >= 0.95 * top ? 'hot' : alc >= 0.8 * top ? 'warn' : 'ok'
  return { frac, value: `${Math.round(alc * 100)}%`, zone }
}

/** Output power (watts) → bar, scaled to a 100 W reference (2 m full on the IC-9700). */
export function poBar(watts: number, ratedW = 100): { frac: number; value: string; zone: Zone } {
  // ⭐ FULL SCALE IS THE RADIO'S RATED OUTPUT, not a hardcoded 100 (operator, 2026-09-21).
  // `watts / 100` is wrong for exactly the two operators most likely to be watching: a 5 W QRP
  // set never left the first tick and a station behind an amplifier pinned. `ratedWatts` is
  // per-radio and defaults to 100, so most HF stations are unaffected.
  //
  // ⚠️ Fixed in the SHARED component on purpose, unlike the four-rows contract that had no
  // business here: this is a CORRECTNESS defect and it is identical in all three cockpits — a
  // QRP rig's power bar is as wrong in Operate as it is in Phone. A ruling about one surface
  // does not belong in a component three surfaces share; a fix every surface needs does.
  const full = ratedW > 0 ? ratedW : 100
  const frac = Math.max(0, Math.min(1, watts / full))
  return { frac, value: `${Math.round(watts)} W`, zone: 'ok' }
}

/** Speech compression (dB) → bar, scaled to ~25 dB full scale; warn past 20 dB (heavy comp). */
export function compBar(db: number): { frac: number; value: string; zone: Zone } {
  const frac = Math.max(0, Math.min(1, db / 25))
  const zone: Zone = db >= 20 ? 'warn' : 'ok'
  return { frac, value: `${Math.round(db)} dB`, zone }
}

/** One row of the panel. `bar` null = no reading, which the row prints as a dash — see the
 *  panel's note on the two different kinds of nothing a dash can mean. `key` is stable across
 *  that and across the SWR label's verified/unverified flip, so the "has this rig ever
 *  reported this meter" memory below cannot be fooled by a label change. */
type MeterKey = 'swr' | 'alc' | 'po' | 'comp'
type MeterRow = {
  key: MeterKey
  label: string
  title: string
  bar: ReturnType<typeof swrBar> | null
}

/** What the COMP row may set, given by a host that owns the Phone chain. Absent (CW, Operate),
 *  the row is a meter and nothing else. Each control also appears only where the radio has
 *  READ BACK the value it sets: a processor level, a processor switch, a transmit-bandwidth step
 *  list — a control the radio never answered would be one Nexus cannot show the state of. */
export interface TxMeterControls {
  /** Set the speech-processor level, 0..1. */
  onCompLevel?: (frac: number) => void
  /** Switch the speech processor on or off. */
  onProc?: (on: boolean) => void
  /** Pick a transmit-bandwidth step by its name in `radio.txBwSteps`. */
  onTxBw?: (step: string) => void
}

/** A temperature row: the radio's own PA, or an amplifier's. Not a transmit meter — it is read
 *  while receiving, so it is shown whenever a reading exists and takes no part in the ⊘ logic. */
type TempRow = { key: 'rigTemp' | 'ampTemp'; label: string; title: string; frac: number; value: string }

export function TxMeters({
  radio,
  pinned = false,
  inline: inlineProp = false,
  strip = false,
  controls,
}: {
  radio: RadioStatus
  pinned?: boolean
  /** The COMP row's controls; see [`TxMeterControls`]. */
  controls?: TxMeterControls
  /** Inline telemetry-cell variant (the Operate strip): pinned semantics — always
   *  rendered, live while keyed, dimmed last readings between overs — inside a
   *  FIXED-WIDTH cell, so the TX cycle causes zero geometry change at zero
   *  vertical cost (the 722ef273 anti-bounce ruling without the dead row). */
  inline?: boolean
  /** The TX strip's cell (Phone and CW, the compact dock): the inline variant's semantics, laid
   *  out in at most two rows that grow sideways, so the strip is never taller than its buttons. */
  strip?: boolean
}) {
  const inline = inlineProp || strip
  // Retain the last live readings so the pinned strip has something to show between overs.
  // A plain ref (not state): the poll re-renders us anyway, and retention must never
  // itself cause a render. Unconditional hook — declared before any early return.
  const lastRows = useRef<MeterRow[]>([])
  // ⭐ WHICH METERS THIS RIG ACTUALLY HAS, and whether we are yet in a position to say.
  //
  // These fields are populated ONLY while transmitting, so a blank row means two different
  // things at two different times: before the first over nothing is known about any meter,
  // and after one, a meter that stayed silent through it is one the radio does not report.
  // Telling those apart is the whole reason the row can carry a ⊘ at all — a mark hung on
  // the first case would be writing a radio off for never having been asked.
  const seen = useRef(new Set<MeterKey>())
  const everRead = useRef(false)
  // The verdict is due only once an over that read something has ENDED. The keyed poll reads
  // one meter per 150 ms, so 150 ms into the first over ALC/Po/COMP are meters not yet asked,
  // not meters the rig lacks, and a ⊘ hung on them then would flash and go as they arrive.
  const overJudged = useRef(false)
  // Has the rig keyed since this panel mounted? The Operate cell gives every meter its slot
  // from that moment on (below).
  const keyedOnce = useRef(false)
  const units = useUnits()
  // #343: Settings ▸ Workspace ▸ TX meters. Needle draws PO, ALC and SWR as needles (TxNeedles) in
  // place of their bars; COMP and the temperatures stay bars. Bars is the default.
  const needle = useTxMeterStyle() === 'needle'
  // The processor level while a drag is in flight: the marker follows the pointer at once.
  const [dragPct, setDragPct] = useState<number | null>(null)
  // The level last set here, held on the marker until the radio's reported level moves. Mid-over
  // the radio is not read back (that happens between overs), so without this the marker would
  // snap back to the level from before the over while the radio already has the new one.
  const [held, setHeld] = useState<{ pct: number; base: number | null } | null>(null)
  if (held && held.base !== (radio.compLevel ?? null)) setHeld(null)

  const rows: MeterRow[] = []
  {
    // ⚠️ IS THIS NUMBER ON A SCALE NEXUS CAN STAND BEHIND? The engine takes that question
    // seriously — it refuses to arm the high-SWR cutoff without `swrScaleVerified` — and so
    // does Settings. This panel did not, which made it the loudest surface asserting a
    // threshold over the one number the engine will not act on. On a Xiegu, 1.2:1 on the
    // rig's own front panel arrives here as 6:1 (#292): a red bar and "keep it under 2:1",
    // both wrong, in front of an operator who is transmitting.
    //
    // ABSENT READS AS UNVERIFIED (`!== true`). The field is optional on the DTO and
    // per-active-radio; SettingsPanel's three readers all test it the same way, for the same
    // reason — resolving a missing value to "verified" is the one direction that puts the
    // false red bar back.
    //
    // Unverified, TWO THINGS GO and one stays. The ok/warn/hot zoning (a threshold at 1.5
    // and 2.0) and the title's "keep it under 2:1" are the same claim about an absolute
    // scale in two media, and neither is supportable, so the bar goes unzoned and the title
    // is replaced by one that says what the figure is and is not. The READING STAYS: it is
    // repeatable, so the operator can still watch it move while they tune, and a meter that
    // rendered as nothing would be indistinguishable from one Nexus never built. Which is
    // also why the lost colour cannot be the only signal — `SWR?` is the visible mark.
    //
    // The value keeps its `:1`: the doubt is about the rig's CALIBRATION, not about which
    // quantity this is, and dropping the unit would cost readability without buying honesty.
    const verified = radio.swrScaleVerified === true
    const bar = radio.txSwr == null ? null : swrBar(radio.txSwr)
    rows.push({
      key: 'swr',
      // ⚠️ THE `SWR?` MARK NEEDS A READING TO BE ABOUT. It says "do not trust this number's
      // absolute scale", and with no number there is nothing to distrust — a blank row
      // wearing it reads as a doubt about whether the meter EXISTS, which is the ⊘'s
      // question and a different one.
      label: bar == null || verified ? SWR : SWR_UNSCALED,
      title: verified ? t('meters.tx.swr.title') : t('meters.tx.swr.unverified'),
      bar: bar == null || verified ? bar : { ...bar, zone: 'unknown' },
    })
  }
  rows.push({
    key: 'alc',
    label: ALC,
    title: t('meters.tx.alc.title'),
    bar: radio.txAlc == null ? null : alcBar(radio.txAlc, radio.txAlcZone ?? 1),
  })
  rows.push({
    key: 'po',
    label: PO,
    title: t('meters.tx.po.title'),
    bar: radio.txPoW == null ? null : poBar(radio.txPoW, radio.ratedWatts ?? 100),
  })
  rows.push({
    key: 'comp',
    label: COMP,
    title: t('meters.tx.comp.title'),
    bar: radio.txCompDb == null ? null : compBar(radio.txCompDb),
  })

  // ON AIR via the ARBITER, not the FT slot flag (#57): `transmitting` is written only by
  // the slot/beacon path, so a voice or CW over — the overs Phone/CW actually key — never
  // lit this pane even though the SWR/ALC/Po poll runs whenever Nexus keys the rig and the
  // readings were sitting in the snapshot unshown. `txBusyReason` is Some for all seven
  // TX owners.
  const onAir = isOnAir(radio)
  const reading = rows.some((r) => r.bar != null)
  const live = onAir && reading
  if (live) {
    lastRows.current = rows
    everRead.current = true
    for (const r of rows) if (r.bar != null) seen.current.add(r.key)
  }
  if (onAir) keyedOnce.current = true
  else if (everRead.current) overJudged.current = true

  // ⭐ FINALS TEMPERATURE (operator request; #247). The radio's own reading, °C on the wire, shown
  // in the operator's units. The amplifier's beside it, reusing the reading its own pane shows.
  // Neither has a zone colour: a safe temperature is the radio's own business, and its limits
  // differ by model, so a red bar here would be a threshold Nexus cannot stand behind.
  const temps: TempRow[] = []
  const amp = radio.amp?.linked ? radio.amp : null
  const ampTemp = amp?.temp ?? null
  if (radio.paTempC != null) {
    temps.push({
      key: 'rigTemp',
      label: ampTemp != null ? RIG : TEMP,
      title: t('meters.tx.temp.rig.title'),
      frac: Math.max(0, Math.min(1, radio.paTempC / 100)),
      value: fmtTempC(radio.paTempC, units),
    })
  } else if (radio.paTempMeter != null) {
    // A meter and no degrees: the radio's own TEMP bar, as a share of its scale. Inventing °C
    // from it would be a number the radio never stated.
    temps.push({
      key: 'rigTemp',
      label: ampTemp != null ? RIG : TEMP,
      title: t('meters.tx.temp.meter.title'),
      frac: Math.max(0, Math.min(1, radio.paTempMeter)),
      value: `${Math.round(radio.paTempMeter * 100)}%`,
    })
  }
  if (amp && ampTemp != null) {
    // SPE's protocol does not say whether its number is °C or °F (the amplifier sends whatever
    // its own panel is set to), so that number is shown bare, unconverted and unbarred — the
    // amplifier pane's rule. The KPA's is documented Celsius and converts like the radio's.
    temps.push(
      amp.tempCelsius
        ? {
            key: 'ampTemp',
            label: AMP,
            title: t('meters.tx.temp.amp.title'),
            frac: Math.max(0, Math.min(1, ampTemp / 100)),
            value: fmtTempC(ampTemp, units),
          }
        : { key: 'ampTemp', label: AMP, title: t('amp.temp.unknownScale'), frac: 0, value: `${ampTemp}°` },
    )
  }
  const tempRows = temps.map((r) => (
    <div key={r.key} className="ph-txmeter temp" data-meter={r.key} title={r.title}>
      <span className="ph-txmeter-label">{r.label}</span>
      <div className="ph-txmeter-track well">
        <div className="ph-txmeter-fill" style={{ width: `${Math.round(r.frac * 100)}%`, background: ZONE_COLOR.unknown }} />
      </div>
      <span className="ph-txmeter-value">{r.value}</span>
    </div>
  ))

  // ⭐ THE COMP ROW SETS WHAT IT SHOWS (operator request). The bar stays the compression METER;
  // the marker on it is the processor LEVEL, and dragging the bar (or the arrow keys) moves the
  // level. The two share a track, not a scale: the meter is dB the radio measured, the marker the
  // knob's position — which is why the marker is a line, not a second fill.
  const levelPct =
    controls?.onCompLevel && radio.compLevel != null
      ? dragPct ?? held?.pct ?? Math.round(radio.compLevel * 100)
      : null
  const setLevel = (pct: number) => {
    const p = Math.max(0, Math.min(100, Math.round(pct)))
    controls?.onCompLevel?.(p / 100)
    return p
  }
  const pctAt = (e: PointerEvent<HTMLDivElement>) => {
    const box = e.currentTarget.getBoundingClientRect()
    return box.width > 0 ? ((e.clientX - box.left) / box.width) * 100 : 0
  }
  const onLevelDown = (e: PointerEvent<HTMLDivElement>) => {
    e.currentTarget.setPointerCapture?.(e.pointerId)
    setDragPct(setLevel(pctAt(e)))
  }
  const onLevelMove = (e: PointerEvent<HTMLDivElement>) => {
    if (dragPct == null) return
    const p = Math.max(0, Math.min(100, Math.round(pctAt(e))))
    if (p !== dragPct) setDragPct(setLevel(p))
  }
  const onLevelUp = () => {
    if (dragPct != null) setHeld({ pct: dragPct, base: radio.compLevel ?? null })
    setDragPct(null)
  }
  const onLevelKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (levelPct == null) return
    const next =
      e.key === 'ArrowRight' || e.key === 'ArrowUp'
        ? levelPct + COMP_KEY_STEP
        : e.key === 'ArrowLeft' || e.key === 'ArrowDown'
          ? levelPct - COMP_KEY_STEP
          : e.key === 'Home'
            ? 0
            : e.key === 'End'
              ? 100
              : null
    if (next == null) return
    e.preventDefault()
    setHeld({ pct: setLevel(next), base: radio.compLevel ?? null })
  }
  // PROC only where the radio loop says the switch's write is the maker's own command (the native
  // Icom link's `16 44`): elsewhere the processor stays on the chain's own COMP control.
  const procOn = controls?.onProc && radio.txProcSwitch && radio.comp != null ? radio.comp === true : null
  const bwSteps = controls?.onTxBw && radio.txBw ? (radio.txBwSteps ?? []) : []
  const bwNext = () => {
    const i = bwSteps.indexOf(radio.txBw ?? '')
    controls?.onTxBw?.(bwSteps[(i + 1) % bwSteps.length])
  }
  const compControls = (
    <>
      {procOn != null && (
        <button
          type="button"
          className={`ph-txmeter-btn${procOn ? ' on' : ''}`}
          data-ctl="proc"
          aria-pressed={procOn}
          title={t('meters.tx.proc.title')}
          onClick={() => controls?.onProc?.(!procOn)}
        >
          {PROC}
        </button>
      )}
      {bwSteps.length > 1 && (
        <button
          type="button"
          className="ph-txmeter-btn"
          data-ctl="txbw"
          // Never while on the air: the radio's own menu write is not one Nexus will make mid-over.
          disabled={isOnAir(radio)}
          title={isOnAir(radio) ? t('meters.tx.txbw.busy') : t('meters.tx.txbw.title', { step: radio.txBw ?? '' })}
          aria-label={t('meters.tx.txbw.aria', { step: radio.txBw ?? '' })}
          onClick={bwNext}
        >
          {radio.txBw}
        </button>
      )}
    </>
  )

  const pin = pinned || inline
  if (!pin) {
    // Default (Phone/CW's old shape): appear only while keyed, exactly as before. It has no
    // caller left in the app — both cockpits pass `pinned` — so it deliberately does NOT
    // inherit the always-four-rows contract, which was decided for the dock and is about the
    // dock's height.
    if (!onAir || !reading) return null
  }

  // ⭐ FOUR ROWS, ALWAYS, and COMPACT UNTIL KEYED (operator, 2026-09-20).
  //
  // The panel used to be 0-4 rows tall depending on the radio and on whether it had keyed
  // yet, in a BOTTOM-ANCHORED dock above the PTT button — so every one of those transitions
  // moved the dock's top edge, and the key-down ones moved it while the operator was holding
  // the button. Fixed at four there is nothing left to move, and the idle/keyed switch below
  // is then free to be a real size change: the reading you squint at is the one you are
  // making right now.
  //
  // It is a VARIANT SWITCH over CSS that already exists — `.pinned` is the compact geometry
  // (6px bars, smaller type) and the bare class is the full one (10px) — not new geometry.
  // `inline` keeps the compact geometry THROUGH a key-down: that cell is fixed-width chrome
  // in the Operate strip and the anti-bounce ruling is that the TX cycle costs it nothing.
  const compact = inline || !live
  const variant = `${compact ? ' pinned' : ''}${inline ? ' inline' : ''}${strip ? ' strip' : ''}`
  const shown = live || lastRows.current.length === 0 ? rows : lastRows.current

  // ⛔ THE FOUR-ROWS-ALWAYS CONTRACT IS THE DOCK'S, AND IT DOES NOT REACH THIS CELL.
  //
  // The ruling above was taken for the Phone/CW dock, for a reason that is entirely about the
  // dock: it is BOTTOM-ANCHORED above the PTT button, so a row appearing moves the button under
  // a held pointer. `inline` is the Operate strip's `.cq-telemetry` cell — fixed-width chrome in
  // a TOP-anchored column, where nothing is anchored to the panel's bottom edge and the cost of
  // a taller panel is paid by whatever sits BELOW it instead.
  //
  // ⚠️ AND IT WAS PAID, on public main. Making the four rows unconditional grew Operate's cell
  // from ONE hint line to four rows plus the hint, which pushed the recall card 5 px past the
  // fold at 1280×800 and reddened two compiled-browser shards (`cardBottom` 806 against a
  // viewport of 800; `remote/test/browser.test.mjs:2689`). The contract was right and its SCOPE
  // was wrong — a shared component inherited a ruling made about one of its three hosts.
  //
  // So the cell shows the hint ALONE until the rig first keys. From that moment it holds all
  // four slots, each empty until its meter reads, in a fixed two-track grid (`.inline` in
  // styles.css), and keeps them between overs (operator, 2026-10-17). It used to show only the
  // meters that had read, in an `auto-fit` grid: the poll reads one meter per 150 ms, so the
  // first over showed SWR alone and that one bar spanned the whole cell, then shrank to a third
  // as the others arrived. Four slots in two rows is the height a four-meter rig already had.
  // …EXCEPT IN THE TX STRIP (`strip`, Phone and CW from md up): there the meters are the dock's, moved,
  // and Phone's COMP row carries the processor level, PROC and the TX bandwidth, which an operator sets
  // BEFORE the first over. So the strip keeps the dock's four slots from the start, in the compact
  // geometry, without the hint line (a cell of its own in the strip's two rows); the rows' dashes
  // and their titles say the same until a reading arrives.
  if (inline && !strip && !keyedOnce.current) {
    return (
      <div className={`ph-txmeters${variant} idle`} role="group" aria-label={t('meters.tx.aria')}>
        <span className="ph-txmeters-hint">{t('meters.tx.idle', { when: TX_METERS_WHEN })}</span>
        {tempRows}
      </div>
    )
  }
  return (
    <div
      className={`ph-txmeters${variant}${!live && pin ? ' idle' : ''}${needle ? ' needle' : ''}`}
      role="group"
      aria-label={t('meters.tx.aria')}
    >
      {needle && (
        <TxNeedles
          rows={rows.filter((r): r is NeedleRow => isNeedle(r.key))}
          live={live}
          swrVerified={radio.swrScaleVerified === true}
          alcZoneTop={radio.txAlcZone ?? 1}
          unreported={overJudged.current ? NEEDLE_KEYS.filter((k) => !seen.current.has(k)) : []}
        />
      )}
      {/* With needles, Operate's fixed-width cell has room for the three faces only, so its COMP slot
          goes (FT does not use the speech processor); everywhere else COMP stays a bar beside them. */}
      {shown.filter((r) => !needle || (!isNeedle(r.key) && !(r.key === 'comp' && inline && !strip))).map((r) => {
        const slider = r.key === 'comp' && levelPct != null
        const hasCtl = r.key === 'comp' && (slider || procOn != null || bwSteps.length > 1)
        return (
        <div
          key={r.key}
          className={`ph-txmeter${slider ? ' settable' : ''}${hasCtl ? ' has-ctl' : ''}`}
          title={slider ? t('meters.tx.comp.level.title', { level: levelPct }) : r.title}
        >
          <span className="ph-txmeter-label">{r.label}</span>
          {/* A display well (styles.css DISPLAY WELLS), in all three hosts alike: the zone colours
              above resolve to the dark theme's inks on a track that is dark in both themes. */}
          <div
            className="ph-txmeter-track well"
            {...(slider
              ? {
                  role: 'slider',
                  tabIndex: 0,
                  'aria-label': t('meters.tx.comp.level.aria'),
                  'aria-valuemin': 0,
                  'aria-valuemax': 100,
                  'aria-valuenow': levelPct,
                  'aria-valuetext': `${levelPct}%`,
                  onPointerDown: onLevelDown,
                  onPointerMove: onLevelMove,
                  onPointerUp: onLevelUp,
                  onPointerCancel: onLevelUp,
                  onKeyDown: onLevelKey,
                }
              : {})}
          >
            <div
              className="ph-txmeter-fill"
              style={{
                width: `${Math.round((r.bar?.frac ?? 0) * 100)}%`,
                background: ZONE_COLOR[r.bar?.zone ?? 'unknown'],
              }}
            />
            {slider && <div className="ph-txmeter-mark" style={{ left: `${levelPct}%` }} />}
          </div>
          <span className="ph-txmeter-value">{r.bar?.value ?? '—'}</span>
          {r.key === 'comp' && compControls}
          {/* ⊘ ONLY ONCE WE COULD HAVE KNOWN. The rig has keyed and this meter stayed
              silent through it, so it is one the radio does not report — as against the
              blank rows before the first over, which say nothing about the radio and are
              answered by the hint below instead. */}
          {overJudged.current && !seen.current.has(r.key) && (
            <span className="ph-unavail" role="note" title={t('phone.unavail.meter', { meter: r.label })}>
              <span aria-hidden="true">⊘</span> {t('phone.unavail.mark')}
            </span>
          )}
        </div>
        )
      })}
      {tempRows}
      {/* Before the first over nothing is known about any meter — these fields are populated
          only while transmitting — so the panel says WHEN it reads rather than letting four
          dashes read as four missing features. It goes once a reading has arrived, because
          from then on each row answers for itself. The Operate cell shows it alone (above),
          never beside its slots: a line that came and went would move the cell. */}
      {pin && !inline && !everRead.current && (
        <span className="ph-txmeters-hint">{t('meters.tx.idle', { when: TX_METERS_WHEN })}</span>
      )}
    </div>
  )
}

const isNeedle = (k: MeterKey): k is NeedleKey => (NEEDLE_KEYS as readonly string[]).includes(k)
