// The Rotor pane — a real control surface for the rotctld rotator (the Phase-1
// plumbing shipped earlier: point/point-at-call/read; this adds the cockpit).
// Compass rose with the LIVE azimuth needle (polled while mounted), click-the-
// rose or type to slew, STOP, and the WMM magnetic heading beside true so a
// compass-zeroed controller reads the same number. Draws only its box's one
// line when no rotator is CONFIGURED — honesty: a needle with no rotctld behind
// it would be an ornament. A configured rotator
// that does not report its position keeps the pane, with "—" where the needle
// would be: pointing and STOP do not depend on the readback.
//
// ⚠️ THIS FILE IS ON THE **MIGRATED** LIST (i18n/hardcoded-strings.test.ts): the prose is in
// the catalog under `rotor.pane.*`. Its ■ STOP halts ROTATION, not a transmission — it is on no
// cockpit's stop-line census and no sweep looks for it — so nothing here is deferred.
//
// The units rule lands on the COMPASS: every azimuth and elevation in degrees, the true/
// magnetic `°T`/`°M` marks, the `az°` and `el°` the entry fields ask for, the `EL` plate and the
// four cardinal letters are the vocabulary of the instrument and stay in the code.
//
// ELEVATION (2026-09-29, the Yaesu G-5500): shown and settable only when the rotator's own
// backend declares an elevation axis (`read_rotator_state`'s `elRange`, from rotctld's
// `\dump_state` — never guessed from the model's name), within the range it declares. Each move
// carries the other axis only while the mast is still on its way to it (features/rotorTargets);
// otherwise the backend keeps that axis where the rotator reports it. The one ■ STOP stops both.
// A browser on Nexus Remote keeps the azimuth-only pane it had.
//
// LINE 2, BESIDE A COCKPIT (`entryCall`): the call in that cockpit's log entry, the bearing and
// distance to it, and Point. The bearing is the station's own answer to "where would a point at
// this call turn the antenna" (`rotatorBearingToCall`: the point's resolver, read only), never one
// worked out here, so the number shown is the number Point turns to. Point is the cockpits'
// point-at-call (`rotorPointAt`), short path. It turns the antenna and nothing else: it keys
// nothing, and this pane's ■ STOP stops rotation, never a transmission. No bearing is said in words
// (the station is not placed, or the operator's own grid is not set), never drawn as a number
// nobody resolved; no call, no line. A browser has no line 2 at all: the bearing read is the
// desktop's, and Nexus Remote offers it nothing new.
import { useEffect, useRef, useState } from 'react'
import {
  getDeclination,
  getSatTrackStatus,
  getSettings,
  pointRotator,
  pointRotatorElevation,
  readRotator,
  readRotatorState,
  rotatorBearingToCall,
  stopRotator,
  stopSatTrack,
} from '../../api'
import type { CallBearing, RotatorState, SatTrackStatus } from '../../types'
import { useStationControl } from '../../stationAccess'
import { magneticDeg } from '../../grid'
import { pushToast } from '../../toast'
import { t } from '../../i18n'
import { pollSingleFlight } from '../../singleFlight'
import { follow, pending, type Pending } from '../../features/rotorTargets'
import { fmtDistanceKm, useUnits } from '../../units'
import { ROTATOR_POLL_MS } from '../../remote-web/rotator'
import { pointedTo, rotorPointAt } from '../rotorPointAt'

const SIZE = 148
const R = SIZE / 2 - 10

/** The instrument's own marks: the compass points, and the abbreviation the bearing field asks
 *  for. Tokens, named so the catalog guard reads them as a decision. */
const CARDINALS = ['N', 'E', 'S', 'W']
const AZ_ENTRY = 'az°'
/** The elevation's plate and the abbreviation its entry field asks for. */
const EL_PLATE = 'EL'
const EL_ENTRY = 'el°'

function azFromClick(e: React.MouseEvent<SVGSVGElement>): number {
  const rect = e.currentTarget.getBoundingClientRect()
  const dx = e.clientX - rect.left - rect.width / 2
  const dy = e.clientY - rect.top - rect.height / 2
  return (Math.atan2(dx, -dy) * (180 / Math.PI) + 360) % 360
}

/** Why the station has no bearing for a call: the code its bearing read refused with, or null for a
 *  failure it did not name (then no bearing is drawn, and Point stays: the point says what it finds). */
type NoBearing = 'noGrid' | 'unknownStation' | null

