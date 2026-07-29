import { MapView } from './MapView'
import { AprsStationCard } from './AprsStationCard'
import type { NeedTag, Station } from '../types'
import type { Theme } from '../useTheme'
import { useEffect, useMemo, useRef, useState } from 'react'

import {
  aprsArm,
  aprsAutoArm,
  aprsSendBeacon,
  aprsSendMessage,
  getAprsHeard,
  getAprsHealth,
  getAprsIsStatus,
  getAprsStations,
  setSettings,
  getSettings,
  type AprsHealth,
  type AprsHeard,
  type AprsIsStatus,
  type AprsSource,
  type AprsStationsView,
} from '../api'
import type { Settings } from '../types'
import { ageFade, CATEGORY_VAR, GLYPH_PATHS, resolveSymbol, symbolCategory } from '../aprsSymbols'
import { bearingDeg, gridToLatLon, haversineKm, type LatLon } from '../grid'

const COMPASS = ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW']
function compass(deg: number): string {
  return COMPASS[Math.round(deg / 45) % 8]
}

/** The regional 2 m FM APRS frequencies (MHz) — all AFSK-1200, what this decoder handles. */
const APRS_FREQS: [number, string][] = [
  [144.39, 'N. America'],
  [144.8, 'Europe / Africa'],
  [145.175, 'Australia'],
  [144.575, 'New Zealand'],
  [144.66, 'Japan'],
  [144.93, 'Argentina'],
  [145.57, 'Brazil'],
]

/** Common APRS symbols (primary table `/`): [code, label]. */
const SYMBOLS: [string, string][] = [
  ['>', 'Car'],
  ['-', 'House'],
  ['[', 'Person'],
  ['b', 'Bicycle'],
  ['j', 'Jeep'],
  ['<', 'Motorcycle'],
  ['k', 'Truck'],
  ['.', 'Dot'],
]

function ageLabel(atUnix: number, nowSec: number): string {
  const s = Math.max(0, nowSec - atUnix)
  if (s < 60) return `${s}s`
  if (s < 3600) return `${Math.floor(s / 60)}m`
  return `${Math.floor(s / 3600)}h`
}

// ─── CALIBRATION SEAM ────────────────────────────────────────────────────────────────────────
// These two constants draw the silent-vs-hiss boundary, and they are currently REASONED, not
// MEASURED. A squelched codec that emits exact digital zeros makes any positive threshold work;
// one that emits low-level dither does not, and that is the case we have no numbers for.
//
// To calibrate: capture the peak readout (now shown in dBFS in the chip) on the operator's rig in
// two resting states — (B) squelch closed, no signal, and (C) squelch open on a dead-quiet
// channel. SILENT_PEAK belongs between those two, with margin. If they turn out to overlap, the
// level alone cannot separate them and the discriminator has to change (tracking a noise floor
// while samples flow is the obvious next move) — deliberately NOT built ahead of that data,
// because guessing an adaptation rule now just risks a second wrong heuristic on the operator's
// screen. Nothing outside this block encodes the boundary.

/** Peak below which the input counts as carrying silence rather than signal. -60 dBFS.
 * Digital silence from a squelched codec is exactly 0; open-squelch hiss measures far above. */
const SILENT_PEAK = 0.001

/** Drains the decode thread must have reported before an absent arrival stamp is called a fault.
 * ~1 s at the thread's 100 ms poll — long enough that arming never flashes a capture alarm. */
const MIN_DRAINS_BEFORE_CAPTURE_FAULT = 5
// ─────────────────────────────────────────────────────────────────────────────────────────────

/** The peak level as dBFS, so "what is the decoder hearing" is a NUMBER on screen rather than an
 * inference from which message appeared. Exact zero has no logarithm — say so plainly. */
function levelLabel(peak: number): string {
  // Say WHICH WINDOW the number measures. It is the peak sample of the most recent ~100 ms drain
  // the decoder consumed — an instantaneous reading, not a decaying meter — so a number that
  // looks alarmingly low may just be the gap between two packets.
  if (!(peak > 0)) return 'input over the most recent 0.1 s: digital silence (exactly zero)'
  return `input peak over the most recent 0.1 s: ${Math.round(20 * Math.log10(peak))} dBFS`
}

/** How long the tap may go without any samples ARRIVING before the capture counts as dead.
 *
 * The decode thread polls every 100 ms but the radio loop that feeds it can take far longer per
 * iteration (blocking CAT, up to 2500 ms on slow serial), so gaps of a second or two are normal.
 * Judging on the instant would cry wolf constantly on a healthy station. */
const AUDIO_STALE_SEC = 5

/** How recently a frame candidate must have arrived for "packets are failing" to be a
 * PRESENT-TENSE claim.
 *
 * 60 s. APRS bursts are sporadic — a station beacons every 1–10 minutes, and even a busy channel
 * goes quiet for stretches — so a shorter window would flicker between "failing" and "quiet" in
 * the ordinary gap between packets. Longer, and a couple of candidates from several minutes ago
 * keep asserting something about right now, which is the bug this fixes. */
const FRAME_RECENT_SEC = 60

// ─── HEALTHY BURST BAND ──────────────────────────────────────────────────────────────────────
// What a packet burst SHOULD measure at the app's input, and what the operator can watch between
// packets. These are headroom targets, NOT decodability thresholds — see the note below.
//
// ⚠️ MEASURED: absolute level does not decide whether a frame decodes. The Bell-202 discriminator
// compares mark ENERGY against space energy, so level cancels — decode holds to -140 dBFS on a
// clean signal, down to ~9 dB SNR over the operator's measured -99 dBFS dither floor, and through
// 30 dB of hard clipping (FSK survives limiting). So a burst outside this band costs MARGIN, and
// is worth fixing for that reason; it is never the explanation for a failed checksum.
const HEALTHY_BURST_MIN = 0.0316 // -30 dBFS
const HEALTHY_BURST_MAX = 0.5 // -6 dBFS
/** At or above this a burst is hitting the rails (-1 dBFS). */
const CLIPPING_PEAK = 0.891
/** What open-squelch hiss should read — the thing the operator can watch between packets. */
const HISS_TARGET = '-30 to -25 dBFS'
// ─────────────────────────────────────────────────────────────────────────────────────────────

/** dBFS for a peak, as a bare number (no label). */
function dbfs(peak: number): string {
  return peak > 0 ? `${Math.round(20 * Math.log10(peak))} dBFS` : 'silence'
}

/** Headroom advice for a burst, or '' when it sits in the healthy band.
 *
 * Deliberately framed as headroom. The operator asked why frames fail; the honest answer is that
 * level is not it, so this says what level costs rather than what it prevents. */
