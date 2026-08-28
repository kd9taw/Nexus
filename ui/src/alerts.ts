// Decode alerts: a WebAudio beep + a visual toast, fired from the live decode
// feed and gated by user settings.
//
// We do NOT alert on every decode — that's noise. Experienced operators want
// loud, aggressive alerts ONLY for the things worth chasing:
// - alertMyCall → a decode directed at my callsign (someone calling me)
// - alertNew    → a NEW DXCC entity (a "new one" — aggressive) or a new grid
// - alertCq     → a plain CQ (off by default; opt-in, since CQs are constant)
//
// Each unique decode (from + message + freq) alerts at most once.

//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every word an alert
// says comes from the catalog. What does NOT: the callsign, the DXCC entity name, the grid
// square and the watch filter's own label — they are interpolated as they arrive. The two
// optional clauses (` — <entity>` and the grid after "grid") are assembled here as WHOLE
// clauses and handed in, so a toast with neither never carries a dangling separator.

import type { DecodeRow, Settings } from './types'
import { pushToast } from './toast'
import { announce } from './announce'
import { t } from './i18n'
import { matchWatchlist, watchLabel, type WatchFilter } from './watchlist'

// ⭐ TWO SETS, DELIBERATELY. The "once ever" dedups (a new DXCC, a new grid, a
// watch-list hit) must NEVER be evicted by churn from the repeating kinds — that
// is precisely how an ATNO started re-alerting every cycle. See `alertedRepeat`.
const alertedOnce = new Set<string>()
// The kinds that legitimately repeat as an exchange advances (mycall / cq). Bounded,
// because a busy band produces these continuously.
const alertedRepeat = new Set<string>()
// Every decode key ever seen (not just alert-worthy) — drives the batch
// freshness check for the decode tick + screen-reader batch summaries.
const seenDecodes = new Set<string>()

let audioCtx: AudioContext | null = null

