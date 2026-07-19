import { useEffect, useRef, useState } from 'react'
import type { AppSnapshot, BandChannel, SstvGalleryEntry, SstvState } from '../types'
import { CockpitHeader } from './CockpitHeader'
import { FrequencyControl } from './FrequencyControl'
import {
  getLicensedBandPlan,
  getSstvState,
  setOperatingMode,
  sstvArm,
  sstvSend,
  sstvStop,
} from '../api'
import { bandLabelForMhz } from '../band'
import { announce } from '../announce'
import { pushToast, withErrorToast } from '../toast'

/** One transmittable SSTV mode: the backend `parse_sstv_mode` slug, its display
 * name, its exact pixel dimensions (the webview cover-crops to these; the backend
 * refuses any mismatch), and an approximate on-air key-down time for the picker
 * label. Dimensions mirror `crates/tempo-sstv/src/modespec.rs` (`ModeSpec`). */
interface TxMode {
  slug: string
  name: string
  group: 'Scottie' | 'Martin' | 'Robot' | 'PD'
  width: number
  height: number
  seconds: number
}

/** The 15 modes, grouped by family. Scottie/Martin/PD are 320×256 unless noted;
 * PD-120/180/240 are 640×496, PD-160 is 512×400, PD-290 is 800×616; Robot is
 * 320×240. Seconds are approximate (the backend's `txTotalSecs` drives progress). */
