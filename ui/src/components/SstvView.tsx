import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { AppSnapshot, BandChannel, SstvGalleryEntry, SstvHealth, SstvState } from '../types'
import { Waterfall } from './Waterfall'
import { CockpitHeader } from './CockpitHeader'
import { FrequencyControl } from './FrequencyControl'
import { PanelsMenu } from './PanelsMenu'
import { CockpitPaneFrame } from './panes/CockpitPaneFrame'
import { panelHost } from '../features/panelHost'
import { sstvImageWidth } from '../sstvScale'
import { readExifOrientation, readIntrinsicSize, sniffImageKind, type ImageKind } from '../sstvExif'
import { effectiveOrientation, orientTransform } from '../sstvOrient'
import {
  CENTRE,
  cropWindow,
  dragCentre,
  freeAxis,
  isExactFit,
  isUpscale,
  nudgeCentre,
  type CropCentre,
} from '../sstvCrop'
import { halvingChain } from '../sstvResample'
import { drawIdPlate, normalizeCall } from '../sstvIdOverlay'
import { SSTV_PANEL_IDS, type SstvPanelId, type PanelLayoutApi } from '../features/panelState'
import {
  getLicensedBandPlan,
  getSstvState,
  haltTx,
  setOperatingMode,
  setRfPower,
  setTune,
  sstvArm,
  sstvAutoArm,
  sstvDeleteImage,
  sstvSend,
  sstvStop,
} from '../api'
import { bandLabelForMhz, sidebandForQsy } from '../band'
import { announce } from '../announce'
import { pushToast, withErrorToast } from '../toast'
// The 15 transmittable modes, their rasters and their exact key-down seconds. A pure
// module because Settings ▸ Digital ▸ SSTV picks the DEFAULT mode from the same rows —
// see its header for why that is not a second hand-written table.
import { MODE_BY_SLUG, SSTV_TX_MODES, TX_MODE_GROUPS } from '../sstvModes'

/** What the file picker offers. The magic-number sniff is what actually decides — an
 *  iPhone HEIC renamed `.jpg` has to be caught by its bytes — but the picker should
 *  not show the operator files it is going to refuse. */
const TX_ACCEPT =
  'image/jpeg,image/png,image/webp,image/bmp,image/gif,.jpg,.jpeg,.png,.webp,.bmp,.gif'

/** Refuse above this many megapixels. The limit is the DECODED buffer, not the file:
 *  80 MP is ~320 MB of RGBA, which is what actually exhausts the webview. A 100 MB
 *  progressive JPEG may be 8000×6000 and a 100 MB BMP 6000×5000 — same file size,
 *  very different cost — so file size is not consulted anywhere. */
const MAX_MEGAPIXELS = 80

/** Header bytes read for the magic sniff, the EXIF walk and the SOF/IHDR dimensions.
 *  Generous enough to clear an EXIF block carrying a full-size thumbnail, and read
 *  through `Blob.slice` so a 100 MB file never lands in memory to be refused. */
const HEADER_BYTES = 262_144

/**
 * A named refusal for a format we positively identified and cannot decode, or null to
 * go ahead and try.
 *
 * ⚠️ `unknown` DELIBERATELY FALLS THROUGH TO THE DECODE. The sniff table cannot
 * enumerate every image a webview can decode, and refusing on "I don't recognise the
 * first twelve bytes" would reject working pictures. Positive identification refuses;
 * absence of identification defers to the decoder, which then gives the honest answer.
 */
function refusalFor(kind: ImageKind): string | null {
  switch (kind) {
    case 'heic':
      return (
        "iPhone HEIC photos can't be read here — Nexus has no HEVC decoder. On the " +
        'iPhone: Settings → Camera → Formats → Most Compatible (new photos are JPEG), ' +
        'or Settings → Photos → Transfer to Mac or PC → Automatic (converts on send). ' +
        'Then re-send this picture.'
      )
    case 'avif':
      return 'That is an AVIF file. SSTV sends JPEG, PNG, WebP, BMP or GIF — export or save-as one of those.'
    case 'tiff':
      return 'That is a TIFF file. SSTV sends JPEG, PNG, WebP, BMP or GIF — export or save-as one of those.'
    case 'raw':
      return 'That is a camera RAW file. SSTV sends JPEG, PNG, WebP, BMP or GIF — export a JPEG from it first.'
    case 'psd':
      return 'That is a Photoshop file. SSTV sends JPEG, PNG, WebP, BMP or GIF — export or save-as one of those.'
    case 'svg':
      return 'That is an SVG drawing, not a photo. SSTV sends JPEG, PNG, WebP, BMP or GIF — export it as one of those.'
    default:
      return null
  }
}

/** The first four bytes as hex, for the "that isn't an image" message — naming what
 *  the file actually starts with beats "could not load that image". */
function magicHex(b: Uint8Array): string {
  return Array.from(b.subarray(0, 4))
    .map((v) => v.toString(16).padStart(2, '0'))
    .join(' ')
}
/** Pack the RGB channels of RGBA canvas data (dropping alpha) into base64 — the
 * raw row-major RGB the `sstv_send` backend validates against the mode's size. */
function rgbToBase64(data: Uint8ClampedArray, pixels: number): string {
  const rgb = new Uint8Array(pixels * 3)
  for (let i = 0, o = 0; i < pixels; i++) {
    const s = i * 4
    rgb[o++] = data[s]
    rgb[o++] = data[s + 1]
    rgb[o++] = data[s + 2]
  }
  // Chunked so a big frame (PD-290 ≈ 1.4 MB) never overflows the call stack.
  let bin = ''
  const CHUNK = 0x8000
  for (let i = 0; i < rgb.length; i += CHUNK) {
    bin += String.fromCharCode.apply(null, rgb.subarray(i, i + CHUNK) as unknown as number[])
  }
  return btoa(bin)
}

/** "1:52" from a seconds count (m:ss). */
function fmtClock(secs: number): string {
  const s = Math.max(0, Math.round(secs))
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
}

interface Props {
  /** Live snapshot — may be absent while the app is still connecting; the shell
   * (canvas / gallery) renders without it, only the header needs it. */
  snap?: AppSnapshot | null
  /** Palette name for the band waterfall (same value the Operate cockpit uses). */
  theme?: string
  /** Wheel sensitivity (Settings) — how much scroll one tuning step costs on the readout. */
  wheelSensitivity?: number
  /** Settings ▸ Digital ▸ SSTV — default transmit mode: a `sstvModes.ts` slug, or
   *  'auto'/undefined to keep the band-aware pick. */
  txModeDefault?: string
  /** Settings ▸ Digital ▸ SSTV — transmit power, percent. null/undefined = leave the
   *  rig's power alone (the shipped behaviour). */
  txPowerPct?: number | null
  /** Apply a snapshot returned by a command without waiting for the poll. */
  onSnap?: (snap: AppSnapshot) => void
  /** True when SSTV is the visible view. The view stays MOUNTED in its
   * keep-alive host (the armed receiver keeps listening in the backend either
   * way); this flag pauses the display poll while hidden — the same gate the
   * FT8 cockpit uses for its render loop. */
  active?: boolean
  /** QSY to a band-plan channel (the shared App setFrequency path). */
  onSetFrequency?: (dialMhz: number, band: string, mode: string) => void
  /** Arm/disarm TX (WSJT-X "Enable Tx") — the header pill becomes the arm control, since the
   * TopBar's Enable-Tx is hidden with the digital chrome in this view. Without it, an SSTV
   * send sits at the "TX is off" gate with no way to arm from this screen. */
  onSetTxEnabled?: (on: boolean) => void
  /** Panel visibility/resize record — host-owned (App), so it survives this view's remounts.
   *  Optional: without it the panels all show and there's no ⊞ menu. */
  panels?: PanelLayoutApi<SstvPanelId>
  /** Open Settings at a section id (see settings/registry.ts). Absent ⇒ the audio and
   * callsign faults below say what to fix without offering to take you there. */
  onOpenSettings?: (target: string) => void
}

/** Display labels for the SSTV removable panels (the ⊞ Panels menu). */
const SSTV_PANEL_LABELS: Record<SstvPanelId, string> = {
  txcompose: 'Transmit',
  gallery: 'Gallery',
}

/** Tauri v2 convertFileSrc without the npm package (this app talks to the
 * injected bridge directly — see api.ts): map an absolute file path to the
 * asset-protocol URL the webview may load under the tauri.conf.json
 * assetProtocol scope (asset://localhost/… on Linux/macOS,
 * http://asset.localhost/… on Windows). Null outside the desktop shell. */
function assetUrl(path: string): string | null {
  const w = window as unknown as {
    __TAURI_INTERNALS__?: { convertFileSrc?: (p: string, protocol?: string) => string }
    __TAURI__?: { core?: { convertFileSrc?: (p: string, protocol?: string) => string } }
  }
  const conv = w.__TAURI_INTERNALS__?.convertFileSrc ?? w.__TAURI__?.core?.convertFileSrc
  try {
    return conv ? conv(path) : null
  } catch {
    return null
  }
}