function burstLevelAdvice(health: AprsHealth): string {
  const peak = Math.max(health.framePeak, health.maxFramePeak)
  if (peak <= 0) return ''
  if (health.frameClippedSamples > 0 || peak >= CLIPPING_PEAK) {
    return (
      ` The burst is CLIPPING (peaked ${dbfs(peak)}, ${health.frameClippedSamples} sample(s) at the ` +
      'rails) — lower the rig\'s USB AF output level, or the Windows input level for that device. ' +
      'Packet survives a lot of clipping, so this costs headroom rather than decodes, but there is ' +
      'no reason to run into the rails.'
    )
  }
  if (peak < HEALTHY_BURST_MIN) {
    return (
      ` The burst peaked ${dbfs(peak)}, well below the healthy ${dbfs(HEALTHY_BURST_MIN)} to ` +
      `${dbfs(HEALTHY_BURST_MAX)} band — raise the rig's USB AF output level (IC-9700: SET > ` +
      'Connectors > USB AF Output Level) or the Windows input level for that device. That buys ' +
      'margin against noise; it is not by itself why a checksum fails.'
    )
  }
  return ''
}

/** How far the dial may sit from the APRS channel before it counts as a different frequency.
 * 5 kHz — wider than any rounding or CAT read-back jitter, far narrower than a channel step. */
const DIAL_TOLERANCE_MHZ = 0.005

/** The rig state the chip judges against. Only the facts that decide whether APRS can possibly
 * hear anything: where the dial is, whether the rig is in FM, and — above both — whether the
 * radio can reach the APRS channel at all. */
export interface AprsRadio {
  dialMhz: number
  sideband: string
  /** The radio's RECEIVE coverage as `[loMhz, hiMhz]` pairs, from Hamlib's capability table.
   * **EMPTY = unknown, which means ALLOW.** Never treat an empty list as "covers nothing": the
   * probe legitimately comes up empty with no CAT, before the first poll, or on a rigctld whose
   * `\dump_state` we could not parse, and refusing a QSY on a guess is worse than the refused
   * command the check exists to avoid. */
  rxRangesMhz?: [number, number][]
}

/** Whether `mhz` is inside the radio's known receive coverage. `null` = unknown → allow. */
export function radioCoversMhz(
  ranges: [number, number][] | undefined,
  mhz: number,
): boolean | null {
  if (!ranges || ranges.length === 0) return null
  return ranges.some(([lo, hi]) => mhz >= lo && mhz <= hi)
}

export type AprsDecodeState =
  | 'off'
  /** The radio physically cannot receive the APRS channel — an HF-only rig and 2 m. Above every
   * other verdict because it is the only one no amount of tuning, squelch or audio routing can
   * fix, and because offering a [Tune] button here would be offering a control that cannot work. */
  | 'norf'
  /** CAT says the radio is not where APRS lives. Dispositive: no audio-level reading can
   * substitute for the fact that a different frequency is being received. */
  | 'wrongfreq'
  /** Nothing arriving from the capture device at all — a real fault. */
  | 'nocapture'
  /** Arriving, but at zero level — squelch closed. The normal resting state of an FM channel. */
  | 'silent'
  | 'listening'
  | 'unreadable'
  | 'decoding'

/**
 * A station's APRS symbol, drawn from the SAME path data the map canvas uses so the list and the
 * map can never show different icons for the same station.
 *
 * Purely decorative next to the callsign it sits beside, so it is hidden from assistive tech and
 * its meaning is carried by the `title` instead — a screen reader hearing "Car" between the age
 * and the callsign learns nothing the row does not already say.
 */
export function AprsSymbolIcon({ table, code }: { table: string; code: string }) {
  const sym = resolveSymbol(table, code)
  return (
    <span
      className={`aprs-sym${sym.known ? '' : ' aprs-sym-unknown'}`}
      title={sym.label}
      // Same category colour the map paints, from the same variable — one palette, two renderers.
      style={{ color: `var(${CATEGORY_VAR[symbolCategory(sym.glyph)]})` }}
    >
      <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" focusable="false">
        <path d={GLYPH_PATHS[sym.glyph]} fill="currentColor" />
      </svg>
      {sym.overlay && <span className="aprs-sym-overlay">{sym.overlay}</span>}
    </span>
  )
}

/** A roster with nothing in it, and the backend's own default thresholds. */
const EMPTY_ROSTER: AprsStationsView = { stations: [], ttlMin: 60, fadeAfterMin: 20 }

/**
 * ⭐ THE RENDER-SIDE HALF OF THE FLASHING BUG. Keep the previous array when a poll brought back the
 * same data, so React sees the same reference and nothing downstream re-runs.
 *
 * The poll fires every two seconds and used to `setHeard(h)` unconditionally. Tauri hands back a
 * FRESH array every time, so the identity changed on every tick even when not a single packet had
 * arrived. That invalidated the memo feeding MapView's `aprs` prop, which re-ran the draw effect,
 * which reassigns `canvas.width` — and assigning canvas width RESETS THE BITMAP. So the entire map,
 * relief and coastlines included, was torn down and repainted every two seconds whether or not
 * anything had changed. That is a visible flash on its own, independent of the store churn, and it
 * is why stations appeared to blink even while continuously present.
 *
 * Compares what actually drives the render rather than deep-equalling everything.
 */
function samePackets(a: AprsHeard[], b: AprsHeard[]): boolean {
  if (a === b) return true
  if (a.length !== b.length) return false
  for (let i = 0; i < a.length; i++) {
    if (a[i].source !== b[i].source || a[i].atUnix !== b[i].atUnix || a[i].text !== b[i].text) {
      return false
    }
  }
  return true
}

/** As `samePackets`, for the station roster: identity, when it was last heard, and where it is. */
function sameRoster(a: AprsStationsView, b: AprsStationsView): boolean {
  if (a === b) return true
  if (a.ttlMin !== b.ttlMin || a.fadeAfterMin !== b.fadeAfterMin) return false
  if (a.stations.length !== b.stations.length) return false
  for (let i = 0; i < a.stations.length; i++) {
    const x = a.stations[i]
    const y = b.stations[i]
    if (
      x.call !== y.call ||
      x.lastHeardUnix !== y.lastHeardUnix ||
      x.lat !== y.lat ||
      x.lon !== y.lon ||
      x.sourceKind !== y.sourceKind ||
      x.symbolCode !== y.symbolCode ||
      x.symbolTable !== y.symbolTable
    ) {
      return false
    }
  }
  return true
}

/** Short tag shown in the station list's Via column. */
export const SOURCE_LABEL: Record<AprsSource, string> = {
  rf: 'RF',
  inet: 'net',
  both: 'RF+net',
}