/** Lazily create / resume the shared AudioContext (needs a user gesture first). */
function ensureCtx(): AudioContext | null {
  try {
    if (!audioCtx) {
      const Ctor =
        window.AudioContext ||
        (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext
      if (!Ctor) return null
      audioCtx = new Ctor()
    }
    if (audioCtx.state === 'suspended') void audioCtx.resume()
    return audioCtx
  } catch {
    return null
  }
}

/** Short two-tone beep. Frequencies differ by alert kind so they're distinguishable. */
function beep(freq: number): void {
  const ctx = ensureCtx()
  if (!ctx) return
  const now = ctx.currentTime
  const osc = ctx.createOscillator()
  const gain = ctx.createGain()
  osc.type = 'sine'
  osc.frequency.value = freq
  gain.gain.setValueAtTime(0.0001, now)
  gain.gain.exponentialRampToValueAtTime(0.18, now + 0.01)
  gain.gain.exponentialRampToValueAtTime(0.0001, now + 0.22)
  osc.connect(gain)
  gain.connect(ctx.destination)
  osc.start(now)
  osc.stop(now + 0.24)
}

function decodeKey(d: DecodeRow): string {
  return `${d.from ?? '?'}|${d.message}|${Math.round(d.freqHz)}`
}

/** Identity of a decode for ALERT dedup — deliberately WITHOUT the frequency.
 *
 * ⚠️ `decodeKey` includes `Math.round(d.freqHz)`, and a station's MEASURED audio
 * offset drifts a few Hz between transmissions (rig drift plus the decoder's own
 * estimate). Rounded to 1 Hz that is a different integer nearly every cycle, so the
 * same station sending the same message minted a NEW key every time and alerted
 * again — the operator's "it alerted over and over ... on each cycle".
 *
 * Frequency is a MEASUREMENT, not identity. Who said what is the identity. */
function alertIdentity(d: DecodeRow): string {
  return `${d.from ?? '?'}|${d.message}`
}

type AlertKind = 'mycall' | 'newdxcc' | 'newgrid' | 'cq'

const BEEP_HZ: Record<AlertKind, number> = { mycall: 880, newdxcc: 520, newgrid: 740, cq: 620 }

/** Two quick tones — the attention-grabbing alert for a new DXCC ("new one")
 * and the M5+/X-class solar-flare heads-up (flareAlert.ts). */
export function doubleBeep(freq: number): void {
  beep(freq)
  window.setTimeout(() => beep(freq * 1.5), 130)
}

/** Soft, short tick — much quieter than an alert beep. The decode-batch pulse
 * (Settings, off by default): the band's rhythm for eyes-free operating. */
function tickBeep(): void {
  const ctx = ensureCtx()
  if (!ctx) return
  const now = ctx.currentTime
  const osc = ctx.createOscillator()
  const gain = ctx.createGain()
  osc.type = 'sine'
  osc.frequency.value = 340
  gain.gain.setValueAtTime(0.0001, now)
  gain.gain.exponentialRampToValueAtTime(0.05, now + 0.008)
  gain.gain.exponentialRampToValueAtTime(0.0001, now + 0.07)
  osc.connect(gain)
  gain.connect(ctx.destination)
  osc.start(now)
  osc.stop(now + 0.08)
}

/** TX key/unkey earcon (Settings `soundTxState`, off by default): a rising
 * two-tone on key-down, a falling one on unkey — the eyes-free TX pill. */
export function txEarcon(on: boolean): void {
  if (on) {
    beep(660)
    window.setTimeout(() => beep(880), 110)
  } else {
    beep(880)
    window.setTimeout(() => beep(560), 110)
  }
}

/** Pass-start (AOS) earcon for the ARMED satellite track (satPassAlert.ts): a
 * rising three-tone. Three notes, not two, and frequencies no other alert
 * uses (BEEP_HZ, the 990 Hz ⏰ alarm, the 660/880 TX pair) — the operator's
 * hands are on a rotor and the sound alone must say which event this is. */
export function aosEarcon(): void {
  beep(523)
  window.setTimeout(() => beep(784), 120)
  window.setTimeout(() => beep(1046), 240)
}

/** Pass-end (LOS) earcon — the falling mirror of [`aosEarcon`], so rise and
 * set are tellable apart blind. */
export function losEarcon(): void {
  beep(1046)
  window.setTimeout(() => beep(784), 120)
  window.setTimeout(() => beep(523), 240)
}

/** What the operator is currently doing in the FT8/FT4 sequencer — lets the
 * alerts stay quiet while they're already engaged (the chatty-popup fix). */
export interface QsoContext {
  /** Sequencer state ("Listening", "CallingCq", "AwaitReport", … "Done"), or
   * null when the FT8 area isn't active at all. */
  state: string | null
  /** The station currently being worked, if any. */
  dxcall: string | null
}

/**
 * Engaged = the sequencer is MID-QSO. Calling CQ is not mid-QSO, and the difference is the
 * whole point of this function.
 *
 * ⚠️ `CallingCq` COUNTED AS ENGAGED UNTIL 2026-08-22, which silenced the one alert an operator
 * calling CQ is actually waiting for: `mycall` is gated on `!engaged`, so a station answering
 * YOU raised no beep at all. Reported by a new operator whose other alerts all worked ("the
 * other alerts like new DXCC work fine") — the tell that audio was fine and this single path
 * was gated off. WSJT-X alerts here, and an operator running CQ with Auto OFF has nothing else
 * to tell them somebody replied.
 *
 * The guard is still right for a QSO in progress — that was the chatty-popup fix, and `mycall`
 * dedups per DECODE, so without it every message of an exchange would beep. Nothing is lost by
 * exempting CallingCq: the moment somebody answers, the sequencer leaves CallingCq for
 * AwaitReport (or the Field Day equivalent) and the rest of the exchange is suppressed exactly
 * as before. So the alert fires on the ANSWER and then goes quiet.
 *
 * The engine draws the same line: `qso.rs` scores `State::Listening | State::CallingCq => 0`
 * exchanges completed, grouping the two precisely as here.
 */
function engagedInQso(ctx?: QsoContext): boolean {
  return (
    !!ctx?.state &&
    ctx.state !== 'Listening' &&
    ctx.state !== 'CallingCq' &&
    ctx.state !== 'Done'
  )
}

/** Per-alert band scope: which bands an alert type may fire on. 'vhf' = 6 m and up
 * (≥ 50 MHz — where grid chasing actually lives, VUCC/FFMA), 'hf' = below 6 m. */
export type AlertBandScope = 'off' | 'hf' | 'vhf' | 'all'

/**
 * Does an alert scope admit the current dial? Unknown dial (no CAT) is PERMISSIVE —
 * any non-off scope passes, because silently dropping alerts on a band we can't
 * verify would hide real ones; the operator can still pick Off for hard silence.
 */
export function bandScopeOk(scope: string | undefined, dialMhz: number | undefined, fallback: AlertBandScope): boolean {
  const s = (scope ?? fallback) as AlertBandScope
  if (s === 'off') return false
  if (s === 'all') return true
  if (dialMhz == null || !(dialMhz > 0)) return true // band unknown → permissive
  return s === 'vhf' ? dialMhz >= 50 : dialMhz < 50
}

/**
 * Inspect the latest decode rows and fire alerts ONLY for new/needed things:
 * someone calling me, a new DXCC entity (aggressive), a new grid, or — if the
 * operator opted in — a plain CQ. Each unique decode alerts at most once.
 *
 * Quiet while operating: no popups about the station currently being worked
 * (every reply from a QSO partner is "directed to me" — that toasted every
 * over), and no "calling you" popups while mid-QSO/CQ-run (the sequencer is
 * already answering; the cockpit shows it). Monitoring stays fully alerted.
 *
 * Band scoping: new-DXCC / new-grid / rare-grid each carry a band scope setting
 * (all decodes are on the CURRENT band, so the scope is just "should this alert
 * on the band I'm on"). Plain grids default to VHF+ only — on HF nearly every
 * decode is an unworked grid and the alert is noise; rare 💎 grids stay special
 * everywhere by default.
 */
export function processDecodes(
  decodes: DecodeRow[],
  settings: Settings,
  // Click-to-work: when provided, each alert toast gets a button that works the station
  // the alert is about (identical to double-clicking its decode row). Optional so the
  // alert path stays usable without a handler.
  onWork?: (d: DecodeRow) => void,
  qso?: QsoContext,
  // The operator's user-defined watch list (localStorage). A match is the loudest tier —
  // they explicitly asked to be told about it — and takes precedence over the generic logic.
  watchlist?: WatchFilter[],
  // Current dial (MHz) for the per-alert band scopes; absent = band unknown (permissive).
  dialMhz?: number,
): void {
  const engaged = engagedInQso(qso)
  const partner = qso?.dxcall?.toUpperCase() ?? null

  // ── Batch freshness (eyes-free channel): which rows have never been seen at
  // all (any kind, not just alert-worthy)? Drives the soft decode tick and the
  // screen-reader batch summary — both rate-limited by construction to one
  // event per decode cycle. Seen-set capped so a long session can't grow it
  // unbounded (a reset only risks one duplicate tick).
  if (seenDecodes.size > 5000) seenDecodes.clear()
  const fresh: DecodeRow[] = []
  for (const d of decodes) {
    if (d.mine) continue
    const k = decodeKey(d)
    if (!seenDecodes.has(k)) {
      seenDecodes.add(k)
      fresh.push(d)
    }
  }
  if (fresh.length > 0) {
    if (settings.soundDecodeTick) tickBeep()
    // Verbosity "all": a compact spoken batch summary (count + up to three CQ
    // callers) — the QLog filter doctrine, never the raw firehose. "needed"
    // adds nothing here: the alert toasts below are already announced.
    if (settings.announceVerbosity === 'all') {
      const cqs = fresh.filter((d) => d.isCq && d.from).slice(0, 3)
      const cqPart = cqs.length
        ? t('alerts.batch.cq', {
            calls: cqs.map((d) => `${d.from}${d.country ? ` (${d.country})` : ''}`).join(', '),
          })
        : ''
      announce(t('alerts.batch.decodes', { count: fresh.length }) + cqPart)
    }
  }

  // Per-type band scopes, resolved once per batch. `alertNew` stays the master gate
  // (a pre-scope settings file with it off keeps everything off, no migration).
  const dxccOk = bandScopeOk(settings.alertDxccBands, dialMhz, 'all')
  const gridOk = bandScopeOk(settings.alertGridBands, dialMhz, 'vhf')
  // 'vhf' fallback matches the backend default — an absent setting must not resurrect the
  // HF rare-grid chatter the default was changed to remove (operator, 2026-08-15).
  const rareOk = bandScopeOk(settings.alertRareGridBands, dialMhz, 'vhf')
  for (const d of decodes) {
    const call = d.from

    // ⭐ THE MYCALL KIND IS DECIDED BEFORE THE PARTNER SKIP, and it has to be (#167). The
    // sequencer sets `qso.dxcall` in the SAME ingest that produces the decode, so the very
    // decode that ANSWERS your CQ arrives on a snapshot where that station is already the
    // partner. Skipping it here dropped the one alert a CQ run exists for, and the operator
    // heard nothing until their post-RR73 "73" — by then the QSO was over and dxcall had moved
    // on. WSJT-X applies its MyCall highlight to every decode carrying your callsign with no
    // exclusion for the station being worked (widgets/displaytext.cpp:490); dxCall drives a
    // separate highlight. The anti-chatter job the skip was doing is the `engaged` gate's and
    // the per-decode dedup's, not this line's: mid-exchange states never reach `callingMe`.
    const callingMe = !!(settings.alertMyCall && d.directedToMe && !engaged)

    // Already working this station → nothing else about them is news (skipped WITHOUT
    // consuming the dedup key, so a later fresh event can still alert). A partner row that
    // IS someone calling us falls through to the kind ladder below, where `mycall` wins
    // first — so a partner can still only ever produce that one alert.
    const isPartner = !!partner && call?.toUpperCase() === partner
    if (isPartner && !callingMe) continue

    // User watch list FIRST: an explicitly-watched call/prefix/entity/grid is the loudest tier
    // and pre-empts the generic new/CQ logic (deduped once per filter+call so it doesn't spam).
    // Deliberately ABOVE the band-scope gates: a grid the operator typed in must fire on HF
    // even though unworked-grid chatter is HF-quiet by default.
    // …but NOT for the station being worked: the watch tier is louder than "calling you" and
    // pre-empts it, and a partner reaching this line is here on the mycall exemption alone.
    if (!isPartner && watchlist && watchlist.length) {
      const hit = matchWatchlist(d, watchlist)
      if (hit) {
        const wkey = `watch:${hit.id}:${call ?? '?'}`
        if (!alertedOnce.has(wkey)) {
          alertedOnce.add(wkey)
          const where = d.country ? t('alerts.where', { entity: d.country }) : ''
          doubleBeep(BEEP_HZ.newdxcc)
          pushToast(
            t('alerts.watch', {
              what: watchLabel(hit),
              call: call ?? t('alerts.station'),
              where,
            }),
            'success',
            15000,
            {
              prominent: true,
              action: onWork && d.from ? () => onWork(d) : undefined,
              actionLabel: t('alerts.action.work'),
            },
          )
        }
        continue // don't ALSO fire a generic new/CQ alert for the same decode
      }
    }

    // Decide whether this row should alert (highest priority first). New DXCC
    // and new grid are gated by alertNew (master) + their per-type band scope;
    // a new DXCC is the loud "new one". A rare grid alerts when EITHER grid
    // scope admits it (the rare scope keeps the gems when plain grids are off;
    // the plain scope demotes a gem to a quiet toast when only IT is open).
    const isRareGrid = d.gridRarity === 'rare' || d.gridRarity === 'ultraRare'
    let kind: AlertKind | null = null
    if (callingMe) kind = 'mycall'
    else if (settings.alertNew && d.newDxcc && dxccOk) kind = 'newdxcc'
    else if (settings.alertNew && d.newGrid && (isRareGrid ? rareOk || gridOk : gridOk))
      kind = 'newgrid'
    else if (settings.alertCq && d.isCq) kind = 'cq'
    if (!kind) continue

    // Rarity escalation: a NEEDED rare/water-only grid is a hunting moment and
    // earns the loudness plain new-grids gave up (the "too chatty" fix) — but
    // only when the RARE scope admits this band (else it rides the quiet tier).
    const rareGrid = kind === 'newgrid' && isRareGrid && rareOk

    // Dedup scope per kind: a new DXCC alerts once per ENTITY (not again as the
    // same station's message evolves through the QSO), a new grid once per
    // station — except a RARE grid, which dedups once per GRID (a second rover
    // in the same water grid isn't a second event); mycall/cq dedup on the
    // exact decode (they may legitimately repeat as the exchange advances).
    const onceEver = kind === 'newdxcc' || kind === 'newgrid'
    const key =
      kind === 'newdxcc'
        ? `dxcc:${d.country ?? d.from ?? '?'}`
        : kind === 'newgrid'
          ? rareGrid && d.grid
            ? `rgrid:${d.grid.toUpperCase()}`
            : `grid:${d.from ?? '?'}`
          : `${kind}:${alertIdentity(d)}`
    const seen = onceEver ? alertedOnce : alertedRepeat
    if (seen.has(key)) continue
    seen.add(key)

    const who = call ?? t('alerts.station')
    const where = d.country ? t('alerts.where', { entity: d.country }) : ''
    // Every decode alert is "here's a station worth working" — wire the toast's action to
    // work it (same as double-clicking the decode). Only when we know who (from is set).
    const workAction = onWork && d.from ? () => onWork(d) : undefined
    if (kind === 'newdxcc') {
      // Aggressive: double tone + a prominent, long-lived toast.
      doubleBeep(BEEP_HZ.newdxcc)
      pushToast(t('alerts.newDxcc', { call: who, where }), 'success', 15000, {
        prominent: true,
        action: workAction,
        actionLabel: t('alerts.action.work'),
      })
      continue
    }
    // A new grid is common enough to be noise at full volume (operator report:
    // "too chatty") — quiet info toast, no beep. The future grid-RARITY tiers
    // are what earn loudness back for the rare ones.
    if (kind !== 'newgrid') beep(BEEP_HZ[kind])
    // Someone calling YOU is the most time-critical alert — make it loud and let
    // it linger (the beep was firing but the toast vanished before you could
    // find it). An opt-in CQ stays a quieter, shorter info toast.
    if (kind === 'mycall') {
      pushToast(t('alerts.myCall', { call: who }), 'success', 20000, {
        prominent: true,
        action: workAction,
        actionLabel: t('alerts.action.answer'),
      })
    } else if (kind === 'newgrid') {
      if (rareGrid) {
        // The gem earns its keep: rare = islet/sliver, ultra = water-only
        // (rover/maritime/DXpedition) — aggressive like a new one.
        const tier = d.gridRarity === 'ultraRare' ? t('alerts.tier.ultraRare') : t('alerts.tier.rare')
        doubleBeep(BEEP_HZ.newgrid)
        pushToast(
          t('alerts.rareGrid', { tier, grid: d.grid ? ` ${d.grid}` : '', call: who, where }),
          'success',
          15000,
          {
            prominent: true,
            action: workAction,
            actionLabel: t('alerts.action.work'),
          },
        )
      } else {
        pushToast(t('alerts.newGrid', { call: who, where }), 'info', 6000, {
          action: workAction,
          actionLabel: t('alerts.action.work'),
        })
      }
    } else {
      pushToast(t('alerts.cq', { call: who, where }), 'info', 6000, {
        action: workAction,
        actionLabel: t('alerts.action.answer'),
      })
    }
  }

  // Keep the REPEATING dedups bounded over a long session (Field Day / contests)
  // WITHOUT a wholesale clear — that would re-alert every familiar station. Evict
  // the oldest (Set preserves insertion order) so recent dedups survive.
  //
  // ⭐ ONLY this set is evicted. It used to be one shared set, and that is what made
  // an ATNO re-alert: every CQ decode minted a unique key (frequency was in the key,
  // and it drifts), so on a busy band the 2000-entry cap blew in a minute or two and
  // FIFO eviction dropped the OLDEST entries first — which included the `dxcc:` entry
  // written when the new one was first heard. The durable dedups now live in
  // `alertedOnce`, where no amount of CQ traffic can reach them.
  const CAP = 2000
  if (alertedRepeat.size > CAP) {
    const drop = Math.floor(CAP * 0.2)
    let i = 0
    for (const k of alertedRepeat) {
      alertedRepeat.delete(k)
      if (++i >= drop) break
    }
  }
}
