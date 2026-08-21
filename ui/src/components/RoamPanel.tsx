// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog. What does NOT: the band-plan channel labels and dial
// frequencies, the partner's callsign, and the role word — `initiator` / `follower` / `idle`
// is the backend's enum, printed as it arrives (only the "unpaired" stand-in is prose).
//
// `Stop → home` stops the channel HOPPING and returns to the home channel. It is not a
// transmit control and is on no cockpit's stop-line census.
import { useEffect, useState } from 'react'
import type { AppSnapshot, BandChannel, QsyStatus } from '../types'
import {
  qsyConfigure as apiQsyConfigure,
  qsyMoveNow as apiQsyMoveNow,
  qsyPause as apiQsyPause,
  qsySetEnabled as apiQsySetEnabled,
  qsyStop as apiQsyStop,
} from '../api'
import { withErrorToast } from '../toast'
import { t } from '../i18n'
import { T } from '../i18n/T'

interface Props {
  /** Live coordinated-QSY status (null while the feature is off). */
  qsy: QsyStatus | null
  /** Persisted QSY channel set (band-plan tokens). */
  channels: string[]
  /** Persisted announce cadence (initiator hops every N overs). */
  cadence: number
  /** Band-plan presets, for the channel picker. */
  bandPlan: BandChannel[]
  /** Currently-selected peer (the roaming partner defaults to this). */
  activePeer: string | null
  /** Apply a fresh snapshot returned by a command. */
  onSnap: (s: AppSnapshot) => void
  /** Refresh persisted settings after a configure (set / cadence). */
  onReloadSettings: () => void
}

const CADENCES = [3, 6, 10, 20]