/** Why the tag matters — an RF sighting is evidence about YOUR station, an internet one is not. */
export const SOURCE_TITLE: Record<AprsSource, string> = {
  rf: 'Your receiver decoded this station off the air',
  inet: 'Reported by APRS-IS — your receiver has not heard this station',
  both: 'Heard off the air by your receiver AND reported by APRS-IS',
}

/** How long the APRS-IS feed may go quiet before we say so. Far longer than the RF equivalent:
 * a tight range filter on a rural grid legitimately delivers a packet every few minutes. */
const INET_QUIET_SEC = 300

export type AprsInetState = 'off' | 'connecting' | 'quiet' | 'live'

/**
 * Turn APRS-IS feed status into what the operator should be told.
 *
 * Exported and pure so the wording is testable without a socket — the same treatment the decode
 * chip gets, and for the same reason: these three states looked identical as an empty list.
 */
export function aprsInetStatus(
  st: AprsIsStatus | null,
  nowSec: number,
): { state: AprsInetState; label: string; detail: string } {
  if (!st || !st.enabled) {
    return { state: 'off', label: 'Internet off', detail: 'The APRS-IS feed is switched off.' }
  }
  const gate = st.uplinkEnabled
    ? ` iGate on: ${st.uploaded} contributed${st.gateRejected ? `, ${st.gateRejected} held back${st.lastReject ? ` (last: ${st.lastReject})` : ''}` : ''}.`
    : ''
  if (!st.connected) {
    return {
      state: 'connecting',
      label: 'Internet connecting',
      detail: `Not connected to APRS-IS yet — retrying with backoff.${gate}`,
    }
  }
  // Connected but nothing arriving: a real state, and usually a filter that is too tight rather
  // than a fault. Say which, rather than leaving an empty list to be read as broken.
  const quiet = st.lastPacketUnix == null || nowSec - st.lastPacketUnix > INET_QUIET_SEC
  if (quiet) {
    return {
      state: 'quiet',
      label: 'Internet quiet',
      detail: `Connected, but no packets ${st.packets > 0 ? 'recently' : 'yet'} — nothing matches your filter. Widen the radius or add watched calls.${gate}`,
    }
  }
  return {
    state: 'live',
    label: `Internet ${st.packets}`,
    detail: `Connected${st.verified ? ' and verified' : ' read-only'} — ${st.packets} packets received.${gate}`,
  }
}


/**
 * Turn decoder health into what the operator should be told.
 *
 * THE BUG THIS EXISTS FOR: an empty APRS screen looked the same whether the app was listening to
 * the wrong sound card, hearing a channel whose every frame failed the checksum, or sitting on a
 * quiet band. Only successfully-decoded frames ever reached the UI, so "nothing here" was the
 * single answer to three completely different questions — and the first two are faults the
 * operator can fix in seconds once told.
 */
