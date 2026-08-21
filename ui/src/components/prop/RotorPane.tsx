// The Rotor pane — a real control surface for the rotctld rotator (the Phase-1
// plumbing shipped earlier: point/point-at-call/read; this adds the cockpit).
// Compass rose with the LIVE azimuth needle (polled while mounted), click-the-
// rose or type to slew, STOP, and the WMM magnetic heading beside true so a
// compass-zeroed controller reads the same number. Renders nothing when no
// rotator is CONFIGURED (PaneFrame falls back to the Basic hint) — honesty:
// a needle with no rotctld behind it would be an ornament. A configured rotator
// that does not report its position keeps the pane, with "—" where the needle
// would be: pointing and STOP do not depend on the readback.
//
// ⚠️ THIS FILE IS ON THE **MIGRATED** LIST (i18n/hardcoded-strings.test.ts): the prose is in
// the catalog under `rotor.pane.*`. Its ■ STOP halts ROTATION, not a transmission — it is on no
// cockpit's stop-line census and no sweep looks for it — so nothing here is deferred.
//
// The units rule lands on the COMPASS: every azimuth and elevation in degrees, the true/
// magnetic `°T`/`°M` marks, the `az°` the entry field asks for and the four cardinal letters
// are the vocabulary of the instrument and stay in the code.
import { useEffect, useRef, useState } from 'react'
import {
  getDeclination,
  getSatTrackStatus,
  getSettings,
  pointRotator,
  readRotator,
  stopRotator,
  stopSatTrack,
} from '../../api'
import type { SatTrackStatus } from '../../types'
import { magneticDeg } from '../../grid'
import { pushToast } from '../../toast'
import { t } from '../../i18n'

const SIZE = 148
const R = SIZE / 2 - 10

/** The instrument's own marks: the compass points, and the abbreviation the bearing field asks
 *  for. Tokens, named so the catalog guard reads them as a decision. */
const CARDINALS = ['N', 'E', 'S', 'W']
const AZ_ENTRY = 'az°'

function azFromClick(e: React.MouseEvent<SVGSVGElement>): number {
  const rect = e.currentTarget.getBoundingClientRect()
  const dx = e.clientX - rect.left - rect.width / 2
  const dy = e.clientY - rect.top - rect.height / 2
  return (Math.atan2(dx, -dy) * (180 / Math.PI) + 360) % 360
}