export function RoamPanel({
  qsy,
  channels,
  cadence,
  bandPlan,
  activePeer,
  onSnap,
  onReloadSettings,
}: Props) {
  // Local copies so the chips/cadence feel instant; committed to the backend on
  // change (which persists + returns a snapshot).
  const [set, setSet] = useState<string[]>(channels)
  const [cad, setCad] = useState<number>(cadence)

  useEffect(() => setSet(channels), [channels])
  useEffect(() => setCad(cadence), [cadence])

  const enabled = qsy?.enabled ?? false
  const paused = qsy?.paused ?? false

  const apply = (p: Promise<AppSnapshot | void>, msg: string) =>
    void withErrorToast(() => p, msg).then((s) => {
      if (s) onSnap(s)
    })

  const commitConfig = (nextSet: string[], nextCad: number) => {
    setSet(nextSet)
    setCad(nextCad)
    void withErrorToast(() => apiQsyConfigure(nextSet, nextCad), t('roam.failed.configure')).then(
      (s) => {
        if (s) onSnap(s)
        onReloadSettings()
      },
    )
  }

  const toggleChannel = (band: string) => {
    const next = set.includes(band) ? set.filter((b) => b !== band) : [...set, band]
    commitConfig(next, cad)
  }

  const role = qsy?.role ?? 'idle'
  const partner = qsy?.partner ?? activePeer
  const statusLine = !enabled
    ? t('roam.status.off')
    : paused
      ? t('roam.status.paused', { channel: qsy?.current ?? '—' })
      : qsy?.lostSync
        ? t('roam.status.lostSync', { home: qsy?.home ?? t('roam.status.home') })
        : qsy?.nextChannel
          ? qsy.nextSlot != null
            ? t('roam.status.nextSlot', { channel: qsy.nextChannel, slot: qsy.nextSlot })
            : t('roam.status.next', { channel: qsy.nextChannel })
          : role === 'initiator'
            ? t('roam.status.auto', { channel: qsy?.current ?? '—', count: cad })
            : role === 'follower'
              ? t('roam.status.following', {
                  partner: partner ?? t('roam.status.partner'),
                  channel: qsy?.current ?? '—',
                })
              : t('roam.status.idle')

  return (
    <section className="panel roam-panel">
      <div className="panel-header">
        <h2>{t('roam.title')}</h2>
        <span className="settings-sub">{t('roam.subtitle')}</span>
      </div>

      <div className="roam-scroll">
        {/* Non-dismissible honesty disclaimer (legal ceiling, in the clear). */}
        <div className="roam-disclaimer" role="note">
          <T k="roam.disclaimer" tags={{ b: <strong />, em: <em /> }} />
        </div>

        {/* Master enable */}
        <div className="roam-row roam-enable">
          <div>
            <div className="roam-row-title">{t('roam.enable.title')}</div>
            <div className="roam-row-sub">
              {enabled ? t('roam.enable.sub.on') : t('roam.enable.sub.off')}
            </div>
          </div>
          <button
            type="button"
            className={`op-btn monitor${enabled ? ' on' : ''}`}
            aria-pressed={enabled}
            onClick={() => apply(apiQsySetEnabled(!enabled), t('roam.failed.enable'))}
          >
            {enabled ? t('roam.enable.on') : t('roam.enable.off')}
          </button>
        </div>

        {/* Partner + role */}
        <div className="roam-row">
          <div>
            <div className="roam-row-title">{t('roam.partner.title')}</div>
            <div className="roam-row-sub">
              {partner ? (
                <T
                  k="roam.partner.line"
                  tags={{ b: <strong /> }}
                  vals={{ partner, role: role === 'idle' ? t('roam.role.unpaired') : role }}
                />
              ) : (
                t('roam.partner.none')
              )}
            </div>
          </div>
          <span className={`roam-chip role-${role}`}>{role}</span>
        </div>

        {/* Channel set */}
        <fieldset className="settings-section roam-channels" disabled={!enabled}>
          <legend>{t('roam.channels.legend')}</legend>
          <p className="settings-hint">{t('roam.channels.hint')}</p>
          <div className="roam-chip-grid">
            {bandPlan.map((c) => {
              const on = set.includes(c.band)
              const vhfPlus = c.group === 'VHF' || c.group === 'UHF'
              return (
                <button
                  key={c.band}
                  type="button"
                  className={`theme-chip roam-ch${on ? ' active' : ''}`}
                  aria-pressed={on}
                  title={t('roam.channel.title', {
                    label: c.label,
                    freq: c.dialMhz.toFixed(4),
                    mode: c.mode,
                  })}
                  onClick={() => toggleChannel(c.band)}
                >
                  {c.label}
                  {vhfPlus && <span className="roam-ch-tag">{c.group}</span>}
                </button>
              )
            })}
          </div>
        </fieldset>

        {/* Cadence */}
        <fieldset className="settings-section" disabled={!enabled}>
          <legend>{t('roam.cadence.legend')}</legend>
          <p className="settings-hint">{t('roam.cadence.hint')}</p>
          <div className="theme-switcher" role="group" aria-label={t('roam.cadence.aria')}>
            {CADENCES.map((n) => (
              <button
                key={n}
                type="button"
                className={`theme-chip${cad === n ? ' active' : ''}`}
                aria-pressed={cad === n}
                onClick={() => commitConfig(set, n)}
              >
                {t('roam.cadence.option', { count: n })}
              </button>
            ))}
          </div>
        </fieldset>

        {/* Overrides + status */}
        <fieldset className="settings-section" disabled={!enabled}>
          <legend>{t('roam.controls.legend')}</legend>
          <div className="roam-controls">
            <button
              type="button"
              className="op-btn"
              disabled={!enabled || paused || role !== 'initiator'}
              title={t('roam.moveNow.title')}
              onClick={() => apply(apiQsyMoveNow(), t('roam.failed.moveNow'))}
            >
              {t('roam.moveNow.label')}
            </button>
            <button
              type="button"
              className={`op-btn${paused ? ' on' : ''}`}
              aria-pressed={paused}
              title={t('roam.pause.title')}
              onClick={() => apply(apiQsyPause(!paused), t('roam.failed.pause'))}
            >
              {paused ? t('roam.resume.label') : t('roam.pause.label')}
            </button>
            <button
              type="button"
              className="op-btn stop"
              title={t('roam.stop.title')}
              onClick={() => apply(apiQsyStop(), t('roam.failed.stop'))}
            >
              {t('roam.stop.label')}
            </button>
          </div>
          <div className={`roam-status${qsy?.lostSync ? ' bad' : ''}`} role="status">
            <span className="roam-status-dot" aria-hidden />
            {statusLine}
          </div>
        </fieldset>
      </div>
    </section>
  )
}