export function aprsDecodeStatus(
  health: AprsHealth | null,
  nowSec: number,
  radio?: AprsRadio | null,
  wantDialMhz?: number,
): { state: AprsDecodeState; label: string; detail: string } {
  // ⭐⭐ ABOVE EVERYTHING, including the decoder's own state: can this radio even receive the APRS
  // channel? The FTdx10 report — an HF/6 m radio, and opening APRS commanded it to 144.390, which
  // it refused. This is a fact about the hardware, so it outranks "Monitor off": arming the decoder
  // cannot help, and neither can tuning, squelch, or audio routing. Saying so is also the only way
  // the view stops looking broken on a station that simply has no VHF radio.
  //
  // Coverage unknown (no CAT, not yet probed, unparseable caps) deliberately falls through — the
  // rest of the ladder still applies and nothing is claimed that we cannot back up.
  if (radio && wantDialMhz != null && radioCoversMhz(radio.rxRangesMhz, wantDialMhz) === false) {
    return {
      state: 'norf',
      label: 'No 2 m radio',
      detail:
        `This radio doesn't cover ${wantDialMhz.toFixed(3)} MHz, so it can't receive RF APRS. ` +
        'RF APRS needs a VHF radio. The internet feed works without one — turn it on to see ' +
        'APRS traffic reported by other stations.',
    }
  }
  if (!health || health.arm === 'off') {
    return {
      state: 'off',
      label: 'Monitor off',
      detail: 'The APRS decoder is not running. Arm Monitor to decode the RX audio.',
    }
  }
  // ⭐ TOP OF THE LADDER: what the radio is actually receiving.
  //
  // A second on-air report had FT8 decoding perfectly on 2 m while this chip insisted there was
  // no audio. The capture path was fine; the dial was simply parked on the FT8 frequency in USB.
  // One receiver, one dial — APRS's channel was never being received. No amount of reasoning
  // about audio levels can reach that conclusion, and every message below it was therefore
  // advice about the wrong problem. CAT knows, so CAT speaks first.
  //
  // Judged against the frequency the operator SELECTED, never a hardcoded 144.390: the APRS
  // channel is regional (144.800 in Europe, 145.175 in Australia…), and telling a correctly
  // tuned European operator they are on the wrong frequency would be its own bug.
  if (radio && wantDialMhz != null) {
    const modeKnown = radio.sideband.trim() !== ''
    const notFm = modeKnown && !/fm/i.test(radio.sideband)
    const offChannel = Math.abs(radio.dialMhz - wantDialMhz) > DIAL_TOLERANCE_MHZ
    if (offChannel || notFm) {
      const want = wantDialMhz.toFixed(3)
      const mode = radio.sideband.toUpperCase()
      // Name the thing that is actually wrong. Restating a frequency the operator is already on
      // reads as noise; "you are in USB and this needs FM" is the answer to the question they
      // are really asking.
      const detail = !offChannel
        ? `The radio is on ${want} but in ${mode} — APRS needs FM. FM packet audio demodulated ` +
          'as SSB is garbled, so nothing will decode however strong the signal is.'
        : notFm
          ? `The radio is on ${radio.dialMhz.toFixed(3)} ${mode} — APRS needs ${want} FM. ` +
            'Nothing on this channel can decode as APRS packet, whatever the audio level says.'
          : `The radio is on ${radio.dialMhz.toFixed(3)} — APRS needs ${want}. Nothing on this ` +
            'channel can decode as APRS packet, whatever the audio level says.'
      return {
        state: 'wrongfreq',
        label: offChannel ? 'Wrong frequency' : 'Wrong mode',
        detail: `${detail} Tune to the APRS channel to start hearing it.`,
      }
    }
  }
  const level = levelLabel(health.audioPeak)
  // NOTHING ARRIVING — the only genuine capture fault. Held apart from a zero LEVEL (below),
  // because the two have opposite fixes. Waits for a few drains so arming cannot flash an alarm
  // before the decode thread has reported anything.
  const noArrivals =
    health.lastAudioUnix == null || nowSec - health.lastAudioUnix > AUDIO_STALE_SEC
  if (noArrivals && health.drains >= MIN_DRAINS_BEFORE_CAPTURE_FAULT) {
    return {
      state: 'nocapture',
      label: 'No input',
      detail:
        'Armed, but no audio samples are arriving at all — the capture device is not delivering ' +
        'anything. Check that Settings → Audio input is the radio (not a microphone or a ' +
        'disconnected device); what you hear on the speaker does not tell you what the app is ' +
        'capturing.',
    }
  }
  // Decodes outrank everything below: once packets are landing, a squelched gap between them is
  // not news, and flicking back to a fault message between bursts is what made the readout
  // untrustworthy on air.
  // A successful decode LATCHES this state, unlike the failure count below, and the difference
  // is principled: a checksummed frame is a durable fact about the setup — it proves the whole
  // chain works — whereas "candidates are failing" is a claim about current conditions. So the
  // fact stays, and the WORDING carries its age rather than an eternal count masquerading as now.
  if (health.framesDecoded > 0) {
    return {
      state: 'decoding',
      label: `${health.framesDecoded} decoded`,
      detail:
        (health.lastDecodeUnix == null
          ? `${health.framesDecoded} packets decoded since arming.`
          : `${health.framesDecoded} packets decoded since arming, last one ${ageLabel(health.lastDecodeUnix, nowSec)} ago.`) +
        ` Live ${level}.`,
    }
  }
  // ⚠️ PRESENT TENSE ONLY. This claim is about what the channel is doing NOW, so it needs a
  // recent candidate to stand on. The counter is cumulative since arming; left ungated it latched
  // forever, and the chip ended up saying "2 packets were heard" beside a live level of -99 dBFS
  // — two facts of completely different ages rendered as one sentence about the present.
  const framesFresh =
    health.lastFrameSeenUnix != null && nowSec - health.lastFrameSeenUnix <= FRAME_RECENT_SEC
  if (health.framesSeen > 0 && framesFresh) {
    return {
      state: 'unreadable',
      label: `${health.framesSeen} failed CRC`,
      detail:
        `${health.framesSeen} bursts heard since arming, last one ` +
        `${ageLabel(health.lastFrameSeenUnix as number, nowSec)} ago — none passed the checksum. ` +
        'Some of that is normal: when the squelch opens partway through a burst the start of the ' +
        'packet is lost, and a part-heard packet can never pass. It is only a fault if nothing ' +
        'ever decodes — in which case check the rig is on 144.390 in FM.' +
        burstLevelAdvice(health) +
        ` Last burst peaked ${dbfs(Math.max(health.framePeak, health.maxFramePeak))}; live ${level}.`,
    }
  }
  // ARRIVING BUT SILENT. Overwhelmingly this is just a closed squelch, which is what an idle FM
  // channel looks like — so it names the squelch first and does not read as a fault.
  if (health.audioPeak < SILENT_PEAK) {
    return {
      state: 'silent',
      label: 'Silent',
      detail:
        'The input is alive and delivering audio, but it is silent — normally that just means ' +
        'the squelch is closed between packets, which is what an idle FM channel looks like. To ' +
        'confirm the routing, open the squelch: hiss should show up here as a level. If it still ' +
        `reads silent with the squelch open, the wrong input device is selected. Live ${level}.`,
    }
  }
  return {
    state: 'listening',
    label: 'Listening',
    detail:
      'Audio is reaching the decoder and no packets have been heard recently — a quiet channel. ' +
      `Live ${level}. With the squelch open, hiss should sit around ${HISS_TARGET}; a packet burst ` +
      `should peak ${dbfs(HEALTHY_BURST_MIN)} to ${dbfs(HEALTHY_BURST_MAX)}.`,
  }
}

/**
 * APRS cockpit — monitor decoded packets and send a position beacon. RX-first: arming starts the
 * AFSK-1200 decoder; a beacon is an explicit, gated one-shot send (never automatic).
 */