const SSTV_TX_MODES: TxMode[] = [
  { slug: 'scottie1', name: 'Scottie 1', group: 'Scottie', width: 320, height: 256, seconds: 110 },
  { slug: 'scottie2', name: 'Scottie 2', group: 'Scottie', width: 320, height: 256, seconds: 71 },
  { slug: 'scottiedx', name: 'Scottie DX', group: 'Scottie', width: 320, height: 256, seconds: 269 },
  { slug: 'martin1', name: 'Martin 1', group: 'Martin', width: 320, height: 256, seconds: 114 },
  { slug: 'martin2', name: 'Martin 2', group: 'Martin', width: 320, height: 256, seconds: 58 },
  { slug: 'robot24', name: 'Robot 24', group: 'Robot', width: 320, height: 240, seconds: 36 },
  { slug: 'robot36', name: 'Robot 36', group: 'Robot', width: 320, height: 240, seconds: 36 },
  { slug: 'robot72', name: 'Robot 72', group: 'Robot', width: 320, height: 240, seconds: 72 },
  { slug: 'pd50', name: 'PD-50', group: 'PD', width: 320, height: 256, seconds: 50 },
  { slug: 'pd90', name: 'PD-90', group: 'PD', width: 320, height: 256, seconds: 90 },
  { slug: 'pd120', name: 'PD-120', group: 'PD', width: 640, height: 496, seconds: 126 },
  { slug: 'pd160', name: 'PD-160', group: 'PD', width: 512, height: 400, seconds: 161 },
  { slug: 'pd180', name: 'PD-180', group: 'PD', width: 640, height: 496, seconds: 187 },
  { slug: 'pd240', name: 'PD-240', group: 'PD', width: 640, height: 496, seconds: 248 },
  { slug: 'pd290', name: 'PD-290', group: 'PD', width: 800, height: 616, seconds: 289 },
]
const TX_MODE_GROUPS: TxMode['group'][] = ['Scottie', 'Martin', 'Robot', 'PD']
const MODE_BY_SLUG: Record<string, TxMode> = Object.fromEntries(
  SSTV_TX_MODES.map((m) => [m.slug, m]),
)

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
export function SstvView({ snap, onSnap, active = true, onSetFrequency, onSetTxEnabled }: Props) {
  // Live decoder state — polled at 1 Hz while this is the visible view (the
  // backend keeps decoding while hidden; the first tick catches the display up).
  const [sstv, setSstv] = useState<SstvState | null>(null)
  // Live snapshot ref so the Send handler reads the CURRENT dial/privileges (same
  // pattern as the CW/RTTY cockpits).
  const snapRef = useRef(snap)
  snapRef.current = snap
  useEffect(() => {
    if (!active) return
    let alive = true
    const tick = () => {
      getSstvState()
        .then((s) => {
          if (alive) setSstv(s)
        })
        .catch(() => {})
    }
    tick()
    const id = window.setInterval(tick, 1000)
    return () => {
      alive = false
      window.clearInterval(id)
    }
  }, [active])

  const armed = sstv?.armed === true
  const toggleArm = () => {
    void sstvArm(!armed)
      .then(setSstv)
      .catch(() => pushToast('Could not switch the SSTV receiver', 'error'))
  }

  // Licensed SSTV calling frequencies (built-in band plan — 14.230, the ISS
  // 145.800 FM downlink, …), same source as the CW/Phone band pickers.
  const [plan, setPlan] = useState<BandChannel[]>([])
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
    onSetFrequency?.(mhz, band, snap?.radio.sideband || 'USB')
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

  // Honest V1 caption: the two-pass core lands lines nearly all at once at
  // completion, so until they land we say "decoding <mode>…" — never a fake
  // progress count. VIS-detected mode + total show immediately.
  const inFlight = sstv?.mode != null
  const caption =
    inFlight && sstv
      ? (sstv.linesDone > 0
          ? `${sstv.mode} — ${sstv.linesDone}/${sstv.linesTotal} lines`
          : `decoding ${sstv.mode}…`)
      : ''

  // Gallery arrives oldest-first; show newest first.
  const gallery = sstv?.gallery && sstv.gallery.length > 0 ? [...sstv.gallery].reverse() : []

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
  const modeSlugRef = useRef(modeSlug)
  modeSlugRef.current = modeSlug
  const txMode = MODE_BY_SLUG[modeSlug]

  // The operator's chosen picture: decoded to an <img> once, then cover-cropped to
  // the selected mode's exact pixels on demand. `packed` holds the base64 RGB
  // actually sent — pixel-identical to the live preview canvas.
  const srcImgRef = useRef<HTMLImageElement | null>(null)
  const txCanvasRef = useRef<HTMLCanvasElement>(null)
  const [imageName, setImageName] = useState<string | null>(null)
  const [packed, setPacked] = useState<{
    slug: string
    width: number
    height: number
    b64: string
  } | null>(null)

  // Cover-crop the source image onto the preview canvas at the mode's dimensions
  // and read back the raw RGB (what you see is exactly what goes out).
  const recrop = (slug: string) => {
    const img = srcImgRef.current
    const canvas = txCanvasRef.current
    const m = MODE_BY_SLUG[slug]
    if (!img || !canvas || !m) return
    const sw = img.naturalWidth || img.width
    const sh = img.naturalHeight || img.height
    if (sw <= 0 || sh <= 0) return
    canvas.width = m.width
    canvas.height = m.height
    const ctx = canvas.getContext('2d')
    if (!ctx) return
    // Scale up until the image fills the frame, centre it, crop the overflow.
    const scale = Math.max(m.width / sw, m.height / sh)
    const dw = sw * scale
    const dh = sh * scale
    ctx.clearRect(0, 0, m.width, m.height)
    ctx.drawImage(img, (m.width - dw) / 2, (m.height - dh) / 2, dw, dh)
    try {
      const data = ctx.getImageData(0, 0, m.width, m.height).data
      setPacked({ slug, width: m.width, height: m.height, b64: rgbToBase64(data, m.width * m.height) })
    } catch {
      pushToast('Could not read the image pixels', 'error')
    }
  }

  // Re-crop whenever the mode changes and a picture is loaded — the preview + the
  // packed RGB must always match the dimensions the backend validates.
  useEffect(() => {
    if (srcImgRef.current) recrop(modeSlug)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [modeSlug])

  const loadImage = (file: File) => {
    if (!file.type.startsWith('image/')) {
      pushToast('Choose an image file (PNG / JPEG / …)', 'info', 3000)
      return
    }
    const url = URL.createObjectURL(file)
    const img = new Image()
    img.onload = () => {
      URL.revokeObjectURL(url)
      srcImgRef.current = img
      setImageName(file.name)
      recrop(modeSlugRef.current)
    }
    img.onerror = () => {
      URL.revokeObjectURL(url)
      pushToast('Could not load that image', 'error')
    }
    img.src = url
  }

  const changeMode = (slug: string) => {
    userPickedMode.current = true
    setModeSlug(slug)
  }

  const sendImage = () => {
    if (!packed || sending) return
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
      return sstvSend(packed.b64, packed.width, packed.height, packed.slug)
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

      <section className="sstv-canvas" aria-label="SSTV image">
        {inFlight ? (
          <div className="sstv-live">
            {preview && <canvas ref={canvasRef} className="sstv-live-canvas" />}
            <div className="sstv-live-caption" role="status">
              {caption}
            </div>
          </div>
        ) : (
          <div className="sstv-canvas-empty">
            {armed
              ? 'Armed — waiting for a VIS header…'
              : 'Tune 14.230 / 145.800 — images decode here'}
          </div>
        )}
      </section>

      <section className="sstv-tx" aria-label="Transmit an image">
        <div className="sstv-tx-head">Transmit</div>
        <div
          className={`sstv-tx-drop${packed ? ' loaded' : ''}`}
          onDragOver={(e) => e.preventDefault()}
          onDrop={(e) => {
            e.preventDefault()
            const f = e.dataTransfer.files?.[0]
            if (f) loadImage(f)
          }}
        >
          <canvas
            ref={txCanvasRef}
            className={`sstv-tx-preview${packed ? '' : ' empty'}`}
            role="img"
            aria-label={packed ? `Transmit preview, ${packed.width}×${packed.height}` : 'No image chosen'}
          />
          {!packed && (
            <div className="sstv-tx-drop-hint">
              Drop an image here, or choose one below. Cover-cropped to the mode size.
            </div>
          )}
        </div>

        <div className="sstv-tx-controls">
          <label className="sstv-tx-file">
            <span>{imageName ? 'Change image…' : 'Choose image…'}</span>
            <input
              type="file"
              accept="image/*"
              onChange={(e) => {
                const f = e.target.files?.[0]
                if (f) loadImage(f)
                e.target.value = ''
              }}
            />
          </label>
          {imageName && txMode && (
            <span className="sstv-tx-name" title={imageName}>
              {imageName} → {txMode.width}×{txMode.height}
            </span>
          )}
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
              disabled={!packed || sending}
              title={
                packed
                  ? 'Transmit this image — switches to Phone (USB/LSB) and keys the rig'
                  : 'Choose an image to transmit first'
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
      </section>

      <section className="sstv-gallery" aria-label="Received images">
        <div className="sstv-gallery-head">Gallery</div>
        <div className="sstv-gallery-grid">
          {gallery.length === 0 ? (
            <div className="sstv-gallery-empty">
              Received images collect here — auto-saved with callsign (FSK ID), mode, frequency, and
              time.
            </div>
          ) : (
            gallery.map((g) => (
              <figure key={g.path} className="sstv-thumb" title={g.path}>
                <GalleryThumb entry={g} />
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
      </section>
    </main>
  )
}
