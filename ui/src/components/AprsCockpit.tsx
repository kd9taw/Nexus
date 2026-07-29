import { MapView } from './MapView'
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
  getSettings,
  type AprsHealth,
  type AprsHeard,
} from '../api'
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
  if (!(peak > 0)) return 'level: digital silence (exactly zero)'
  return `level: peak ${Math.round(20 * Math.log10(peak))} dBFS`
}

/** How long the tap may go without any samples ARRIVING before the capture counts as dead.
 *
 * The decode thread polls every 100 ms but the radio loop that feeds it can take far longer per
 * iteration (blocking CAT, up to 2500 ms on slow serial), so gaps of a second or two are normal.
 * Judging on the instant would cry wolf constantly on a healthy station. */
const AUDIO_STALE_SEC = 5

/** How far the dial may sit from the APRS channel before it counts as a different frequency.
 * 5 kHz — wider than any rounding or CAT read-back jitter, far narrower than a channel step. */
const DIAL_TOLERANCE_MHZ = 0.005

/** The rig state the chip judges against. Only the two facts that decide whether APRS can
 * possibly hear anything: where the dial is, and whether the rig is in FM. */
export interface AprsRadio {
  dialMhz: number
  sideband: string
}

export type AprsDecodeState =
  | 'off'
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
      const where = `${radio.dialMhz.toFixed(3)}${modeKnown ? ` ${radio.sideband.toUpperCase()}` : ''}`
      return {
        state: 'wrongfreq',
        label: 'Wrong frequency',
        detail:
          `The radio is on ${where} — APRS needs ${wantDialMhz.toFixed(3)} FM. Nothing on this ` +
          'channel can decode as APRS packet, whatever the audio level says. Tune to the APRS ' +
          'channel to start hearing it.',
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
  if (health.framesDecoded > 0) {
    return {
      state: 'decoding',
      label: `${health.framesDecoded} decoded`,
      detail:
        (health.lastDecodeUnix == null
          ? `${health.framesDecoded} packets decoded.`
          : `${health.framesDecoded} packets decoded, last ${ageLabel(health.lastDecodeUnix, nowSec)} ago.`) +
        ` Input ${level}.`,
    }
  }
  if (health.framesSeen > 0) {
    return {
      state: 'unreadable',
      label: `${health.framesSeen} failed CRC`,
      detail:
        `${health.framesSeen} packets were heard but none passed the checksum. Some of that is ` +
        'normal: when the squelch opens partway through a burst the start of the packet is lost, ' +
        'and a part-heard packet can never pass. It is only a fault if nothing ever decodes — in ' +
        'which case check the rig is on 144.390 in FM, and that the RX audio is not so hot that ' +
        `it is clipping. Input ${level}.`,
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
        `reads silent with the squelch open, the wrong input device is selected. Input ${level}.`,
    }
  }
  return {
    state: 'listening',
    label: 'Listening',
    detail: `Audio is reaching the decoder and no packets have been heard yet — a quiet channel. Input ${level}.`,
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
  radio?: { dialMhz: number; band: string; sideband: string; txEnabled: boolean; transmitting?: boolean }
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
  const [health, setHealth] = useState<AprsHealth | null>(null)
  const [lat, setLat] = useState('')
  const [lon, setLon] = useState('')
  const [comment, setComment] = useState('Nexus APRS')
  const [symbol, setSymbol] = useState('>')
  // Stable empty inputs for the embedded map (it plots APRS only — no decode
  // stations, spots or needs), so MapView's per-tick projections don't rebuild.
  const noStations = useMemo(() => [] as Station[], [])
  const noNeeds = useMemo(() => new Map<string, NeedTag>(), [])
  const noSelectCall = useMemo(() => () => {}, [])
  // How many heard stations actually carry a position — status and message
  // packets carry none, so "nothing on the map" is a normal state worth naming.
  const positioned = useMemo(
    () => heard.filter((h) => h.lat != null && h.lon != null).length,
    [heard],
  )
  const [path, setPath] = useState('WIDE1-1,WIDE2-1')
  const [msgTo, setMsgTo] = useState('')
  const [msgText, setMsgText] = useState('')
  const [status, setStatus] = useState<string | null>(null)
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000))
  const [me, setMe] = useState<LatLon | null>(null)
  const prefilled = useRef(false)
  const autoTuned = useRef(false)
  const autoArmed = useRef(false)

  // Default to the APRS radio on ENTERING the view: hand off to the 2 m-capable rig, land on the
  // selected APRS frequency in FM. This is the operator's "hitting APRS should default to the 9700"
  // — done once per entry (rising edge of `active`), never on every render, and never keys TX.
  useEffect(() => {
    if (!active) {
      autoTuned.current = false
      return
    }
    if (!autoTuned.current && onTune) {
      autoTuned.current = true
      onTune(freq)
    }
  }, [active, onTune, freq])

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
        .then((h) => alive && setHeard(h))
        .catch(() => {})
      void getAprsHealth()
        .then((h) => alive && setHealth(h))
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

  // Messages get their OWN chronological list (newest first) — never collapsed by source, so a
  // conversation of several lines from one station all show. Positions are the roster below.
  const messages = useMemo(
    () => heard.filter((h) => h.kind === 'message').slice().reverse(),
    [heard],
  )

  // Collapse the POSITION stream to ONE row per station (latest wins — `heard` is oldest→newest),
  // newest first, with distance + bearing from the operator's grid. Messages are excluded (above).
  const rows = useMemo(() => {
    const bySource = new Map<string, AprsHeard>()
    for (const h of heard) {
      if (h.kind === 'message') continue
      bySource.set(h.source, h)
    }
    return [...bySource.values()]
      .sort((a, b) => b.atUnix - a.atUnix)
      .map((h) => {
        const hasPos = h.lat != null && h.lon != null
        const there = hasPos ? { lat: h.lat as number, lon: h.lon as number } : null
        return {
          h,
          dist: me && there ? haversineKm(me, there) : null,
          brg: me && there ? bearingDeg(me, there) : null,
        }
      })
  }, [heard, me])

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
                const f = Number(e.target.value)
                setFreq(f)
                tuneToAprs(f)
              }}
              title="APRS frequency by region — selecting one tunes the rig (2 m FM, AFSK-1200)"
            >
              {APRS_FREQS.map(([f, region]) => (
                <option key={f} value={f}>
                  {f.toFixed(3)} · {region}
                </option>
              ))}
            </select>
            <button
              type="button"
              className="np-chip"
              onClick={() => tuneToAprs(freq)}
              title="Re-tune the rig to the selected APRS frequency (2 m FM simplex; switches to your 2 m radio)"
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
              <th>From</th>
              <th>Type</th>
              <th>Position</th>
              <th>Dist</th>
              <th>Info</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(({ h, dist, brg }) => (
              // Selecting here selects on the map and vice versa — one selection,
              // two views of it. Clicking the same row again clears it.
              <tr
                key={h.source}
                className={selected === h.source ? 'sel' : undefined}
                onClick={() => setSelected(selected === h.source ? null : h.source)}
                title={
                  h.lat != null && h.lon != null
                    ? `Highlight ${h.source} on the map`
                    : `${h.source} reported no position — nothing to highlight`
                }
              >
                <td className="aprs-age">{ageLabel(h.atUnix, now)}</td>
                <td className="aprs-from">{h.source}</td>
                <td className={`aprs-kind aprs-kind-${h.kind}`}>{h.kind}</td>
                <td className="aprs-pos">
                  {h.lat != null && h.lon != null
                    ? `${h.lat.toFixed(4)}, ${h.lon.toFixed(4)}${
                        h.speedKnots ? ` · ${h.speedKnots}kt ${h.courseDeg}°` : ''
                      }`
                    : '—'}
                </td>
                <td className="aprs-dist">
                  {dist != null ? `${Math.round(dist)} km ${brg != null ? compass(brg) : ''}` : ''}
                </td>
                <td className="aprs-info">{h.text}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
        </div>
        <div className="aprs-map">
          <MapView
            embedded={{ aprs: true }}
            aprs={heard}
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
        </div>
      </div>
    </main>
  )
}
