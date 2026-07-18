import { useEffect, useRef, useState } from 'react'
import type { AppSnapshot, BandChannel, SstvGalleryEntry, SstvState } from '../types'
import { CockpitHeader } from './CockpitHeader'
import { FrequencyControl } from './FrequencyControl'
import { getLicensedBandPlan, getSstvState, sstvArm } from '../api'
import { bandLabelForMhz } from '../band'
import { pushToast } from '../toast'

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
export function SstvView({ snap, onSnap, active = true, onSetFrequency }: Props) {
  // Live decoder state — polled at 1 Hz while this is the visible view (the
  // backend keeps decoding while hidden; the first tick catches the display up).
  const [sstv, setSstv] = useState<SstvState | null>(null)
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

  return (
    <main className="layout single sstv-view">
      {snap && (
        <CockpitHeader
          snap={snap}
          onSnap={onSnap}
          txState={false}
          modeIndicator={
            <span
              className="cw-mode-badge"
              title="Detected SSTV mode — fills in (Martin / Scottie / Robot / PD) when the receiver hears a VIS header"
            >
              {inFlight && sstv?.mode ? `SSTV · ${sstv.mode}` : 'SSTV'}
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