/** Rasterize one of OUR gallery BMPs (sstv_store.rs writes a fixed layout:
 * 54-byte header, 24 bpp, BI_RGB, bottom-up BGR rows padded to 4 bytes) onto a
 * canvas — the fallback when the webview's <img> can't decode BMP. Tolerates a
 * top-down (negative height) variant; anything else is silently skipped. */
function drawBmp(canvas: HTMLCanvasElement | null, buf: ArrayBuffer): void {
  if (!canvas || buf.byteLength < 54) return
  const v = new DataView(buf)
  if (v.getUint16(0, false) !== 0x424d) return // "BM"
  const off = v.getUint32(10, true)
  const w = v.getInt32(18, true)
  const rawH = v.getInt32(22, true)
  const bpp = v.getUint16(28, true)
  const comp = v.getUint32(30, true)
  if (w <= 0 || rawH === 0 || bpp !== 24 || comp !== 0) return
  const bottomUp = rawH > 0
  const h = Math.abs(rawH)
  const stride = Math.ceil((w * 3) / 4) * 4
  if (off + stride * h > buf.byteLength) return
  const bytes = new Uint8Array(buf)
  const img = new ImageData(w, h)
  for (let y = 0; y < h; y++) {
    const srcRow = off + (bottomUp ? h - 1 - y : y) * stride
    for (let x = 0; x < w; x++) {
      const s = srcRow + x * 3
      const d = (y * w + x) * 4
      img.data[d] = bytes[s + 2] // BGR → RGB
      img.data[d + 1] = bytes[s + 1]
      img.data[d + 2] = bytes[s]
      img.data[d + 3] = 255
    }
  }
  canvas.width = w
  canvas.height = h
  canvas.getContext('2d')?.putImageData(img, 0, 0)
}

// ---------------------------------------------------------------------------
// ⭐ WHAT THE RECEIVER IS HEARING.
//
// THE BUG THIS EXISTS FOR (field report, FTdx10 on 14.236 then 14.230): "I hear a
// signal but the SSTV is not decoding." The idle screen said one thing —
// "Tune 14.230 / 145.800 — images decode here" — whether the receiver was never
// started, the capture device was delivering nothing, the input was live but
// silent, or a strong signal was arriving in a mode this build cannot decode. Four
// completely different situations, three of them fixable in seconds once named,
// all rendered identically. Worse, that hint named two frequencies while the
// operator was sitting on a third.
//
// Same ladder discipline as `aprsDecodeStatus`: most-actionable first, present
// tense only where a recent fact backs it, and never a claim the counters cannot
// support.
// ---------------------------------------------------------------------------

/** How the SSTV receiver is doing — drives the caption's tone as well as its text. */
export type SstvRxState =
  | 'off'
  | 'starting'
  | 'nocapture'
  | 'silent'
  | 'listening'
  | 'unsupported'
  | 'decoded'

/** Audio has to have arrived within this long for the tap to count as fed. The
 * radio loop feeds it on its own cadence and can stall on blocking CAT, so this is
 * generous — it is looking for a dead capture device, not a slow one.
 *
 * Twice `aprsDecodeStatus`'s window on purpose: SSTV shares the radio loop's tap
 * with the FT decoders, whose per-iteration work is far heavier than APRS's. */
const AUDIO_STALE_SEC = 10
/** Wait for a few drains before crying "no input": arming resets the health and the
 * view re-reads it immediately, so `lastAudioUnix` is legitimately null for a moment.
 * 20 drains is ~2 s at the decode thread's 100 ms poll — long enough that a slow
 * first fill cannot flash an alarm, short enough to name a real fault straight away. */
const MIN_DRAINS_BEFORE_CAPTURE_FAULT = 20
/** Below this peak the input is delivering samples but nothing audible. */
const SILENT_PEAK = 0.002
/** An unsupported-mode burst is news for this long; after that it is history. */
const UNKNOWN_VIS_RECENT_SEC = 300

/** "3 min" / "45 s" — the age of a stamped fact. */
function ageLabel(unix: number, nowSec: number): string {
  const s = Math.max(0, nowSec - unix)
  if (s < 90) return `${s} s`
  if (s < 5400) return `${Math.round(s / 60)} min`
  return `${Math.round(s / 3600)} h`
}

/** The SSTV calling channel for the band the radio is on, from the built-in plan —
 * never a hardcoded pair. Prefers the channel nearest the current dial, so an
 * operator on 20 m is told about 14.236 rather than 14.230 when that is where they
 * are. Null when the plan carries nothing for this band. */
export function sstvChannelForDial(plan: BandChannel[], dialMhz?: number): BandChannel | null {
  if (dialMhz == null || !Number.isFinite(dialMhz)) return null
  const band = bandLabelForMhz(dialMhz)
  if (!band) return null
  const onBand = plan.filter((c) => c.band === band || c.band.startsWith(`${band}-`))
  if (onBand.length === 0) return null
  return onBand.reduce((best, c) =>
    Math.abs(c.dialMhz - dialMhz) < Math.abs(best.dialMhz - dialMhz) ? c : best,
  )
}

/** Turn receiver health into what the operator should be told while no picture is
 * coming in. `channel` is the SSTV calling frequency for the band they are on. */
export function sstvDecodeStatus(
  health: SstvHealth | null | undefined,
  nowSec: number,
  channel: BandChannel | null,
): { state: SstvRxState; text: string } {
  // Where to point the radio — appended wherever it helps, and derived from the
  // band plan rather than frozen into a sentence.
  const where = channel
    ? ` Images on this band appear at ${channel.dialMhz.toFixed(3)} ${channel.mode}.`
    : ''
  if (!health || !health.armed) {
    return {
      state: 'off',
      text: `The receiver is stopped — nothing is being decoded. Press Arm to start it.${where}`,
    }
  }
  // NOTHING ARRIVING — the only genuine capture fault, and held apart from a zero
  // LEVEL below because the two have opposite fixes. What you hear on the speaker
  // says nothing about what the app is capturing, which is exactly the trap the
  // field report fell into.
  //
  // The two audio faults name the CONTROL — "Input Device (RX)" is the label the
  // operator is looking for once they get there. They used to say "Settings → Audio
  // input", a name that appears nowhere in the panel; the caption renders a button to
  // the section beside this text, so the sentence no longer has to carry a path.
  const noArrivals = health.lastAudioUnix == null || nowSec - health.lastAudioUnix > AUDIO_STALE_SEC
  if (noArrivals && health.drains >= MIN_DRAINS_BEFORE_CAPTURE_FAULT) {
    return {
      state: 'nocapture',
      text:
        'Listening, but no audio is reaching the decoder at all — the capture device is not ' +
        'delivering anything. Check that Input Device (RX) is the radio; hearing the ' +
        'signal on the speaker does not mean the app is capturing it.',
    }
  }
  // ⚠️ NO READING YET — and it must not be dressed up as one. Arming resets the
  // health and the view re-reads it in the same breath, so for the first couple of
  // seconds nothing has been reported at all: no drains, no stamp, a peak of zero.
  // Without this rung that fell through to `silent`, whose text accuses the operator
  // of having the wrong sound card — shown on EVERY entry to the view, before the
  // decode thread had drained even once, and shown permanently in the one case where
  // it never drains (a decoder that fails to construct). Absence of evidence is its
  // own state; saying so costs nothing and blames nobody.
  if (health.lastAudioUnix == null) {
    return {
      state: 'starting',
      text: `Receiver started — no audio has reached the decoder yet.${where}`,
    }
  }
  // ⭐ AN UNSUPPORTED MODE OUTRANKS EVERYTHING BELOW. A clean header arrived and was
  // thrown away — the single most misleading way SSTV can fail, because the screen
  // is identical to a dead band. This used to be a console line nobody could see.
  if (
    health.unknownVis > 0 &&
    health.lastUnknownVisUnix != null &&
    nowSec - health.lastUnknownVisUnix <= UNKNOWN_VIS_RECENT_SEC
  ) {
    const code = health.lastUnknownVisCode
    return {
      state: 'unsupported',
      text:
        `Heard an SSTV header ${ageLabel(health.lastUnknownVisUnix, nowSec)} ago in a mode this ` +
        `build cannot decode${code != null ? ` (VIS 0x${code.toString(16).toUpperCase()})` : ''}. ` +
        'The signal and the audio path are fine — Scottie, Martin, Robot and PD images all decode.',
    }
  }
  // A decode LATCHES: a completed picture is a durable fact about the whole chain,
  // unlike a claim about what the band is doing right now.
  if (health.images > 0 && health.lastImageUnix != null) {
    return {
      state: 'decoded',
      text: `${health.images} image${health.images === 1 ? '' : 's'} decoded since arming, last one ${ageLabel(health.lastImageUnix, nowSec)} ago. Listening for the next header.`,
    }
  }
  // ARRIVING BUT SILENT — a routing or level problem, not a band problem.
  if (health.audioPeak < SILENT_PEAK) {
    return {
      state: 'silent',
      text:
        'Audio is arriving but it is silent. If you can hear the signal on the speaker, the app ' +
        'is on a different input — check Input Device (RX), and RX Gain if the level is ' +
        `just low.${where}`,
    }
  }
  // THE HEALTHY IDLE STATE. It says the audio is being heard, which is the one thing
  // the old hint could never say.
  return {
    state: 'listening',
    text: `Hearing audio, no SSTV header yet — a picture decodes automatically when one starts.${where}`,
  }
}

