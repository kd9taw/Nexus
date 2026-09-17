import { useId, useState } from 'react'
import type { ReactNode } from 'react'
import { Info } from 'lucide-react'
import { t } from '../i18n'
import type { FeedControl } from './client'
import { FeedWatch } from './FeedWatch'
import type { OperationClient } from './operation-client'
import { LoggingAuthority } from './operations'
import type { PresentationState } from './presentation'
import type { AlertToggle, StationAlertControl } from './browser-alerts'

// Presentation belongs to this browser. Expanding help cannot acquire, release
// or replace station authority, and never remounts the underlying Nexus app.
export function SessionStatus({ client, stale, disconnect, signOut, display, alerts, rareAlerts, potaAlerts, audio, feed }: {
  client?: OperationClient | null; stale: boolean; disconnect: () => void; signOut?: () => void; display?: PresentationState
  alerts?: AlertToggle; rareAlerts?: StationAlertControl; potaAlerts?: StationAlertControl
  /** The listen control. In the row itself, not behind the fold: an operator has to be
   *  able to stop the sound in one movement, and a control they have to expand a panel to
   *  reach is not that. It renders nothing at all on a station that cannot do audio. */
  audio?: ReactNode
  /** This tab's feed. In the row, for the same kind of reason as the listen control: the operator
   *  it exists for is watching a frequency on a screen they are NOT looking at, so they have to be
   *  able to find it before they look away, not after the feed has already paused on them.
   *
   *  Rendered TWICE, and only one is ever shown (`remote.css`). At xs the row has no width left to
   *  give — the authority label and its Release button already wrap to three lines there, and this
   *  strip may not take more than a quarter of the screen — so below that width the choice moves
   *  into the settings panel beside the other browser-local ones. It is one node or the other, both
   *  reading the same store, so the two can never disagree. */
  feed?: FeedControl
}) {
  const [expanded, setExpanded] = useState(false)
  const id = useId()
  const usable = !!alerts?.supported && !alerts.blocked
  return <div className="remote-application-status" role="region" aria-label={t('remote.browserWorkspace')}>
    <div className="remote-session-row">
      {/* Data loss is said in the label's own slot, never in a line of its own: this banner sits
          above every cockpit, and a line that appeared with each late sample moved all of them. */}
      {client?.enabled ? <LoggingAuthority client={client} unavailable={stale} /> : <span className="remote-session-label">
        <span role="alert" className="remote-session-unavailable">{stale ? t('remote.applicationUnavailable') + ' ' : null}</span>
        <span role="status">{t('monitor.observer')}</span>
      </span>}
      {audio}
      {feed && <FeedWatch feed={feed} />}
      <button type="button" className="remote-button remote-session-toggle"
        aria-label={t('remote.settingsLegend')} title={t('remote.settingsLegend')}
        aria-expanded={expanded} aria-controls={id} onClick={() => setExpanded(value => !value)}>
        <Info size={20} aria-hidden="true" />
        <span>{t('remote.settingsLegend')}</span>
      </button>
    </div>
    <div className="remote-session-info" id={id} hidden={!expanded}>
      <strong>{t('remote.browserWorkspace')}</strong>
      <p>{(client?.operationVersion ?? 0) >= 4 ? t('remote.ftControlPreview') : (client?.operationVersion ?? 0) >= 2 ? t('remote.controlPreview')
        : client?.enabled ? t('remote.applicationLoggingPreview') : t('remote.applicationObserver')}</p>
      <button type="button" className="remote-button" onClick={disconnect}>{t('remote.disconnect')}</button>
      {/* Signing out used to mean disconnecting first and finding the button on the stations page. */}
      {signOut && <button type="button" className="remote-button" onClick={signOut}>{t('remote.signOut')}</button>}
      {display && <button type="button" className="remote-button" onClick={() => {
        display.change(display.presentation === 'quick' ? 'full' : 'quick')
        setExpanded(false)
      }}>{display.presentation === 'quick' ? t('remote.quick.full') : t('remote.quick.name')}</button>}
      {/* The narrow-screen home for the feed choice — see the `feed` prop. */}
      {feed && <FeedWatch feed={feed} />}
      {/* Browser-local and notify-only. Lives in this folded panel so turning it on moves no cockpit.
          Notification permission is one per site, so a refusal is said once and hides every toggle. */}
      {alerts && <div className="remote-need-alerts">
        <p>{!alerts.supported ? t('remote.b3.needAlertsUnsupported') : alerts.blocked ? t('remote.b3.needAlertsBlocked') : t('remote.b3.needAlertsNote')}</p>
        {usable && <button type="button" className="remote-button" aria-pressed={alerts.enabled} onClick={alerts.toggle}>
          {alerts.enabled ? t('remote.b3.needAlertsOn') : t('remote.b3.needAlertsOff')}</button>}
        {usable && rareAlerts && <StationAlert control={rareAlerts} off={t('remote.b3.rareAlertsOff')} on={t('remote.b3.rareAlertsOn')}
          older={t('remote.b3.rareAlertsOlder')} quiet={t('remote.b3.rareAlertsStationOff')} />}
        {usable && potaAlerts && <StationAlert control={potaAlerts} off={t('remote.b3.potaAlertsOff')} on={t('remote.b3.potaAlertsOn')}
          older={t('remote.b3.potaAlertsOlder')} quiet={t('remote.b3.potaAlertsStale')} />}
      </div>}
    </div>
  </div>
}

/** One alert fed by a station read: its toggle, or why the station cannot feed it. */
function StationAlert({ control, off, on, older, quiet }: { control: StationAlertControl; off: string; on: string; older: string; quiet: string }) {
  if (!control.offered) return <p>{older}</p>
  return <>
    <button type="button" className="remote-button" aria-pressed={control.enabled} onClick={control.toggle}>{control.enabled ? on : off}</button>
    {control.note && <p role="status">{quiet}</p>}
  </>
}