export function AprsCockpit({
  active,
  onTune,
  radio,
  onSetTxEnabled,
  theme,
  myGrid = '',
}: {
  active: boolean
  /** Palette for the embedded APRS map. */
  theme: Theme
  /** Operator's grid — centers the map on the station. */
  myGrid?: string
  /** QSY to an APRS dial (MHz): 2 m FM simplex, auto-routing to the 2 m-capable radio. */
  onTune?: (dialMhz: number) => void
  /** Live rig readout (dial/band/mode + TX-enable) — the TopBar's is hidden on this view. */
  radio?: {
    dialMhz: number
    band: string
    sideband: string
    txEnabled: boolean
    transmitting?: boolean
    /** Receive coverage from Hamlib's caps; EMPTY = unknown = allow (see `radioCoversMhz`). */
    rxRangesMhz?: [number, number][]
  }
  /** Arm/disarm TX (the TopBar's Enable-Tx is hidden here, so APRS carries its own — otherwise a
   * beacon/message is gated off with no way to turn TX on). */
  onSetTxEnabled?: (on: boolean) => void
}) {
  // NO local `armed` state. Arming lives on the ENGINE and is session state that outlives this
  // component, so a local copy drifts: a remount came back up saying "Monitor" while the decoder
  // was still running, and its first click then sent arm(true) at an already-armed engine. The
  // health poll below already carries the flag — one source of truth for the button AND the
  // decode chip, which can therefore never disagree.
  // One selection shared by the list and the map — clicking either highlights both.
  const [selected, setSelected] = useState<string | null>(null)
  const [freq, setFreq] = useState(144.39)
  const [heard, setHeard] = useState<AprsHeard[]>([])
  // The STATION roster — what the list and map draw. The packet log above still feeds the packet
  // pane and the message list, which are about events rather than stations.
  const [roster, setRoster] = useState<AprsStationsView>(EMPTY_ROSTER)
  // The operator's settings, held whole so a board write can ride along without clobbering the
  // ~170 fields it does not touch. THE SAME state the Settings panel edits — see `writeAprsIs`.
  const [settings, setSettingsState] = useState<Settings | null>(null)
  const [inetOpen, setInetOpen] = useState(false)
  const [savingInet, setSavingInet] = useState(false)
  const inetPanelRef = useRef<HTMLDivElement | null>(null)
  const [health, setHealth] = useState<AprsHealth | null>(null)
  const [isStatus, setIsStatus] = useState<AprsIsStatus | null>(null)
  // Show stations the internet reported. On by default when the feed is running (there is no
  // point subscribing to a feed you then hide), but one click hides every station our own antenna
  // has not heard — which is the honest view of what this radio can actually reach.
  const [showInet, setShowInet] = useState(true)
  const [lat, setLat] = useState('')
  const [lon, setLon] = useState('')
  const [comment, setComment] = useState('Nexus APRS')
  const [symbol, setSymbol] = useState('>')
  // Stable empty inputs for the embedded map (it plots APRS only — no decode
  // stations, spots or needs), so MapView's per-tick projections don't rebuild.
  const noStations = useMemo(() => [] as Station[], [])
  const noNeeds = useMemo(() => new Map<string, NeedTag>(), [])
  const noSelectCall = useMemo(() => () => {}, [])
  const [path, setPath] = useState('WIDE1-1,WIDE2-1')
  const [msgTo, setMsgTo] = useState('')
  const [msgText, setMsgText] = useState('')
  const [status, setStatus] = useState<string | null>(null)
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000))
  const [me, setMe] = useState<LatLon | null>(null)
  const prefilled = useRef(false)
  const autoTuned = useRef(false)
  const autoArmed = useRef(false)

  // Can the radio reach the selected APRS channel? `null` = unknown, which must read as "yes,
  // try it" — the backend's refusal handling is the backstop for a radio whose caps we can't read.
  const canReachAprs = radioCoversMhz(radio?.rxRangesMhz, freq) !== false

  // Default to the APRS radio on ENTERING the view: hand off to the 2 m-capable rig, land on the
  // selected APRS frequency in FM. This is the operator's "hitting APRS should default to the 9700"
  // — done once per entry (rising edge of `active`), never on every render, and never keys TX.
  //
  // ⚠️ GATED ON COVERAGE. On an HF-only radio this auto-tune was the whole bug: entering the view
  // commanded 144.390 at a radio that cannot go there, and the fallout (a refused command reported
  // as success, a dial stuck on a frequency the radio was never on, a wedged CAT link) all followed
  // from it. Never command a radio somewhere it provably cannot go.
  useEffect(() => {
    if (!active) {
      autoTuned.current = false
      return
    }
    if (!autoTuned.current && onTune && canReachAprs) {
      autoTuned.current = true
      onTune(freq)
    }
  }, [active, onTune, freq, canReachAprs])

  // Arm the decoder on ENTERING the view, so APRS does not open on a dead screen the operator has
  // to notice and fix. Rising edge of `active`, not mount — the cockpit is kept alive across
  // navigation (App.tsx renders it hidden), so it mounts once per session.
  //
  // ⚠️ RECEIVE-ONLY, and the engine is what guarantees that: `aprs_auto_arm` never confers the
  // auto-ack, only upgrades from off, and refuses once the operator has explicitly stopped the
  // decoder this session. The policy lives there rather than in a ref here so it cannot be lost
  // to a remount — which is exactly how the armed-state desync happened.
  useEffect(() => {
    if (!active) {
      autoArmed.current = false
      return
    }
    if (autoArmed.current) return
    autoArmed.current = true
    void aprsAutoArm()
      .then(() => getAprsHealth())
      .then(setHealth)
      .catch(() => {})
  }, [active])

  // Prefill the beacon lat/lon from the operator's grid (and remember it for distance/bearing), once.
  useEffect(() => {
    if (prefilled.current) return
    prefilled.current = true
    void getSettings()
      .then((s) => {
        setSettingsState(s)
        const ll = gridToLatLon(s.mygrid || '')
        if (ll) {
          setLat(ll.lat.toFixed(4))
          setLon(ll.lon.toFixed(4))
          setMe(ll)
        }
      })
      .catch(() => {})
  }, [])

  // Poll the heard list + decoder health (and tick the age clock) while the cockpit is visible.
  useEffect(() => {
    if (!active) return
    let alive = true
    const tick = () => {
      setNow(Math.floor(Date.now() / 1000))
      void getAprsHeard()
        .then((h) => alive && setHeard((prev) => (samePackets(prev, h) ? prev : h)))
        .catch(() => {})
      void getAprsStations()
        .then((v) => alive && setRoster((prev) => (sameRoster(prev, v) ? prev : v)))
        .catch(() => {})
      void getAprsHealth()
        .then((h) => alive && setHealth(h))
        .catch(() => {})
      void getAprsIsStatus()
        .then((s) => alive && setIsStatus(s))
        .catch(() => {})
    }
    tick()
    const id = window.setInterval(tick, 2000)
    return () => {
      alive = false
      window.clearInterval(id)
    }
  }, [active])

  // The chip judges against the rig's ACTUAL dial/mode and the APRS channel the operator has
  // selected — so it can say "you are on the FT8 frequency" instead of guessing from audio.
  /**
   * Write an APRS-IS setting from the board.
   *
   * ⭐ ONE SOURCE OF TRUTH. The board keeps no copy of anything — it writes the same `Settings`
   * fields the Settings panel reads and writes, through the same `set_settings` command, so the two
   * surfaces cannot disagree. Sends the WHOLE settings object with the change merged in:
   * `set_settings` replaces the struct wholesale, so posting a partial would blank every one of the
   * ~170 fields the board does not know about.
   *
   * The backend reconnects the feed only when the server, filter or callsign actually changed.
   */
  const writeAprsIs = (patch: Partial<Settings>) => {
    if (!settings) return
    const next = { ...settings, ...patch }
    setSettingsState(next) // optimistic, so the control does not lag a round-trip
    setSavingInet(true)
    void setSettings(next)
      .then(() => getSettings())
      .then((fresh) => {
        // Re-read rather than trust the optimistic copy: the engine merges live radio state into
        // what it persists, so its version is authoritative.
        setSettingsState(fresh)
        setStatus(null)
      })
      .catch((e) => setStatus(String(e)))
      .finally(() => setSavingInet(false))
  }

  // Escape closes the internet panel and a click outside dismisses it — the app's dialog idiom.
  useEffect(() => {
    if (!inetOpen) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation()
        setInetOpen(false)
      }
    }
    const onDown = (e: MouseEvent) => {
      if (!inetPanelRef.current?.contains(e.target as Node)) setInetOpen(false)
    }
    window.addEventListener('keydown', onKey)
    // Deferred a tick so the click that OPENED the panel does not immediately close it.
    const id = window.setTimeout(() => document.addEventListener('mousedown', onDown), 0)
    return () => {
      window.removeEventListener('keydown', onKey)
      document.removeEventListener('mousedown', onDown)
      window.clearTimeout(id)
    }
  }, [inetOpen])


  // The selected station's record. One selection, three views of it: the list row, the map marker,
  // and this card — all reading the same store entry, so they cannot disagree.
  const selectedStation = useMemo(
    () => roster.stations.find((st) => st.call === selected) ?? null,
    [roster, selected],
  )

  const decode = useMemo(
    () => aprsDecodeStatus(health, now, radio ?? null, freq),
    [health, now, radio, freq],
  )

  /** Tune to the selected APRS channel, and SAY what happened. A tune the radio cannot take
   * right now (an over in flight) must never look like a button that did nothing — the operator
   * pressed a control whose whole meaning is "move the radio". */
  const tuneToAprs = (mhz: number) => {
    if (!onTune) return
    onTune(mhz)
    setStatus(
      radio?.transmitting
        ? `Transmitting right now — the radio will move to ${mhz.toFixed(3)} when this over ends.`
        : `Tuning to ${mhz.toFixed(3)} FM…`,
    )
  }
  const {
    state: inetState,
    label: inetLabel,
    detail: inetDetail,
  } = useMemo(() => aprsInetStatus(isStatus, now), [isStatus, now])
  // The engine's arm state, as of the last poll. Null health (before the first poll) reads as
  // disarmed, which matches how the engine starts.
  const arm = health?.arm ?? 'off'
  const armed = arm !== 'off'

  const toggleArm = () => {
    // A plain start/stop toggle. Clicking it while armed ALWAYS stops — including when the
    // decoder was auto-armed on view entry. It deliberately does NOT "upgrade" an auto-arm to an
    // explicit one: the button reads "● Monitoring", so a click is the operator reaching for
    // stop, and turning that same click into "grant unattended-transmit capability" would be the
    // most dangerous surprise available here. Explicit arm is reached from OFF, which is what the
    // auto-arm tooltip tells the operator.
    //
    // Re-read health after the round trip rather than assuming it worked: an arm the engine
    // refuses must not leave the button claiming to be monitoring.
    void aprsArm(!armed)
      .then((h) => {
        setHeard(h)
        return getAprsHealth()
      })
      .then(setHealth)
      .catch((e) => setStatus(String(e)))
  }

  const sendBeacon = () => {
    const la = Number.parseFloat(lat)
    const lo = Number.parseFloat(lon)
    if (!Number.isFinite(la) || !Number.isFinite(lo)) {
      setStatus('Enter a valid latitude and longitude first.')
      return
    }
    const digis = path
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean)
    setStatus('Sending beacon…')
    void aprsSendBeacon(la, lo, '/', symbol, comment, digis)
      .then(() => setStatus('Beacon queued — keying now.'))
      .catch((e) => setStatus(String(e)))
  }

  const sendMessage = () => {
    const to = msgTo.trim()
    const text = msgText.trim()
    if (!to || !text) {
      setStatus('Enter a callsign and a message first.')
      return
    }
    setStatus('Sending message…')
    void aprsSendMessage(to, text)
      .then(() => {
        setStatus(`Message to ${to.toUpperCase()} queued — keying now.`)
        setMsgText('')
      })
      .catch((e) => setStatus(String(e)))
  }

  // The station roster, filtered by the "show internet stations" toggle. Applied ONCE here so the
  // list, the counts and the map can never disagree about which stations exist.
  //
  // No per-station source accumulation any more: the backend store derives each station's source
  // from when each channel last carried it, which is one source of truth instead of two that could
  // drift.
  const visibleStations = useMemo(
    () => (showInet ? roster.stations : roster.stations.filter((st) => st.sourceKind !== 'inet')),
    [roster, showInet],
  )

  // How many VISIBLE stations actually carry a position — a station heard only via a message or
  // status packet has none, so "nothing on the map" is a normal state worth naming.
  const positioned = useMemo(
    () => visibleStations.filter((st) => st.lat != null && st.lon != null).length,
    [visibleStations],
  )

  // Messages keep their OWN chronological list from the PACKET log (newest first) — they are
  // events, not stations, and a conversation of several lines from one station must all show.
  const messages = useMemo(
    () => heard.filter((h) => h.kind === 'message').slice().reverse(),
    [heard],
  )

  // The station rows: the backend already collapsed packets into stations and ordered them
  // newest-heard first, so this only decorates them with distance + bearing from the operator.
  const rows = useMemo(
    () =>
      visibleStations.map((st) => {
        const there = st.lat != null && st.lon != null ? { lat: st.lat, lon: st.lon } : null
        return {
          st,
          dist: me && there ? haversineKm(me, there) : null,
          brg: me && there ? bearingDeg(me, there) : null,
        }
      }),
    [visibleStations, me],
  )

  // How many stations the internet contributed that our own receiver has NOT heard — the number
  // the toggle hides, and worth naming so its effect is never a surprise.
  const inetOnly = useMemo(
    () => roster.stations.filter((st) => st.sourceKind === 'inet').length,
    [roster],
  )

  return (
    <main className="layout single needed-panel aprs-cockpit">
      <div className="np-head">
        <h2>APRS</h2>
        <span className="np-count">{rows.length}</span>
        {heard.length !== rows.length && (
          <span className="np-count np-count-filtered">{heard.length} pkts</span>
        )}
        <span className="np-hint">AFSK-1200 packet — decode positions/messages, send a beacon</span>
        {onTune && (
          <>
            <select
              className="np-chip aprs-freq"
              value={freq}
              onChange={(e) => {
                // Selecting a frequency retunes the rig immediately (band-picker behavior) — no
                // separate Tune click needed. Switches to the 2 m radio + FM simplex via onTune.
                // The SELECTION always sticks (it drives the health chip's "which channel do you
                // mean", and every APRS channel is 2 m — so on an HF-only radio a pick must still
                // register); only the RETUNE is gated on the radio being able to get there.
                const f = Number(e.target.value)
                setFreq(f)
                if (radioCoversMhz(radio?.rxRangesMhz, f) !== false) tuneToAprs(f)
              }}
              title="APRS frequency by region — selecting one tunes the rig (2 m FM, AFSK-1200)"
            >
              {APRS_FREQS.map(([f, region]) => (
                <option key={f} value={f}>
                  {f.toFixed(3)} · {region}
                </option>
              ))}
            </select>
            {/* No "move the radio" control at a frequency the radio cannot reach — a button whose
                only possible outcome is a refusal is worse than no button. The health chip beside
                it carries the explanation. */}
            <button
              type="button"
              className="np-chip"
              disabled={!canReachAprs}
              onClick={() => tuneToAprs(freq)}
              title={
                canReachAprs
                  ? 'Re-tune the rig to the selected APRS frequency (2 m FM simplex; switches to your 2 m radio)'
                  : `This radio doesn't cover ${freq.toFixed(3)} MHz — RF APRS needs a VHF radio.`
              }
            >
              Re-tune
            </button>
          </>
        )}
        {radio && (
          <span
            className="aprs-dial"
            title="The rig's current dial / band / mode (this view hides the top bar's readout)"
          >
            {radio.dialMhz.toFixed(3)} MHz · {radio.band} · {radio.sideband || '—'}
          </span>
        )}
        {onSetTxEnabled && radio && (
          <button
            type="button"
            className={`np-chip${radio.txEnabled ? ' active' : ''}`}
            aria-pressed={radio.txEnabled}
            onClick={() => onSetTxEnabled(!radio.txEnabled)}
            title={
              radio.txEnabled
                ? 'Transmit ENABLED — beacons/messages will go out. Click to disable.'
                : 'Transmit is OFF — enable it before a beacon or message can send.'
            }
          >
            {radio.txEnabled ? 'TX On' : 'TX Off'}
          </button>
        )}
        {/* Three states, because "decoding" and "may transmit an ack by itself" are different
            things and the operator has to be able to tell them apart. Auto-armed must never look
            ack-capable — see aprs_auto_ack's gate. */}
        <button
          type="button"
          className={`np-chip${armed ? ' active' : ''}`}
          aria-pressed={armed}
          onClick={toggleArm}
          title={
            arm === 'explicit'
              ? 'You armed the decoder, so automatic acks are allowed — an incoming message ' +
                'addressed to you is acked when TX is on. Click to stop.'
              : arm === 'auto'
                ? 'Armed automatically when you opened APRS: RECEIVE ONLY. It will never send ' +
                  'an automatic ack. To allow those, stop it and arm it yourself, then turn TX ' +
                  'on. Click to stop.'
                : 'Arm the APRS decoder on the RX audio. Arming it yourself also allows ' +
                  'automatic acks once TX is on.'
          }
        >
          {arm === 'auto' ? '● Monitoring (auto)' : arm === 'explicit' ? '● Monitoring' : 'Monitor'}
        </button>
        {/* Decode health, carrying the live input level. An empty APRS screen used to be one
            answer to several different questions — dead capture, squelched channel, unreadable
            signal, quiet band. Only ONE of those is a fault, and the first cut of this chip
            called the most common of them (a closed squelch) a broken audio device. */}
        <span
          className={`aprs-health aprs-health-${decode.state}`}
          role="status"
          title={decode.detail}
        >
          {decode.label}
        </span>
        {/* One-click resolution for the one state that has one. The chip names who owns the
            dial; this button takes it back. */}
        {decode.state === 'wrongfreq' && onTune && (
          <button
            type="button"
            className="np-chip aprs-health-fix"
            onClick={() => tuneToAprs(freq)}
            title={`Tune the radio to ${freq.toFixed(3)} FM for APRS`}
          >
            Tune to {freq.toFixed(3)}
          </button>
        )}
        {/* The OTHER inlet's health. Deliberately a second chip, not a merged one: the RF chain
            and the internet feed fail independently, and seeing a green internet chip beside a
            silent RF chip is the whole diagnostic — it proves the fault is in the radio path. */}
        {/* ⭐ The internet chip is also the internet CONTROL. The chip's own guidance says "widen
            the radius or add watched calls" when the feed is quiet, so the controls that act on
            that advice have to be reachable from where the advice is given — one click, not a trip
            to Settings. Grouped in a panel rather than inline because this cockpit already carries
            a frequency picker, Re-tune, TX, Monitor, a decode chip and this one; three more inline
            controls would be clutter. */}
        <div className="aprs-inet-wrap" ref={inetPanelRef}>
          <button
            type="button"
            className={`aprs-inet aprs-inet-${inetState}${inetOpen ? ' open' : ''}`}
            aria-expanded={inetOpen}
            aria-haspopup="dialog"
            onClick={() => setInetOpen(!inetOpen)}
            title={`${inetDetail}\n\nClick for internet feed controls.`}
          >
            {isStatus?.enabled ? inetLabel : 'Internet off'}
          </button>
          {inetOpen && (
            <div className="aprs-inet-panel" role="dialog" aria-label="APRS-IS internet feed">
              <p className="aprs-inet-detail">{inetDetail}</p>

              <label className="aprs-inet-row">
                <span>Internet feed</span>
                <button
                  type="button"
                  role="switch"
                  aria-checked={!!settings?.aprsIsEnabled}
                  className={`toggle${settings?.aprsIsEnabled ? ' on' : ''}`}
                  disabled={!settings || savingInet}
                  onClick={() => writeAprsIs({ aprsIsEnabled: !settings?.aprsIsEnabled })}
                >
                  <span className="toggle-knob" />
                </button>
              </label>

              <label className="aprs-inet-row">
                <span>Radius (km)</span>
                <input
                  type="number"
                  min={0}
                  max={5000}
                  className="settings-input"
                  value={settings?.aprsIsRadiusKm ?? 150}
                  disabled={!settings || savingInet}
                  onChange={(e) => writeAprsIs({ aprsIsRadiusKm: Number(e.target.value) })}
                />
              </label>

              <label className="aprs-inet-row aprs-inet-row-wide">
                <span>Watched calls</span>
                <input
                  type="text"
                  className="settings-input"
                  placeholder="W9XYZ-9, KD9ABC"
                  spellCheck={false}
                  defaultValue={(settings?.aprsIsWatchCalls ?? []).join(', ')}
                  disabled={!settings || savingInet}
                  // On blur, not per keystroke: each write reconnects the feed, so committing
                  // mid-callsign would drop the session on every character typed.
                  onBlur={(e) =>
                    writeAprsIs({
                      aprsIsWatchCalls: e.target.value
                        .split(',')
                        .map((c) => c.trim().toUpperCase())
                        .filter(Boolean),
                    })
                  }
                />
              </label>

              <p className="aprs-inet-note">
                Changing the radius or watched calls reconnects the feed — the server does the
                filtering, so a new subscription has to be sent. Server, port, traffic types and the
                iGate live in Settings ▸ Modes ▸ APRS.
              </p>
            </div>
          )}
        </div>
        {/* Hide everything our own antenna has not heard. The count is named so the effect of
            the click is never a surprise. */}
        {inetOnly > 0 && (
          <button
            type="button"
            className={`np-chip${showInet ? ' on' : ''}`}
            aria-pressed={showInet}
            onClick={() => setShowInet(!showInet)}
            title={
              showInet
                ? `Hide the ${inetOnly} station${inetOnly === 1 ? '' : 's'} only the internet has reported, leaving what this radio actually hears`
                : `Show the ${inetOnly} station${inetOnly === 1 ? '' : 's'} the internet feed reports`
            }
          >
            {showInet ? `Internet ${inetOnly}` : `Internet ${inetOnly} hidden`}
          </button>
        )}
      </div>

      {/* ⭐ APRS IS A GEOGRAPHIC MODE AND HAD NO MAP. Everything lived in one
          vertical stack, so on any real window the controls bunched into the
          top-left and the rest of the section was empty — operator, 2026-07-29:
          "all information is in a small area in the top left, and you still have
          three-quarters of a black box of items."
          The controls and lists become a left rail; the map takes the space they
          were not using. Positions were already in the packets (AprsHeard carries
          lat/lon, course and speed) — nothing new is decoded for this. */}
      <div className="aprs-body">
        <div className="aprs-rail">
      <div className="aprs-beacon">
        <span className="aprs-beacon-title">Position beacon</span>
        <label>
          Lat
          <input value={lat} onChange={(e) => setLat(e.target.value)} inputMode="decimal" size={9} />
        </label>
        <label>
          Lon
          <input value={lon} onChange={(e) => setLon(e.target.value)} inputMode="decimal" size={9} />
        </label>
        <label>
          Symbol
          <select value={symbol} onChange={(e) => setSymbol(e.target.value)}>
            {SYMBOLS.map(([code, name]) => (
              <option key={code} value={code}>
                {name}
              </option>
            ))}
          </select>
        </label>
        <label className="aprs-beacon-comment">
          Comment
          <input value={comment} onChange={(e) => setComment(e.target.value)} maxLength={43} />
        </label>
        <label>
          Path
          <input value={path} onChange={(e) => setPath(e.target.value)} size={14} />
        </label>
        <button type="button" className="np-chip aprs-beacon-send" onClick={sendBeacon}>
          Send beacon
        </button>
        {status && <span className="aprs-status">{status}</span>}
      </div>

      <div className="aprs-beacon aprs-message-compose">
        <span className="aprs-beacon-title">Message</span>
        <label>
          To
          <input
            value={msgTo}
            onChange={(e) => setMsgTo(e.target.value)}
            placeholder="callsign"
            size={9}
          />
        </label>
        <label className="aprs-beacon-comment">
          Text
          <input
            value={msgText}
            onChange={(e) => setMsgText(e.target.value)}
            maxLength={67}
            placeholder="up to 67 chars"
            onKeyDown={(e) => e.key === 'Enter' && sendMessage()}
          />
        </label>
        <span className="aprs-msg-count">{msgText.length}/67</span>
        <button type="button" className="np-chip aprs-beacon-send" onClick={sendMessage}>
          Send message
        </button>
      </div>

      {messages.length > 0 && (
        <div className="aprs-messages">
          <span className="aprs-beacon-title">Messages</span>
          <ul className="aprs-msg-list">
            {messages.map((m, i) => (
              <li key={`${m.source}-${m.msgId ?? i}-${m.atUnix}`} className="aprs-msg-row">
                <span className="aprs-age">{ageLabel(m.atUnix, now)}</span>
                <span className="aprs-from">{m.source}</span>
                <span className="aprs-msg-arrow">→</span>
                <span className="aprs-msg-to">{m.addressee ?? '?'}</span>
                {m.msgId && <span className="aprs-msg-id">#{m.msgId}</span>}
                <span className="aprs-msg-text">
                  {m.text.replace(/^→[^:]*:\s*/, '')}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {rows.length === 0 ? (
        <div className="np-empty">{decode.detail}</div>
      ) : (
        <table className="aprs-table">
          <thead>
            <tr>
              <th>Age</th>
              <th aria-label="Symbol" />
              <th>From</th>
              <th>Via</th>
              <th>Type</th>
              <th>Position</th>
              <th>Dist</th>
              <th>Info</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(({ st, dist, brg }) => (
              // Selecting here selects on the map and vice versa — one selection,
              // two views of it. Clicking the same row again clears it.
              <tr
                key={st.call}
                className={selected === st.call ? 'sel' : undefined}
                // A station going quiet fades in the list exactly as it does on the map, from the
                // same thresholds the backend sent, so the two readings cannot disagree.
                style={{
                  opacity: ageFade(st.lastHeardUnix, now, roster.fadeAfterMin, roster.ttlMin),
                }}
                onClick={() => setSelected(selected === st.call ? null : st.call)}
                title={
                  st.lat != null && st.lon != null
                    ? `Highlight ${st.call} on the map`
                    : `${st.call} reported no position — nothing to highlight`
                }
              >
                <td className="aprs-age">{ageLabel(st.lastHeardUnix, now)}</td>
                <td className="aprs-sym-cell">
                  <AprsSymbolIcon table={st.symbolTable} code={st.symbolCode} />
                </td>
                <td className="aprs-from">{st.call}</td>
                <td
                  className={`aprs-src aprs-src-${st.sourceKind}`}
                  title={SOURCE_TITLE[st.sourceKind]}
                >
                  {SOURCE_LABEL[st.sourceKind]}
                </td>
                <td className={`aprs-kind aprs-kind-${st.kind}`}>{st.kind}</td>
                <td className="aprs-pos">
                  {st.lat != null && st.lon != null
                    ? `${st.lat.toFixed(4)}, ${st.lon.toFixed(4)}${
                        st.speedKnots ? ` · ${st.speedKnots}kt ${st.courseDeg}°` : ''
                      }`
                    : '—'}
                </td>
                <td className="aprs-dist">
                  {dist != null ? `${Math.round(dist)} km ${brg != null ? compass(brg) : ''}` : ''}
                </td>
                <td className="aprs-info">{st.text}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
        </div>
        <div className="aprs-map">
          <MapView
            embedded={{ aprs: true }}
            aprs={visibleStations}
            aprsFadeAfterMin={roster.fadeAfterMin}
            aprsTtlMin={roster.ttlMin}
            selectedAprs={selected}
            onSelectAprs={setSelected}
            myGrid={myGrid}
            theme={theme}
            stations={noStations}
            prop={null}
            selectedCall={null}
            onSelectCall={noSelectCall}
            needByCall={noNeeds}
          />
          {positioned === 0 && (
            <div className="aprs-map-empty">
              {decode.state === 'decoding'
                ? 'No positions heard yet — status and message packets carry none.'
                : decode.detail}
            </div>
          )}
          {/* Selecting a station — from the map OR the list — opens its detail card over the map.
              Both routes set the same `selected`, so the card serves both without a second path. */}
          {selectedStation && (
            <AprsStationCard
              station={selectedStation}
              nowSec={now}
              me={me}
              onClose={() => setSelected(null)}
            />
          )}
        </div>
      </div>
    </main>
  )
}