/** "2026-07-17 15:30Z" from the gallery's ISO stamp (raw string if unexpected). */
function fmtUtc(iso: string): string {
  return /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/.test(iso)
    ? `${iso.slice(0, 10)} ${iso.slice(11, 16)}Z`
    : iso
}

/** One completed gallery image: BMP over the asset protocol, with a
 * fetch-and-rasterize canvas fallback if this webview's <img> lacks BMP
 * decode (older WebKitGTK). Outside the shell (tests) → caption-only card. */
function GalleryThumb({ entry }: { entry: SstvGalleryEntry }) {
  const src = assetUrl(entry.path)
  const [fallback, setFallback] = useState(false)
  const canvasRef = useRef<HTMLCanvasElement>(null)
  useEffect(() => {
    if (!fallback || !src) return
    let alive = true
    fetch(src)
      .then((r) => r.arrayBuffer())
      .then((buf) => {
        if (alive) drawBmp(canvasRef.current, buf)
      })
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [fallback, src])
  const alt = `${entry.mode} image received ${fmtUtc(entry.finishedUtc)}`
  if (!src) return null
  return fallback ? (
    <canvas ref={canvasRef} className="sstv-thumb-img" role="img" aria-label={alt} />
  ) : (
    <img
      className="sstv-thumb-img"
      src={src}
      alt={alt}
      loading="lazy"
      onError={() => setFallback(true)}
    />
  )
}

/**
 * SSTV view (Digital rail: FT · Tempo · RTTY · SSTV) — LIVE RX-first: arm the
 * receiver and any VIS header heard auto-decodes; the in-flight image renders
 * on the canvas and finished images land in the gallery (auto-saved BMPs with
 * mode/UTC/frequency metadata). Mounted in a keep-alive host so the armed
 * receiver keeps listening while the operator is on another section.
 * txState=false: nothing here transmits.
 */
export function SstvView({ snap, theme = 'default', onSnap, active = true, onSetFrequency, onSetTxEnabled, wheelSensitivity, txModeDefault, txPowerPct, panels, onOpenSettings }: Props) {
  // Panels (Phase 3): the RX canvas + the TX bar are pinned chrome (never panels); only the
  // Transmit composer and the Gallery are removable (⊞ menu). They render through
  // CockpitPaneFrame with ROLES — the composer is fit="content" (a drop zone cannot use
  // surplus height), the gallery fills — so the old seam/share machinery is meaningless
  // here and was removed with the 50/50 `.sstv-lower` split (census #10).
  const host = panels ? panelHost(panels, { menu: SSTV_PANEL_IDS, side: [], main: 'gallery', labels: SSTV_PANEL_LABELS }) : null
  const shown = (id: SstvPanelId) => (host ? host.shown(id) : true)
  // Live decoder state — polled at 1 Hz while this is the visible view (the
  // backend keeps decoding while hidden; the first tick catches the display up).
  const [sstv, setSstv] = useState<SstvState | null>(null)
  // A poll that cannot be reached is NOT an idle receiver. Swallowing the rejection
  // left `sstv` null, which renders exactly like "not armed, nothing heard" — the UI
  // stating a belief it had no evidence for, on the one screen the operator turns to
  // when nothing is happening.
  const [pollError, setPollError] = useState(false)
  // Ticks the age clock in the status line (the counters are cumulative; their
  // stamps are what make the sentence honest).
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000))
  // Live snapshot ref so the Send handler reads the CURRENT dial/privileges (same
  // pattern as the CW/RTTY cockpits).
  const snapRef = useRef(snap)
  snapRef.current = snap
  useEffect(() => {
    if (!active) return
    let alive = true
    const tick = () => {
      setNow(Math.floor(Date.now() / 1000))
      getSstvState()
        .then((s) => {
          if (alive) {
            setSstv(s)
            setPollError(false)
          }
        })
        .catch(() => {
          if (alive) setPollError(true)
        })
    }
    tick()
    const id = window.setInterval(tick, 1000)
    return () => {
      alive = false
      window.clearInterval(id)
    }
  }, [active])

  // ⭐ START THE RECEIVER ON ENTERING THE VIEW, so SSTV does not open on a screen
  // that will never decode anything. Rising edge of `active`, not mount — this view
  // is kept alive across navigation (App renders it hidden), so it mounts once per
  // session.
  //
  // ⚠️ RX ONLY, and the ENGINE is what guarantees that: `sstv_auto_arm` only ever
  // upgrades from off and refuses once the operator has explicitly stopped the
  // receiver this session. Nothing on the SSTV TX path is reachable from it —
  // `sstv_tx` is written by `sstv_send` alone. The policy lives in the engine rather
  // than in a ref here so a remount cannot lose it.
  const autoArmed = useRef(false)
  useEffect(() => {
    if (!active) {
      autoArmed.current = false
      return
    }
    if (autoArmed.current) return
    autoArmed.current = true
    // Re-READ rather than trusting this call's own return: the 1 Hz poll is running
    // beside it, and the reply to a command issued at entry must never overwrite a
    // fresher snapshot (the same shape as the APRS cockpit's auto-arm).
    void sstvAutoArm()
      .then(() => getSstvState())
      .then(setSstv)
      .catch(() => {})
  }, [active])

  const armed = sstv?.armed === true
  const toggleArm = () => {
    void sstvArm(!armed)
      .then(setSstv)
      .catch(() => pushToast('Could not switch the SSTV receiver', 'error'))
  }

  // Licensed SSTV calling frequencies (built-in band plan — 14.230, the 20 m
  // overflow channels, the ISS 145.800 FM downlink, …), same source as the CW/Phone
  // band pickers. Feeds BOTH the band picker and the idle caption: the caption used
  // to hardcode two frequencies while this list, already in hand, held a dozen.
  const [plan, setPlan] = useState<BandChannel[]>([])
  // RF drive, %, pushed to the rig only once the operator touches it — the same contract
  // as the Phone cockpit's slider. Deliberately not read back from CAT: no rig reports
  // drive reliably, and a wrong read-back here would move the operator's power for them.
  const [txPower, setTxPower] = useState(100)
  // Settings ▸ Digital ▸ SSTV seeds that slider. Tracks whether the operator has since
  // dragged it, because a drag already reached the rig and must not be re-sent at Send.
  //
  // ⚠️ THIS EFFECT MUST NOT CALL setRfPower. Opening a screen may not move the operator's
  // power — notify, never act unattended. It seeds the control only; the value reaches the
  // rig at Send, which is already a rig-commanding act.
  const powerTouched = useRef(false)
  useEffect(() => {
    if (txPowerPct == null) return
    powerTouched.current = false // a Settings change supersedes this session's drag
    setTxPower(Math.min(100, Math.max(0, Math.round(txPowerPct))))
  }, [txPowerPct])
  useEffect(() => {
    void getLicensedBandPlan('sstv').then(setPlan).catch(() => {})
  }, [])

  // Commit a typed dial from the shared header readout; rejects out-of-plan
  // frequencies with a toast (same as the other cockpits).
  const commitDial = (mhz: number) => {
    const band = bandLabelForMhz(mhz)
    if (!band) {
      pushToast(`${mhz.toFixed(4)} MHz is outside the band plan`, 'error', 3000)
      return
    }
    // #45: the CURRENT sideband is right for a retune inside a band and wrong the moment the
    // band changes — picking 20 m from 40 m carried LSB across, so the menu offered USB and the
    // rig went to 14.230 LSB. `sidebandForQsy` corrects only across the 10 MHz boundary.
    onSetFrequency?.(mhz, band, sidebandForQsy(mhz, snap?.radio.dialMhz ?? mhz, snap?.radio.sideband))
  }

  // In-flight preview → canvas at the preview's NATIVE size; CSS upscales it
  // crisp (image-rendering: pixelated — putImageData never smooths, so the only
  // smoothing risk is the CSS scale). Bad/short base64 keeps the last frame.
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const preview = sstv?.previewRgbBase64 ?? null
  const pw = sstv?.previewWidth ?? 0
  const ph = sstv?.previewHeight ?? 0
  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas || !preview || pw <= 0 || ph <= 0) return
    try {
      const raw = atob(preview)
      if (raw.length < pw * ph * 3) return
      const img = new ImageData(pw, ph)
      for (let i = 0, d = 0; i < pw * ph; i++) {
        img.data[d++] = raw.charCodeAt(i * 3)
        img.data[d++] = raw.charCodeAt(i * 3 + 1)
        img.data[d++] = raw.charCodeAt(i * 3 + 2)
        img.data[d++] = 255
      }
      canvas.width = pw
      canvas.height = ph
      canvas.getContext('2d')?.putImageData(img, 0, 0)
    } catch {
      /* undecodable base64 → keep the last frame */
    }
  }, [preview, pw, ph])

  // THE PICTURE UPSCALE (census #9): measure the RX stage and show the decode at the
  // largest INTEGER multiple of its native size that fits — never a fractional scale
  // (integerScaleStep.ts has the ruling). The ResizeObserver is 0×0-guarded the same
  // way as useRegionCols: a hidden keep-alive host or mid-layout pass keeps the last
  // real measurement rather than collapsing the picture to 1×.
  const stageRef = useRef<HTMLElement>(null)
  const [stage, setStage] = useState({ w: 0, h: 0 })
  useLayoutEffect(() => {
    const el = stageRef.current
    if (!el) return
    let raf = 0
    const measure = () => {
      const w = el.clientWidth
      const h = el.clientHeight
      if (w >= 2 && h >= 2) setStage((s) => (s.w === w && s.h === h ? s : { w, h }))
    }
    measure()
    if (typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(() => {
      cancelAnimationFrame(raf)
      raf = requestAnimationFrame(measure)
    })
    ro.observe(el)
    return () => {
      cancelAnimationFrame(raf)
      ro.disconnect()
    }
  }, [])
  // Fixed chrome between the stage's client box and the picture: .sstv-live padding
  // (2×space-3) + canvas border on the width; those plus the caption row and flex gap
  // on the height. Overestimating only costs margin; underestimating would trip the
  // CSS min(100%) yield and reintroduce the fractional blur the ruling forbids.
  // sstvImageWidth returns a whole multiple of pw whenever ≥1× fits; a stage shorter
  // than the native decode gets the fractional width that fits the HEIGHT (the axis
  // the CSS min(100%) yield cannot cover — it clipped top and bottom before).
  const STAGE_CHROME_W = 32
  const STAGE_CHROME_H = 64
  const imgW =
    pw > 0 && ph > 0 && stage.w > 0
      ? sstvImageWidth(pw, ph, stage.w - STAGE_CHROME_W, stage.h - STAGE_CHROME_H)
      : null

  // Honest V1 caption: the two-pass core lands lines nearly all at once at
  // completion, so until they land we say "decoding <mode>…" — never a fake
  // progress count. VIS-detected mode + total show immediately.
  //
  // ⚠️ AND IT SAYS HOW LONG THAT TAKES. `find_sync` needs the whole image buffered
  // before any line can be placed (decoder.rs `FINDSYNC_AUDIO_HEADROOM`), so a
  // Scottie 1 preview is a black rectangle for ~110 s and then the picture appears
  // all at once. Without the airtime beside it that reads as a hang — the operator's
  // words were "not working or decoding as the image comes in".
  const inFlight = sstv?.mode != null
  const txSecs = sstv?.mode ? SSTV_TX_MODES.find((m) => m.name === sstv.mode)?.seconds : undefined
  const caption =
    inFlight && sstv
      ? sstv.linesDone > 0
        ? `${sstv.mode} — ${sstv.linesDone}/${sstv.linesTotal} lines`
        : txSecs != null
          ? `decoding ${sstv.mode}… the picture lands when the transmission ends (≈${fmtClock(txSecs)})`
          : `decoding ${sstv.mode}…`
      : ''

  // What the receiver is hearing while no picture is coming in — the four states the
  // old one-line hint collapsed into one.
  const sstvChannel = sstvChannelForDial(plan, snap?.radio.dialMhz)
  const rx = pollError
    ? {
        state: 'off' as SstvRxState,
        text: 'Cannot read the receiver state — the app is not answering. The decoder may still be running.',
      }
    : sstvDecodeStatus(sstv?.health, now, sstvChannel)

  // ⚠️ SPEAK THE CHANGE, DON'T LIVE-REGION THE SENTENCE. The caption restates the
  // age of the last picture every second, so `role="status"` on it made a screen
  // reader re-announce the same paragraph ~90 times after one image. What is news is
  // the TRANSITION — the receiver going deaf, or a header arriving in a mode we
  // cannot decode — so announce that once, through the same bus the TX path uses.
  // The first reading per entry is skipped: it is the state the operator just walked
  // into, not a change, and the caption is right there.
  const spokenRx = useRef<SstvRxState | null>(null)
  useEffect(() => {
    if (!active) {
      spokenRx.current = null
      return
    }
    // ⚠️ Before the first poll answers there is no reading — and `sstvDecodeStatus`
    // renders that as `off`, which is also a real state. Seeding from it would make
    // the ordinary "not asked yet → hearing audio" settle read as a change and speak
    // on every entry to the view.
    if (sstv == null && !pollError) return
    if (spokenRx.current === rx.state) return
    const first = spokenRx.current === null
    spokenRx.current = rx.state
    if (!first) announce(rx.text)
  }, [active, sstv, pollError, rx.state, rx.text])

  // Gallery arrives oldest-first; show newest first.
  const gallery = sstv?.gallery && sstv.gallery.length > 0 ? [...sstv.gallery].reverse() : []

  // Delete one received image, after asking. The message names the PICTURE rather than saying
  // "are you sure", so the operator can tell which one they are about to lose — the tiles are
  // small and several look alike.
  const deleteImage = async (g: { path: string; mode: string; finishedUtc: string }) => {
    const what = `${g.mode} received ${fmtUtc(g.finishedUtc)}`
    if (!window.confirm(`Delete the ${what}?\n\nThe image file is removed and cannot be recovered.`))
      return
    await withErrorToast(async () => {
      await sstvDeleteImage(g.path)
      setSstv(await getSstvState())
    }, 'Could not delete the image')
  }

  // ---------------------------------------------------------------------------
  // TX: compose an image and transmit it. Nothing here keys the rig until the
  // operator clicks Send — the backend re-checks every gate (Phone, TX-enabled,
  // license privileges, mutual exclusion, watchdog) and refuses with a reason we
  // toast. The SSTV view stays assert-nothing on entry; only Send keys.
  // ---------------------------------------------------------------------------
  const sending = sstv?.sending === true

  // Selected TX mode — band-aware default (VHF/2 m → PD-120 for ARISS; HF →
  // Scottie 1, the NA calling-frequency convention) until the operator picks one.
  const [modeSlug, setModeSlug] = useState('scottie1')
  const userPickedMode = useRef(false)
  const dialMhz = snap?.radio.dialMhz
  useEffect(() => {
    if (userPickedMode.current || dialMhz == null) return
    setModeSlug(dialMhz >= 30 ? 'pd120' : 'scottie1')
  }, [dialMhz])
  // An explicit Settings default outranks the band-aware pick, and arrives LATE — App fetches
  // settings after this keep-alive view has mounted. Keyed on the value, so a later Save wins
  // while a pick made in this cockpit stands (the prop did not change).
  //
  // ⚠️ THE LATCH IS SET FROM THE VALUE, AND THE EFFECT RUNS UNCONDITIONALLY. `userPickedMode`
  // is what holds the band-aware effect above off; early-returning on 'auto' would leave it
  // latched from a previous default, so setting the picker back to Automatic would silently do
  // nothing for the rest of the session. An unknown slug (hand-edited file, downgrade) counts
  // as no default and releases it too.
  useEffect(() => {
    const pinned = Boolean(txModeDefault && txModeDefault !== 'auto' && MODE_BY_SLUG[txModeDefault])
    userPickedMode.current = pinned
    if (pinned) setModeSlug(txModeDefault as string)
  }, [txModeDefault])
  const modeSlugRef = useRef(modeSlug)
  modeSlugRef.current = modeSlug
  const txMode = MODE_BY_SLUG[modeSlug]

  // ⭐ THE OPERATOR'S CALLSIGN, AND IT IS NOT OPTIONAL.
  //
  // Before this, an SSTV over went out with NO STATION IDENTIFICATION OF ANY KIND —
  // no burned-in overlay (this file was `drawImage` only, no `fillText` anywhere), no
  // CW ident (`cw_id_after_73` reaches `pending_cw_id` only through the FT/digital QSO
  // state machine; `Engine::sstv_send` never touched it), and the FSK ID is decode-only
  // with no encoder in the tree. One over is a single continuous PTT hold of up to
  // ~290 s of picture-only audio.
  //
  // §97.119(b)(4) lets the call ride in the picture itself, which is what the plate
  // does. It is unconditional on purpose: an ID with an off switch is an ID that is off
  // when it matters, and a toggle in the removable `txcompose` pane would disappear
  // with the pane. `snap.mycall` is already on AppSnapshot, so no new prop is needed —
  // the settings key is required at SettingsPanel's Station section.
  const callsign = normalizeCall(snap?.mycall ?? '')
  const callsignRef = useRef(callsign)
  callsignRef.current = callsign

  // The operator's chosen picture, ALREADY UPRIGHT (EXIF applied at load, before
  // anything measures it — orientations 5–8 change the aspect ratio, so a crop computed
  // before the rotation is a crop of a picture that does not exist). `packed` holds the
  // base64 RGB actually sent, and it is read straight back off the preview canvas, so
  // what the operator sees is what goes out — ID plate included.
  const srcRef = useRef<{ img: CanvasImageSource; w: number; h: number } | null>(null)
  const txCanvasRef = useRef<HTMLCanvasElement>(null)
  const [imageName, setImageName] = useState<string | null>(null)
  const [srcSize, setSrcSize] = useState<{ w: number; h: number } | null>(null)
  const [notice, setNotice] = useState<{ kind: 'error' | 'warn'; text: string } | null>(null)
  const [packed, setPacked] = useState<{
    slug: string
    width: number
    height: number
    b64: string
  } | null>(null)

  // Where the crop is centred in the source, NORMALISED — so changing mode preserves
  // the operator's framing intent across the aspect change instead of snapping back to
  // centre. Held in a ref, not state: the drag re-renders the canvas directly and a
  // state round-trip per pointermove would be a re-render per frame for nothing.
  const centreRef = useRef<CropCentre>(CENTRE)
  // Two ping-pong scratch canvases for the halving chain — allocated once, alternated so
  // a step never resizes the canvas the previous step is being read from.
  const scratchRef = useRef<HTMLCanvasElement[]>([])
  const scratch = (i: number) => {
    const s = scratchRef.current
    if (!s[i]) s[i] = document.createElement('canvas')
    return s[i]
  }

  /**
   * Draw the picture the operator will transmit, and optionally pack it.
   *
   * THE ORDER IS THE WHOLE DESIGN: orient (done at load) → crop → resample → burn in
   * the ID → pack. The ID goes on last, in DESTINATION-raster coordinates, which is
   * what makes it both un-croppable (the drag moves the source behind a fixed frame,
   * and this writes after that) and legible (a callsign drawn at 4032 px and then
   * downscaled 12× becomes a sub-pixel grey smear — present in the bitmap, and not an
   * identification).
   *
   * `pack` is false during a drag: `getImageData` + base64 is 1.4 MB of string building
   * for PD-290 and has no business running per pointermove.
   */
  const renderTx = (slug: string, pack: boolean) => {
    const src = srcRef.current
    const canvas = txCanvasRef.current
    const m = MODE_BY_SLUG[slug]
    if (!src || !canvas || !m || src.w <= 0 || src.h <= 0) return
    canvas.width = m.width
    canvas.height = m.height
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    const win = cropWindow(src.w, src.h, m.width, m.height, centreRef.current)
    const steps = halvingChain(win.sw, win.sh, m.width, m.height)
    // Fold the crop into the FIRST step's source rect: scaling anything outside the
    // crop would average in pixels that were going to be thrown away.
    let cur: CanvasImageSource = src.img
    let [sx, sy, sw, sh] = [win.sx, win.sy, win.sw, win.sh]
    for (let i = 0; i < steps.length - 1; i++) {
      const st = steps[i]
      const cv = scratch(i % 2)
      cv.width = st.w
      cv.height = st.h
      const sctx = cv.getContext('2d')
      if (!sctx) break
      // ⚠️ LOAD-BEARING. With smoothing off, "bilinear" is nearest-neighbour and the
      // halving chain becomes 4:1 decimation — strictly worse than one naive step.
      sctx.imageSmoothingEnabled = true
      sctx.drawImage(cur, sx, sy, sw, sh, 0, 0, st.w, st.h)
      cur = cv
      ;[sx, sy, sw, sh] = [0, 0, st.w, st.h]
    }
    ctx.imageSmoothingEnabled = true
    ctx.clearRect(0, 0, m.width, m.height)
    ctx.drawImage(cur, sx, sy, sw, sh, 0, 0, m.width, m.height)
    // ⭐ THE STATION ID, after everything that could shrink or crop it away — unless the
    // operator has affirmed this picture already carries it (#50): then drawing ours would
    // cover the artwork that identifies them, to repeat what the image already says.
    if (!idInImageRef.current) {
      drawIdPlate(ctx, m.width, m.height, callsignRef.current)
    }

    if (!pack) return
    try {
      const data = ctx.getImageData(0, 0, m.width, m.height).data
      setPacked({ slug, width: m.width, height: m.height, b64: rgbToBase64(data, m.width * m.height) })
    } catch {
      pushToast('Could not read the image pixels', 'error')
    }
  }

  // rAF-coalesced redraw for the drag: at most one render per frame, no packing.
  const rafRef = useRef(0)
  const scheduleRender = () => {
    if (rafRef.current) return
    rafRef.current = requestAnimationFrame(() => {
      rafRef.current = 0
      renderTx(modeSlugRef.current, false)
    })
  }
  useEffect(
    () => () => {
      if (rafRef.current) cancelAnimationFrame(rafRef.current)
    },
    [],
  )

  // Re-run whenever the mode changes and a picture is loaded — the preview and the
  // packed RGB must always match the dimensions the backend validates. Deliberately
  // from the ORIENTED FULL-RESOLUTION source, never from the previous crop: re-cropping
  // a crop compounds resample loss. The centre is kept and re-clamped for the new
  // aspect, which is why 320×256 → 800×616 (same 1.30 ratio) is a free resolution
  // change with identical framing.
  //
  // The callsign is a dependency too: an operator who fills in Settings → Station after
  // loading a picture must not be left holding a packed frame with no ident in it.
  useEffect(() => {
    if (srcRef.current) renderTx(modeSlug, true)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [modeSlug, callsign])

  /** Decode a file to something drawable. Asks for `imageOrientation: 'none'` first so
   *  the EXIF matrix is ours to apply; falls back through plain `createImageBitmap` and
   *  then an `<img>`, because the app runs on both WebKitGTK and WebView2 and neither
   *  the option's availability nor the default is something to bet the picture on.
   *  `effectiveOrientation` then MEASURES which of them happened. */
  const decodeImage = (file: File): Promise<{ img: CanvasImageSource; w: number; h: number } | null> => {
    const cib = (globalThis as { createImageBitmap?: (b: Blob, o?: unknown) => Promise<ImageBitmap> })
      .createImageBitmap
    const viaImg = () =>
      new Promise<{ img: CanvasImageSource; w: number; h: number } | null>((resolve) => {
        const url = URL.createObjectURL(file)
        const img = new Image()
        img.onload = () => {
          URL.revokeObjectURL(url)
          resolve({ img, w: img.naturalWidth || img.width, h: img.naturalHeight || img.height })
        }
        img.onerror = () => {
          URL.revokeObjectURL(url)
          resolve(null)
        }
        img.src = url
      })
    if (!cib) return viaImg()
    return cib(file, { imageOrientation: 'none' })
      .catch(() => cib(file))
      .then((bmp) => (bmp && bmp.width > 0 ? { img: bmp, w: bmp.width, h: bmp.height } : null))
      .catch(() => viaImg())
  }

  /** Rotate/mirror a decoded picture upright onto its own canvas, so everything
   *  downstream works in upright coordinates. Only reached for orientations 2–8 that
   *  the decoder did not already apply, so the common path allocates nothing. */
  const orientOnto = (
    d: { img: CanvasImageSource; w: number; h: number },
    orientation: number,
  ): { img: CanvasImageSource; w: number; h: number } => {
    const t = orientTransform(orientation, d.w, d.h)
    const cv = document.createElement('canvas')
    cv.width = t.w
    cv.height = t.h
    const ctx = cv.getContext('2d')
    if (!ctx) return d
    ctx.setTransform(t.m[0], t.m[1], t.m[2], t.m[3], t.m[4], t.m[5])
    ctx.drawImage(d.img, 0, 0)
    return { img: cv, w: t.w, h: t.h }
  }

  // #50: the operator's per-picture affirmation that the callsign is already IN the image (a
  // pre-made QSO card), so the plate should not be drawn over it. Deliberately per-image —
  // reset every load, never persisted: a sticky opt-out would quietly carry one picture's
  // affirmation onto the next, which is exactly how an unidentified picture goes on the air.
  const [idInImage, setIdInImage] = useState(false)
  const idInImageRef = useRef(false)
  idInImageRef.current = idInImage

  const loadImage = async (file: File) => {
    setIdInImage(false)
    setNotice(null)
    // Header only, through Blob.slice — a 100 MB file must not land in memory just to
    // be refused for being 100 MB.
    let head = new Uint8Array(0)
    try {
      head = new Uint8Array(await file.slice(0, HEADER_BYTES).arrayBuffer())
    } catch {
      /* no Blob.slice/arrayBuffer here — fall through and let the decoder answer */
    }
    const kind = sniffImageKind(head)
    const refusal = refusalFor(kind)
    if (refusal) {
      // Keep the previous picture and its crop loaded: a refusal must not also cost the
      // operator the framing they had already set up.
      setNotice({ kind: 'error', text: refusal })
      return
    }
    const decoded = await decodeImage(file)
    if (!decoded || decoded.w <= 0 || decoded.h <= 0) {
      setNotice({
        kind: 'error',
        text:
          kind === 'unknown' && head.length >= 4
            ? `That file isn't an image Nexus can read (it starts with ${magicHex(head)}).`
            : "That image is damaged and only decoded partly — Nexus won't transmit half a picture. Try re-exporting it.",
      })
      return
    }
    const mp = (decoded.w * decoded.h) / 1e6
    if (mp > MAX_MEGAPIXELS) {
      setNotice({
        kind: 'error',
        text:
          `That's a ${decoded.w}×${decoded.h} image (${mp.toFixed(0)} megapixels) — too large to ` +
          'work with. Export a smaller copy; anything over about 4000 px wide is already far ' +
          'more than SSTV can send.',
      })
      return
    }

    // ⭐ ORIENTATION FIRST, and only when the decode did not already do it.
    const tag = readExifOrientation(head)
    const eff = effectiveOrientation(tag, readIntrinsicSize(head), { w: decoded.w, h: decoded.h })
    srcRef.current = eff === 1 ? decoded : orientOnto(decoded, eff)
    centreRef.current = CENTRE
    setSrcSize({ w: srcRef.current.w, h: srcRef.current.h })
    setImageName(file.name)
    if (kind === 'gif') {
      setNotice({ kind: 'warn', text: 'Sending the first frame — SSTV transmits one still picture.' })
    }
    renderTx(modeSlugRef.current, true)
  }

  // ---------------------------------------------------------------------------
  // DRAG TO ADJUST. The destination frame is fixed and the SOURCE moves behind it —
  // the preview IS the output raster, so the frame can never move or resize. Pointer
  // events rather than HTML5 drag: the app runs `dragDropEnabled: false` (0.15.1, so
  // WebView2's OS handler stops eating HTML5 drag events), the drop zone's HTML5
  // handlers are the one thing that config exists to keep alive, and a second HTML5
  // drag inside it would fight them for the same events. Pointer events also give
  // touch and pen for free.
  // ---------------------------------------------------------------------------
  const dragRef = useRef<{ id: number; x: number; y: number; scale: number } | null>(null)
  const axis = srcSize && txMode ? freeAxis(srcSize.w, srcSize.h, txMode.width, txMode.height) : 'none'

  const onPreviewPointerDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (!srcRef.current || !txMode || axis === 'none') return
    const el = e.currentTarget
    const rect = el.getBoundingClientRect()
    // Measured, not assumed: the preview is CSS-scaled, so a drag in CSS pixels has to
    // be divided by that scale or it moves the picture by the wrong amount. This is the
    // classic "the drag feels three times too fast".
    const scale = el.width > 0 && rect.width > 0 ? rect.width / el.width : 1
    el.setPointerCapture?.(e.pointerId)
    dragRef.current = { id: e.pointerId, x: e.clientX, y: e.clientY, scale }
  }

  const onPreviewPointerMove = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const d = dragRef.current
    const src = srcRef.current
    if (!d || d.id !== e.pointerId || !src || !txMode) return
    centreRef.current = dragCentre(
      src.w,
      src.h,
      txMode.width,
      txMode.height,
      centreRef.current,
      e.clientX - d.x,
      e.clientY - d.y,
      d.scale,
    )
    d.x = e.clientX
    d.y = e.clientY
    scheduleRender()
  }

  const endDrag = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (!dragRef.current || dragRef.current.id !== e.pointerId) return
    dragRef.current = null
    e.currentTarget.releasePointerCapture?.(e.pointerId)
    // Pack once, at the end — this is where the expensive read happens.
    renderTx(modeSlugRef.current, true)
  }

  /** Arrows nudge one target-raster pixel, shift ten. Non-negotiable: this app's
   *  accessibility stance is always-on, and a pointer-only crop is unusable with a
   *  screen reader or without a mouse. */
  const onPreviewKeyDown = (e: React.KeyboardEvent<HTMLCanvasElement>) => {
    const src = srcRef.current
    if (!src || !txMode) return
    const step = e.shiftKey ? 10 : 1
    const d: Record<string, [number, number]> = {
      ArrowLeft: [step, 0],
      ArrowRight: [-step, 0],
      ArrowUp: [0, step],
      ArrowDown: [0, -step],
    }
    if (e.key === 'Home') {
      e.preventDefault()
      centreRef.current = CENTRE
      renderTx(modeSlugRef.current, true)
      return
    }
    const mv = d[e.key]
    if (!mv) return
    e.preventDefault()
    centreRef.current = nudgeCentre(src.w, src.h, txMode.width, txMode.height, centreRef.current, mv[0], mv[1])
    renderTx(modeSlugRef.current, true)
  }

  /** Double-click re-centres — the one-gesture way back to the default framing. */
  const recentre = () => {
    if (!srcRef.current) return
    centreRef.current = CENTRE
    renderTx(modeSlugRef.current, true)
  }

  const changeMode = (slug: string) => {
    userPickedMode.current = true
    setModeSlug(slug)
  }

  const sendImage = () => {
    if (!packed || sending) return
    // ⚠️ NO CALLSIGN, NO TRANSMISSION. The picture IS the identification here, so an
    // empty call is not a cosmetic gap. The backend refuses the same way and is the
    // authority — this is the half that says so before the operator clicks.
    if (!callsign) {
      // The refusal carries its own fix: this fires on the Send click, so a path to read and
      // navigate by hand is a worse answer than the button the toast bus already supports.
      pushToast(
        'Set your callsign — SSTV identifies by burning it into the picture',
        'error',
        6000,
        onOpenSettings
          ? { action: () => onOpenSettings('operator-radio'), actionLabel: 'Set callsign' }
          : {},
      )
      return
    }
    const m = MODE_BY_SLUG[packed.slug]
    // Soft ISS guard: 145.800 is the ISS SSTV DOWNLINK — transmit there only for a
    // sanctioned ARISS uplink event, never by accident.
    const dial = snapRef.current?.radio.dialMhz
    if (dial != null && Math.abs(dial - 145.8) <= 0.01) {
      const ok = window.confirm(
        '145.800 MHz is the ISS SSTV downlink. Transmit only during a sanctioned ARISS uplink event. Send anyway?',
      )
      if (!ok) return
    }
    void withErrorToast(async () => {
      // Human-initiated: force Phone (USB/LSB) so SSTV rides the phone segment,
      // WITHOUT a QSY (followFreq=false). Then hand the packed image to the gated
      // backend — nothing keys until the radio loop takes it behind every gate.
      const s1 = await setOperatingMode('phone', false)
      onSnap?.(s1)
      // The operator's SSTV drive, when they have set one. AFTER the mode switch, because
      // set_operating_mode re-applies that mode's power ceiling; before the send, because this
      // is the level the picture goes out at. `set_rf_power` clamps to the Phone ceiling, so
      // this can only lower power past the cap, never raise it past one. Skipped when the
      // slider was moved this session — that value already reached the rig on the move — and
      // skipped entirely when the setting is blank, which is the shipped behaviour unchanged.
      // A power command that fails must not block a send.
      if (!powerTouched.current && txPowerPct != null) {
        await setRfPower(Math.min(100, Math.max(0, Math.round(txPowerPct))) / 100).catch(() => {})
      }
      return sstvSend(packed.b64, packed.width, packed.height, packed.slug, idInImageRef.current)
    }, 'SSTV send refused').then((s) => {
      if (s) {
        setSstv(s)
        if (s.sending) announce(`Transmitting SSTV ${m?.name ?? packed.slug}`, { assertive: true })
      }
    })
  }

  const stopTx = () => {
    void sstvStop()
      .then((s) => {
        setSstv(s)
        announce('SSTV transmit stopped', { assertive: true })
      })
      .catch(() => {})
  }

  // Announce natural completion (sending true → false without an explicit Stop).
  const wasSending = useRef(false)
  useEffect(() => {
    if (wasSending.current && !sending) announce('SSTV transmit finished')
    wasSending.current = sending
  }, [sending])

  const txProgressPct = Math.round((sstv?.txProgress ?? 0) * 100)
  const txRemaining = Math.max(0, (sstv?.txTotalSecs ?? 0) - (sstv?.txElapsedSecs ?? 0))
  const txStatus = `TX — ${sstv?.txMode ?? txMode?.name ?? 'SSTV'} · ${fmtClock(txRemaining)} remaining`

  return (
    <main className="layout single sstv-view">
      {snap && (
        <CockpitHeader
          snap={snap}
          onSnap={onSnap}
          onSetTxEnabled={onSetTxEnabled}
          // THE CORE RADIO CONTROLS. SSTV shipped with only the TX-enable latch, so an
          // operator had no drive control, no Tune to set it against, and no Stop TX in the
          // place every other cockpit puts one (operator, 2026-08-04). Drive matters more
          // here than anywhere: SSTV carries the picture in the AUDIO, so overdriving past
          // ALC does not just splatter, it visibly wrecks the image at the far end.
          // The view's own pinned Stop stays — it is the TX-locked one the stop-line census
          // pins (stop-line.test.tsx) — this adds the familiar header one beside it.
          power={{
            value: txPower,
            unit: '%',
            onChange: (pct: number) => {
              // The drag IS the operator's latest word on drive, and it has already reached
              // the rig — Send must not overwrite it with the Settings default.
              powerTouched.current = true
              setTxPower(pct)
              void setRfPower(pct / 100)
            },
            label: 'Power',
            title: 'RF output power — set it against a Tune carrier, below ALC',
          }}
          onTune={(on) => void setTune(on).then((st) => onSnap?.(st))}
          onStopTx={() => void haltTx()}
          modeIndicator={
            <span
              className="cw-mode-badge"
              title="Detected SSTV mode — fills in (Martin / Scottie / Robot / PD) when the receiver hears a VIS header"
            >
              {sending && sstv?.txMode
                ? `SSTV · TX ${sstv.txMode}`
                : inFlight && sstv?.mode
                  ? `SSTV · ${sstv.mode}`
                  : 'SSTV'}
            </span>
          }
          bandControl={
            onSetFrequency ? (
              <FrequencyControl
                channels={plan}
                dialMhz={snap.radio.dialMhz}
                band={snap.radio.band}
                mode={snap.radio.sideband}
                variant="compact"
                showReadout={false}
                showModeToggle={false}
                onSet={onSetFrequency}
              />
            ) : (
              <span
                className="cockpit-ph-pill"
                title="Showing the rig's current band — SSTV decodes wherever you're tuned"
              >
                {bandLabelForMhz(snap.radio.dialMhz) || '— band —'}
              </span>
            )
          }
          onCommitDial={onSetFrequency ? commitDial : undefined}
          digitTune={onSetFrequency != null}
          wheelSensitivity={wheelSensitivity}
          actions={
            host && panels ? (
              <PanelsMenu
                items={host.menuItems}
                onToggle={(id, show) => panels.setPanelState(id as SstvPanelId, show ? 'docked' : 'removed')}
                onUndo={panels.undo}
                canUndo={panels.canUndo}
                onReset={panels.reset}
              />
            ) : undefined
          }
        >
          <label
            className="cw-wpm"
            title="Slant trim — fine sample-clock correction. Auto-corrected by the decoder; the manual trim comes in a later build."
          >
            <span>Slant</span>
            <input
              type="range"
              min={-50}
              max={50}
              defaultValue={0}
              disabled
              aria-label="SSTV slant trim (disabled — decoder not wired yet)"
            />
          </label>
          <button
            type="button"
            className={`sstv-arm${armed ? ' on' : ''}`}
            aria-pressed={armed}
            onClick={toggleArm}
            title={
              armed
                ? 'Armed — any VIS header heard auto-decodes and auto-saves to the gallery (RX only). Click to disarm.'
                : 'Arm — auto-decode any VIS header heard on the receive audio (RX only, never transmits)'
            }
          >
            {armed ? 'Armed' : 'Arm'}
          </button>
        </CockpitHeader>
      )}

      <section className="sstv-canvas" aria-label="SSTV image" ref={stageRef}>
        {inFlight ? (
          <div className="sstv-live">
            {preview && (
              <canvas
                ref={canvasRef}
                className="sstv-live-canvas"
                // Integer-step width (never fractional — census #9) except the one
                // sanctioned yield: a stage shorter than the native decode gets the
                // width that fits its height (sstvImageWidth), and the CSS min(100%)
                // still yields on the width axis. Unset until the first real
                // measurement: the sheet's 480px (3×) default holds.
                style={
                  imgW != null
                    ? ({ '--sstv-img-w': `${imgW}px` } as React.CSSProperties)
                    : undefined
                }
              />
            )}
            <div className="sstv-live-caption" role="status">
              {caption}
              {/* The one thing the spectrum would have shown that the picture
                  cannot: whether the radio is on frequency. Free — the decoder
                  already derives it from the VIS leader and used it only for
                  diagnostics. Hidden below ±10 Hz, which is not worth a glance. */}
              {Math.abs(sstv?.hedrShiftHz ?? 0) >= 10 && (
                <span className="sstv-tuneoff">
                  {' · tuning '}
                  {(sstv?.hedrShiftHz ?? 0) > 0 ? '+' : ''}
                  {Math.round(sstv?.hedrShiftHz ?? 0)} Hz
                </span>
              )}
            </div>
          </div>
        ) : (
          // ⭐ THE BAND, WHEN NOTHING IS DECODING. The operator had no way to see
          // what was on the frequency: "I've got no waterfall. It'd be nice to have
          // a waterfall ... to understand what's on the band right now."
          //
          // The SAME REGION becomes the picture once a VIS lands (the branch
          // above), which is the operator's other ask — "as the band is coming in
          // with the signal, actually show the image in the band as it decodes" —
          // and they chose the picture REPLACING the band over an overlay or a
          // split. One place: it is the band until there is an image, then it is
          // the image.
          <div className="sstv-band">
            <Waterfall
              theme={theme}
              active={active}
              rowMs={50} // live band instrument — rig-scope cadence, not the FT slot default
              transmitting={snap?.radio.transmitting ?? false}
              rxOffsetHz={0}
              txOffsetHz={0}
              hint="the band — a picture takes this space when one arrives"
            />
            {/* The guidance stays on screen. A waterfall shows you the band but
                does not tell you WHERE to point the radio, and this view is often
                the first place a new operator lands.

                ⭐ AND IT SAYS WHETHER THE APP IS HEARING ANYTHING. This line used to
                be one of two fixed strings, so "the receiver was never started",
                "the capture device is dead", "the input is silent" and "that mode
                cannot be decoded" all looked the same — which is how an operator
                with a strong signal on the waterfall spent two sessions unable to
                tell what was wrong. `sstvDecodeStatus` has the ladder.

                ⚠️ NOT a live region — it rewrites the age of the last picture once a
                second, and aria-live would read the whole paragraph out again every
                time. State CHANGES are announced instead (see `spokenRx` above). */}
            <div className={`sstv-band-caption rx-${rx.state}`}>
              {rx.text}
              {/* The two audio faults are the ones with a fix in Settings, and the operator is
                  looking straight at this line when they hit one — so the trip to the device
                  pickers is a click from here rather than a path to go find. */}
              {onOpenSettings && (rx.state === 'nocapture' || rx.state === 'silent') && (
                <>
                  {' '}
                  <button
                    type="button"
                    className="settings-linkbtn"
                    onClick={() => onOpenSettings('audio')}
                  >
                    Open audio settings
                  </button>
                </>
              )}
            </div>
          </div>
        )}
      </section>

      {/* THE LOWER PANES — CockpitPaneFrame with ROLES, and deliberately NO
          .cockpit-panes region (RTTY's precedent: with this few content blocks every
          multi-column tier leaves a track empty — manufactured dead space). The old
          `.sstv-lower` wrapper split the region 50/50 whatever the panes held (census
          #10: ~440px dead per pane on an ultrawide with an empty gallery); now the
          composer is a content-height strip and the gallery is the fill grower beside
          the RX stage, with deficit flowing to the shell's own overflow-y:auto valve. */}
      {shown('txcompose') && (
        <CockpitPaneFrame title="Transmit" paneId="txcompose" fit="content">
          {/* Unnamed section: the frame above is the landmark ("Transmit"); the wrapper
              stays for its column layout, with `.pane-body > .panel` stripping the
              card-in-card chrome. */}
          <section className="sstv-compose panel">
            <div
              className={`sstv-tx-drop${packed ? ' loaded' : ''}`}
              onDragOver={(e) => e.preventDefault()}
              onDrop={(e) => {
                e.preventDefault()
                const f = e.dataTransfer.files?.[0]
                if (f) void loadImage(f)
              }}
            >
              {/* THE STAGE. This canvas IS what gets transmitted — `renderTx` reads it
                  straight back with getImageData — so the preview is WYSIWYG down to the
                  pixel, ID plate included. It is also the drag surface: the frame never
                  moves, the source moves behind it. */}
              <canvas
                ref={txCanvasRef}
                className={`sstv-tx-preview${packed ? '' : ' empty'} drag-${axis}`}
                role="img"
                tabIndex={packed ? 0 : -1}
                aria-label={
                  packed
                    ? `Transmit preview, ${packed.width}×${packed.height}${
                        axis === 'none'
                          ? ' — the picture already fits, no crop needed'
                          : `. Drag or use the arrow keys to choose which part of the picture is sent (${
                              axis === 'x' ? 'left and right' : 'up and down'
                            }); Home re-centres.`
                      }`
                    : 'No image chosen'
                }
                onPointerDown={onPreviewPointerDown}
                onPointerMove={onPreviewPointerMove}
                onPointerUp={endDrag}
                onPointerCancel={endDrag}
                onDoubleClick={recentre}
                onKeyDown={onPreviewKeyDown}
              />
              {!packed && (
                <div className="sstv-tx-drop-hint">
                  Drop an image here, or choose one below — any size, resized to the mode
                  for you.
                </div>
              )}
            </div>
            <label className="sstv-tx-file">
              <span>{imageName ? 'Change image…' : 'Choose image…'}</span>
              <input
                type="file"
                accept={TX_ACCEPT}
                onChange={(e) => {
                  const f = e.target.files?.[0]
                  if (f) void loadImage(f)
                  e.target.value = ''
                }}
              />
            </label>
            {/* What actually goes out: the source size, the raster it was resized to,
                the mode, and — the number that matters most before a Send — how long the
                rig will be keyed. A 290 s PTT hold is an expensive place to discover a
                bad crop. */}
            {imageName && txMode && (
              <span className="sstv-tx-name" title={imageName}>
                {imageName}
                {srcSize ? ` (${srcSize.w}×${srcSize.h})` : ''} → {txMode.width}×{txMode.height} ·{' '}
                {txMode.name} · {fmtClock(txMode.seconds)} key-down
              </span>
            )}
            {/* ⭐ THE IDENT, SAID OUT LOUD — where it is, or whose word says it is already
                in the picture (#50), or that Send is going to refuse and why. */}
            {packed && (
              <span className={`sstv-tx-id${callsign ? '' : ' missing'}`}>
                {callsign ? (
                  idInImage ? (
                    `${callsign} — you've said it's already in the picture`
                  ) : (
                    `${callsign} burned in · top left`
                  )
                ) : (
                  // Send refuses without a callsign, so this line is a stop sign — it carries
                  // the way to clear it rather than the name of a screen to go looking for.
                  <>
                    No callsign set — SSTV identifies by burning your call into the picture, so it
                    will not transmit without one.{' '}
                    {onOpenSettings ? (
                      <button
                        type="button"
                        className="settings-linkbtn"
                        onClick={() => onOpenSettings('operator-radio')}
                      >
                        Set your callsign
                      </button>
                    ) : (
                      'Set one in Settings ▸ Station.'
                    )}
                  </>
                )}
              </span>
            )}
            {/* #50 (akhepcat): a pre-made QSO card already shows the call, and the plate
                would cover artwork to repeat it. Per-image on purpose — it RESETS when a
                new image loads, because an affirmation is about one picture, and a sticky
                toggle would quietly carry it onto the next image, which may not identify
                anyone. The identification is the operator's responsibility either way;
                this checkbox is them telling us where it lives. */}
            {packed && callsign && (
              <label className="sstv-tx-idopt" title="Skip the burned-in callsign for this picture only — use when the image already shows your call, e.g. a pre-made QSO card">
                <input
                  type="checkbox"
                  checked={idInImage}
                  onChange={(e) => {
                    setIdInImage(e.target.checked)
                    // The preview must show what will actually transmit, immediately.
                    idInImageRef.current = e.target.checked
                    renderTx(modeSlugRef.current, true)
                  }}
                />
                My picture already shows my callsign
              </label>
            )}
            {/* Warnings that must survive to Send time, so a badge in the composer
                rather than a toast that has already gone. */}
            {srcSize && txMode && isUpscale(srcSize.w, srcSize.h, txMode.width, txMode.height) && (
              <span className="sstv-tx-notice warn">
                That picture is {srcSize.w}×{srcSize.h}, smaller than {txMode.name}'s{' '}
                {txMode.width}×{txMode.height} — it will be enlarged and look soft.
              </span>
            )}
            {srcSize && txMode && isExactFit(srcSize.w, srcSize.h, txMode.width, txMode.height) && (
              <span className="sstv-tx-notice">
                Already {txMode.width}×{txMode.height} — sent pixel for pixel, no crop needed.
              </span>
            )}
            {notice && (
              <span className={`sstv-tx-notice ${notice.kind}`} role="status">
                {notice.text}
              </span>
            )}
          </section>
        </CockpitPaneFrame>
      )}

      {shown('gallery') && (
        <CockpitPaneFrame title="Gallery" paneId="gallery">
          <div className="sstv-gallery-grid">
            {gallery.length === 0 ? (
              <div className="sstv-gallery-empty">
                Received images collect here — auto-saved with callsign (FSK ID), mode, frequency,
                and time.
              </div>
            ) : (
              gallery.map((g) => (
                <figure key={g.path} className="sstv-thumb" title={g.path}>
                  <GalleryThumb entry={g} />
                  {/* Irreversible — a received picture is the only copy of something somebody
                      sent you, and there is no re-download. Confirmed before it happens, but a
                      plain confirm rather than the Logbook's type-the-word dialog: one image is
                      not a whole logbook, and ceremony out of proportion to the act just teaches
                      people to click through it. */}
                  <button
                    type="button"
                    className="sstv-thumb-del"
                    aria-label={`Delete the ${g.mode} image received ${fmtUtc(g.finishedUtc)}`}
                    title="Delete this image"
                    onClick={() => void deleteImage(g)}
                  >
                    ✕
                  </button>
                  <figcaption className="sstv-thumb-caption">
                    <span className="sstv-thumb-mode">{g.mode}</span>
                    {g.fskId && <span className="sstv-thumb-call">{g.fskId}</span>}
                    <span className="sstv-thumb-meta">
                      {fmtUtc(g.finishedUtc)} · {g.freqMhz.toFixed(3)} MHz
                    </span>
                  </figcaption>
                </figure>
              ))
            )}
          </div>
        </CockpitPaneFrame>
      )}

      {/* Pinned TX dock — transmit mode + Send + Stop + progress. TX-LOCKED: never a
          removable panel, so Stop is ALWAYS reachable (paramount SSTV TX-safety). LAST
          shell child on purpose (the .cockpit-txdock discipline): the bar is sticky at
          the scrollport bottom, so when the shell's deficit valve scrolls, Send/Stop
          stay parked — mid-column it scrolled off the TOP on the way to the gallery. */}
      <div className="sstv-tx-bar" aria-label="SSTV transmit controls">
        <label className="sstv-tx-mode">
          <span>Mode</span>
          <select
            value={modeSlug}
            onChange={(e) => changeMode(e.target.value)}
            aria-label="SSTV transmit mode"
            title="Transmit mode. VHF/2 m images use PD-120 (ARISS); HF uses Scottie 1 (NA) or Martin 1 (EU)."
          >
            {TX_MODE_GROUPS.map((g) => (
              <optgroup key={g} label={g}>
                {SSTV_TX_MODES.filter((m) => m.group === g).map((m) => (
                  <option key={m.slug} value={m.slug}>
                    {m.name} · ≈{m.seconds}s · {m.width}×{m.height}
                  </option>
                ))}
              </optgroup>
            ))}
          </select>
        </label>
        <div className="sstv-tx-actions">
          <button
            type="button"
            className="sstv-tx-send"
            onClick={sendImage}
            disabled={!packed || sending || !callsign}
            title={
              !packed
                ? 'Choose an image to transmit first'
                : !callsign
                  ? 'Set your callsign in Settings → Station — SSTV identifies by burning it into the picture, and will not transmit without one'
                  : `Transmit this image with ${callsign} burned in — switches to Phone (USB/LSB) and keys the rig`
            }
          >
            Send
          </button>
          <button
            type="button"
            className="sstv-tx-stop"
            onClick={stopTx}
            disabled={!sending}
            title="Stop the transmission in progress and unkey"
          >
            Stop
          </button>
        </div>
        {sending && (
          <div
            className="sstv-tx-progress"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={txProgressPct}
            aria-label={txStatus}
          >
            <div className="sstv-tx-progress-status" role="status">
              {txStatus}
            </div>
            <div className="sstv-tx-progress-track">
              <div className="sstv-tx-progress-fill" style={{ width: `${txProgressPct}%` }} />
            </div>
          </div>
        )}
      </div>
    </main>
  )
}