function noBearing(e: unknown): NoBearing {
  const code = e instanceof Error ? e.message : typeof e === 'string' ? e : ''
  return code === 'noGrid' || code === 'unknownStation' ? code : null
}

/** The station's last answer for line 2, and the call it answered for. */
type EntryAim = { call: string } & ({ bearing: CallBearing } | { why: NoBearing })

export function RotorPane({ entryCall = null }: { entryCall?: string | null } = {}) {
  // null = never read (no rotator / daemon down) → pane hides itself.
  const [az, setAz] = useState<number | null>(null)
  const [target, setTarget] = useState<number | null>(null)
  const [entry, setEntry] = useState('')
  // The elevation as the rotator reports it, and the range its backend declares — null for a
  // rotator with no elevation axis, which is what keeps the elevation out of the pane there.
  const [el, setEl] = useState<number | null>(null)
  const [elRange, setElRange] = useState<[number, number] | null>(null)
  const [targetEl, setTargetEl] = useState<number | null>(null)
  const [elEntry, setElEntry] = useState('')
  // What this pane still hands over for the axis it is NOT moving (features/rotorTargets):
  // followed through every reading, dropped on arrival, STOP, a satellite track or a failure.
  const pendingAz = useRef<Pending | null>(null)
  const pendingEl = useRef<Pending | null>(null)
  const [declination, setDeclination] = useState<number | null>(null)
  // Satellite auto-track owning the rotor right now (Satellites section's loop).
  // Shown so the operator knows WHY the needle moves on its own — and so a manual
  // slew/STOP halts the LOOP, not just one command the loop's next 3 s tick redoes.
  const [satTrack, setSatTrack] = useState<SatTrackStatus | null>(null)
  // Is a rotator CONFIGURED at all? Split from "is it reading back", because the two are
  // different stations and only one of them should lose the pane — see the null branch below.
  const [configured, setConfigured] = useState(false)
  // What the last read found (desktop only). ⚠️ A rotator that reports no position and one that
  // does not answer at all are different stations: the Hy-Gain DCU-1 has no read-back and still
  // points; a controller that is switched off answers nothing, and "pointing still works" is
  // then untrue (a tester read "Error 61" on every command). A browser keeps its own read, and
  // this stays null there.
  const [reading, setReading] = useState<RotatorState['reading'] | null>(null)
  const local = useStationControl()
  const alive = useRef(true)
  const units = useUnits()
  // LINE 2's call: the desktop's only (see the header).
  const aimCall = local && entryCall ? entryCall : null
  const [aim, setAim] = useState<EntryAim | null>(null)

  useEffect(() => {
    alive.current = true
    getSettings()
      .then((st) => {
        if (alive.current) setConfigured((st.rotatorModel ?? 0) > 0 || st.rotatorHost.trim() !== '')
      })
      .catch(() => {})
    // One single-flight poll (#335): `read_rotator` takes the engine mutex (before the rotator
    // I/O), so a tick skips while the last read is still out.
    const stop = pollSingleFlight('rotor pane', 2_000, (owns) =>
      Promise.allSettled([
        (local
          ? readRotatorState().then((st) => {
              if (!owns()) return
              setAz(st?.azDeg ?? null)
              setReading(st?.reading ?? null)
              setEl(st?.elDeg ?? null)
              // Absent: rotctld did not answer what the rotator can do this time, so what was
              // known stands. No rotator at all is null.
              if (st == null) setElRange(null)
              else if (st.elRange !== undefined) setElRange(st.elRange)
              pendingAz.current = follow(pendingAz.current, st?.azDeg ?? null, true)
              pendingEl.current = follow(pendingEl.current, st?.elDeg ?? null, false)
            })
          : readRotator().then((v) => {
              if (owns()) setAz(v)
            })
        ).catch(() => {
          if (!owns()) return
          setAz(null)
          setReading(null)
          setEl(null)
        }),
        getSatTrackStatus().then((t) => {
          if (!owns()) return
          setSatTrack(t)
          // A pass owns the mast: nothing this pane sent before it may be handed over after it.
          if (t) pendingAz.current = pendingEl.current = null
        }),
      ]),
    )
    getDeclination()
      .then((d) => alive.current && setDeclination(d))
      .catch(() => {})
    return () => {
      alive.current = false
      stop()
    }
  }, [local])

  // Line 2's bearing: asked the moment the entry's call changes and again on every poll, because
  // what the station knows of the call moves under it (the log form's grid lands from the callbook
  // a moment after the call is typed, and the bearing moves with it). An answer goes with its poll.
  useEffect(() => {
    if (!aimCall) return
    const stop = pollSingleFlight('rotor pane entry', ROTATOR_POLL_MS, (owns) =>
      rotatorBearingToCall(aimCall).then(
        (bearing) => {
          if (owns()) setAim({ call: aimCall, bearing })
        },
        (e) => {
          if (owns()) setAim({ call: aimCall, why: noBearing(e) })
        },
      ),
    )
    return () => {
      stop()
      setAim(null)
    }
  }, [aimCall])

  // ⭐ A ROTATOR YOU CANNOT READ IS STILL A ROTATOR YOU CAN POINT. This used to be
  // `if (az == null) return null`, which deleted the rose, the click-to-slew, the typed bearing
  // AND the STOP button the moment the readback failed — and readback fails for reasons that
  // have nothing to do with pointing. Model 403 (Hy-Gain DCU-1/DCU-1X, a curated entry) has no
  // `get_position` in the bundled Hamlib at all: it answers `p` with `RPRT -11` for ever while
  // taking every `P` perfectly. Its owner had no compass, no slew and no stop.
  //
  // So the two states are separated: no rotator CONFIGURED draws only the box's one line (most
  // stations; drawn here, since the frame never sees a null from this component), while a rotator
  // that is configured but not reporting keeps its whole control surface with an honest "—" where
  // the needle would be. A fake needle would be the dishonest half; a missing STOP button is the
  // dangerous one.
  if (az == null && !configured) return <p className="pane-basic">{t('connect.pane.rotor.basic')}</p>

  const slew = (deg: number) => {
    const d = ((Math.round(deg) % 360) + 360) % 360
    setTarget(d)
    // An elevation still on its way goes with the bearing; otherwise the backend keeps the
    // elevation where the rotator reports it. A browser sends the bearing alone, as it always has.
    const withEl = local ? pendingEl.current?.deg : undefined
    pendingAz.current = pending(d)
    // ALWAYS stop the sat track first (no-op when idle): while a track owns the
    // rotor the loop re-commands az/el every 3 s, so a bare pointRotator would be
    // reverted within one tick. Halt the loop, then take the rotor manually.
    stopSatTrack()
      .then(() => {
        setSatTrack(null)
        return withEl === undefined ? pointRotator(d) : pointRotator(d, withEl)
      })
      .catch((e) => {
        pendingAz.current = null
        pushToast(
          t('rotor.pane.slew.failed', { error: e instanceof Error ? e.message : String(e) }),
          'error',
        )
      })
  }

  const elevate = (deg: number) => {
    if (!elRange) return
    const [lo, hi] = elRange
    const e = Math.round(deg)
    // Refused here, before anything is sent, rather than clamped onto the stop.
    if (!Number.isFinite(e) || e < lo || e > hi) {
      pushToast(t('rotor.pane.el.outside', { min: lo, max: hi }), 'error')
      return
    }
    setTargetEl(e)
    const withAz = pendingAz.current?.deg
    pendingEl.current = pending(e)
    stopSatTrack()
      .then(() => {
        setSatTrack(null)
        return withAz === undefined ? pointRotatorElevation(e) : pointRotatorElevation(e, withAz)
      })
      .catch((err) => {
        pendingEl.current = null
        pushToast(
          t('rotor.pane.slew.failed', { error: err instanceof Error ? err.message : String(err) }),
          'error',
        )
      })
  }

  const needle = (deg: number, len: number) => {
    const rad = (deg - 90) * (Math.PI / 180)
    return { x: SIZE / 2 + len * Math.cos(rad), y: SIZE / 2 + len * Math.sin(rad) }
  }
  const cur = az != null ? needle(az, R - 8) : null
  const tgt = target != null ? needle(target, R - 2) : null
  const mag = az != null ? magneticDeg(az, declination) : null
  const silent = az == null && reading === 'notAnswering'
  // An answer is for the call it was asked about: one for the call before is no answer.
  const entryAim = aimCall != null && aim?.call === aimCall ? aim : null
  const entryBearing = entryAim && 'bearing' in entryAim ? entryAim.bearing : null
  const entryWhy = entryAim && 'why' in entryAim ? entryAim.why : null

  const pointAtEntry = (call: string) => {
    // The mast goes where the station sends it now, so nothing this pane sent before may be handed
    // over after it (features/rotorTargets), and its own target needle and "→" lines no longer say
    // where the mast is going.
    pendingAz.current = pendingEl.current = null
    setTarget(null)
    setTargetEl(null)
    rotorPointAt(local)(call)
  }

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
                ? silent
                  ? t('rotor.pane.notAnswering')
                  : t('rotor.pane.az.title.unknown')
                : mag != null
                  ? t('rotor.pane.az.title.magnetic', { deg: Math.round(az), mag })
                  : t('rotor.pane.az.title')
            }
          >
            {az == null ? '—°T' : `${Math.round(az)}°T`}
            {mag != null && <span className="rotor-mag"> {mag}°M</span>}
          </div>
          {elRange && (
            <div
              className="rotor-el mono"
              title={t('rotor.pane.el.title', { min: elRange[0], max: elRange[1] })}
            >
              {EL_PLATE} {el == null ? '—' : Math.round(el)}°
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
            {elRange && (
              <input
                className="settings-input mono"
                type="number"
                min={elRange[0]}
                max={elRange[1]}
                placeholder={EL_ENTRY}
                value={elEntry}
                onChange={(e) => setElEntry(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && elEntry.trim() !== '') {
                    elevate(Number(elEntry))
                    setElEntry('')
                  }
                }}
                aria-label={t('rotor.pane.el.entry.aria', { min: elRange[0], max: elRange[1] })}
              />
            )}
            <button
              type="button"
              className="rotor-stop"
              onClick={() => {
                // One STOP for both axes (rotctld `S`, the GS-232B's All Stop), and nothing this
                // pane sent before it may be handed over after it: the next move keeps both axes
                // where they stopped.
                pendingAz.current = pendingEl.current = null
                // Stop the track first (no-op when idle): the satTrack poll is up to
                // 2 s stale, and a bare rotor stop mid-pass would be undone by the
                // loop's next 3 s tick. Belt-and-braces halt.
                stopSatTrack()
                  .then(() => {
                    setSatTrack(null)
                    return stopRotator()
                  })
                  .then(() => {
                    // Stopped, so heading nowhere: what it was sent to is no longer on its way.
                    // Not before the rotator answers — a stop that failed may leave it moving.
                    setTarget(null)
                    setTargetEl(null)
                  })
                  .catch((e) =>
                    pushToast(
                      t('rotor.stop.failed', {
                        error: e instanceof Error ? e.message : String(e),
                      }),
                      'error',
                    ),
                  )
              }}
              title={t('rotor.pane.stop.title')}
            >
              {t('rotor.pane.stop.label')}
            </button>
          </div>
          {/* Line 2, under the controls for the reason the lines below are: above them it would move
              ■ STOP every time a call was entered or logged. */}
          {aimCall && (
            <div
              className="rotor-aim"
              title={
                entryBearing
                  ? t('rotor.pane.aim.title', { call: aimCall, to: pointedTo(entryBearing.pointed) })
                  : undefined
              }
            >
              <span className="rotor-aim-to mono">
                → {aimCall}
                {entryBearing &&
                  ` ${Math.round(entryBearing.pointed.bearing) % 360}° (${fmtDistanceKm(entryBearing.km, units)})`}
              </span>
              {entryWhy === 'unknownStation' && <span className="rotor-aim-why">{t('rotor.pane.aim.unknown')}</span>}
              {entryWhy === 'noGrid' && <span className="rotor-aim-why">{t('rotor.pane.aim.noGrid')}</span>}
              {entryWhy == null && (
                <button
                  type="button"
                  className="rotor-aim-point"
                  onClick={() => pointAtEntry(aimCall)}
                  title={t('rotor.pane.aim.point.title', { call: aimCall })}
                >
                  {t('rotor.pane.aim.point.label')}
                </button>
              )}
            </div>
          )}
          {/* What is on its way, UNDER the controls: above them, each line pushed ■ STOP down the
              moment a slew began, which is when it is wanted — and in a short rail, off the pane. */}
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
          {elRange && targetEl != null && (el == null || Math.abs(targetEl - el) > 2) && (
            <div className="rotor-slewing" title={t('rotor.pane.commandedEl.title')}>
              → {EL_PLATE} {targetEl}°
            </div>
          )}
          <p className="rotor-hint">
            {az != null
              ? elRange
                ? t('rotor.pane.hint.azel')
                : t('rotor.pane.hint')
              : silent
                ? t('rotor.pane.notAnswering')
                : t('rotor.pane.hint.noPosition')}
          </p>
        </div>
      </div>
    </section>
  )
}