export function RotorPane() {
  // null = never read (no rotator / daemon down) → pane hides itself.
  const [az, setAz] = useState<number | null>(null)
  const [target, setTarget] = useState<number | null>(null)
  const [entry, setEntry] = useState('')
  const [declination, setDeclination] = useState<number | null>(null)
  // Satellite auto-track owning the rotor right now (Satellites section's loop).
  // Shown so the operator knows WHY the needle moves on its own — and so a manual
  // slew/STOP halts the LOOP, not just one command the loop's next 3 s tick redoes.
  const [satTrack, setSatTrack] = useState<SatTrackStatus | null>(null)
  // Is a rotator CONFIGURED at all? Split from "is it reading back", because the two are
  // different stations and only one of them should lose the pane — see the null branch below.
  const [configured, setConfigured] = useState(false)
  const alive = useRef(true)

  useEffect(() => {
    alive.current = true
    getSettings()
      .then((st) => {
        if (alive.current) setConfigured((st.rotatorModel ?? 0) > 0 || st.rotatorHost.trim() !== '')
      })
      .catch(() => {})
    const load = () => {
      readRotator()
        .then((v) => {
          if (alive.current) setAz(v)
        })
        .catch(() => {
          if (alive.current) setAz(null)
        })
      getSatTrackStatus()
        .then((t) => {
          if (alive.current) setSatTrack(t)
        })
        .catch(() => {})
    }
    load()
    const id = window.setInterval(load, 2_000)
    getDeclination()
      .then((d) => alive.current && setDeclination(d))
      .catch(() => {})
    return () => {
      alive.current = false
      window.clearInterval(id)
    }
  }, [])

  // ⭐ A ROTATOR YOU CANNOT READ IS STILL A ROTATOR YOU CAN POINT. This used to be
  // `if (az == null) return null`, which deleted the rose, the click-to-slew, the typed bearing
  // AND the STOP button the moment the readback failed — and readback fails for reasons that
  // have nothing to do with pointing. Model 403 (Hy-Gain DCU-1/DCU-1X, a curated entry) has no
  // `get_position` in the bundled Hamlib at all: it answers `p` with `RPRT -11` for ever while
  // taking every `P` perfectly. Its owner had no compass, no slew and no stop.
  //
  // So the two states are separated: no rotator CONFIGURED renders nothing (most stations, and
  // the pane frame's Basic hint takes over), while a rotator that is configured but not
  // reporting keeps its whole control surface with an honest "—" where the needle would be. A
  // fake needle would be the dishonest half; a missing STOP button is the dangerous one.
  if (az == null && !configured) return null

  const slew = (deg: number) => {
    const d = ((Math.round(deg) % 360) + 360) % 360
    setTarget(d)
    // ALWAYS stop the sat track first (no-op when idle): while a track owns the
    // rotor the loop re-commands az/el every 3 s, so a bare pointRotator would be
    // reverted within one tick. Halt the loop, then take the rotor manually.
    stopSatTrack()
      .then(() => {
        setSatTrack(null)
        return pointRotator(d)
      })
      .catch((e) =>
        pushToast(
          t('rotor.pane.slew.failed', { error: e instanceof Error ? e.message : String(e) }),
          'error',
        ),
      )
  }

  const needle = (deg: number, len: number) => {
    const rad = (deg - 90) * (Math.PI / 180)
    return { x: SIZE / 2 + len * Math.cos(rad), y: SIZE / 2 + len * Math.sin(rad) }
  }
  const cur = az != null ? needle(az, R - 8) : null
  const tgt = target != null ? needle(target, R - 2) : null
  const mag = az != null ? magneticDeg(az, declination) : null

  return (
    <section className="rotor-pane panel">
      <div className="rotor-row">
        <svg
          width={SIZE}
          height={SIZE}
          className="rotor-rose"
          onClick={(e) => slew(azFromClick(e))}
          role="img"
          aria-label={
            az != null
              ? t('rotor.pane.rose.aria', { deg: Math.round(az) })
              : t('rotor.pane.rose.aria.unknown')
          }
        >
          <circle cx={SIZE / 2} cy={SIZE / 2} r={R} className="rotor-ring" />
          {CARDINALS.map((c, i) => {
            const p = needle(i * 90, R - 14)
            return (
              <text key={c} x={p.x} y={p.y + 4} textAnchor="middle" className="rotor-cardinal">
                {c}
              </text>
            )
          })}
          {Array.from({ length: 12 }, (_, i) => {
            const a = i * 30
            const o = needle(a, R)
            const inn = needle(a, R - 5)
            return <line key={a} x1={inn.x} y1={inn.y} x2={o.x} y2={o.y} className="rotor-tick" />
          })}
          {tgt && (
            <line
              x1={SIZE / 2}
              y1={SIZE / 2}
              x2={tgt.x}
              y2={tgt.y}
              className="rotor-needle target"
            />
          )}
          {cur && <line x1={SIZE / 2} y1={SIZE / 2} x2={cur.x} y2={cur.y} className="rotor-needle" />}
          <circle cx={SIZE / 2} cy={SIZE / 2} r={3} className="rotor-hub" />
        </svg>
        <div className="rotor-side">
          <div
            className="rotor-az mono"
            title={
              az == null
                ? t('rotor.pane.az.title.unknown')
                : mag != null
                  ? t('rotor.pane.az.title.magnetic', { deg: Math.round(az), mag })
                  : t('rotor.pane.az.title')
            }
          >
            {az == null ? '—°T' : `${Math.round(az)}°T`}
            {mag != null && <span className="rotor-mag"> {mag}°M</span>}
          </div>
          {satTrack && (
            <div
              className="rotor-slewing"
              title={t('rotor.pane.track.title', {
                bird: satTrack.name,
                state: satTrack.state,
              })}
            >
              ⟳ {satTrack.name}
            </div>
          )}
          {target != null && (az == null || Math.abs(((target - az + 540) % 360) - 180) > 2) && (
            <div className="rotor-slewing" title={t('rotor.pane.commanded.title')}>
              → {target}°
            </div>
          )}
          <div className="rotor-entry">
            <input
              className="settings-input mono"
              type="number"
              min={0}
              max={359}
              placeholder={AZ_ENTRY}
              value={entry}
              onChange={(e) => setEntry(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && entry.trim() !== '') {
                  slew(Number(entry))
                  setEntry('')
                }
              }}
              aria-label={t('rotor.pane.entry.aria')}
            />
            <button
              type="button"
              className="rotor-stop"
              onClick={() =>
                // Stop the track first (no-op when idle): the satTrack poll is up to
                // 2 s stale, and a bare rotor stop mid-pass would be undone by the
                // loop's next 3 s tick. Belt-and-braces halt.
                stopSatTrack()
                  .then(() => {
                    setSatTrack(null)
                    return stopRotator()
                  })
                  .catch((e) =>
                    pushToast(
                      t('rotor.stop.failed', {
                        error: e instanceof Error ? e.message : String(e),
                      }),
                      'error',
                    ),
                  )
              }
              title={t('rotor.pane.stop.title')}
            >
              {t('rotor.pane.stop.label')}
            </button>
          </div>
          <p className="rotor-hint">
            {az == null ? t('rotor.pane.hint.noPosition') : t('rotor.pane.hint')}
          </p>
        </div>
      </div>
    </section>
  )
}
