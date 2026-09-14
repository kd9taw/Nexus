import { useId, useState } from 'react'
import { Info } from 'lucide-react'
import { t } from '../i18n'
import type { OperationClient } from './operation-client'
import { LoggingAuthority } from './operations'
import type { PresentationState } from './presentation'
import type { NeedAlertsControl } from './useNeedAlerts'

// Presentation belongs to this browser. Expanding help cannot acquire, release
// or replace station authority, and never remounts the underlying Nexus app.
export function SessionStatus({ client, stale, disconnect, display, alerts }: {
  client?: OperationClient | null; stale: boolean; disconnect: () => void; display?: PresentationState; alerts?: NeedAlertsControl
}) {
  const [expanded, setExpanded] = useState(false)
  const id = useId()
  return <div className="remote-application-status" role="region" aria-label={t('remote.browserWorkspace')}>
    <div className="remote-session-row">
      {/* Data loss is said in the label's own slot, never in a line of its own: this banner sits
          above every cockpit, and a line that appeared with each late sample moved all of them. */}
      {client?.enabled ? <LoggingAuthority client={client} unavailable={stale} /> : <span className="remote-session-label">
        <span role="alert" className="remote-session-unavailable">{stale ? t('remote.applicationUnavailable') + ' ' : null}</span>
        <span role="status">{t('monitor.observer')}</span>
      </span>}
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
      {display && <button type="button" className="remote-button" onClick={() => {
        display.change(display.presentation === 'quick' ? 'full' : 'quick')
        setExpanded(false)
      }}>{display.presentation === 'quick' ? t('remote.quick.full') : t('remote.quick.name')}</button>}
      {/* Browser-local and notify-only. Lives in this folded panel so turning it on moves no cockpit. */}
      {alerts && <div className="remote-need-alerts">
        <p>{!alerts.supported ? t('remote.b3.needAlertsUnsupported') : alerts.blocked ? t('remote.b3.needAlertsBlocked') : t('remote.b3.needAlertsNote')}</p>
        {alerts.supported && !alerts.blocked && <button type="button" className="remote-button" aria-pressed={alerts.enabled} onClick={alerts.toggle}>
          {alerts.enabled ? t('remote.b3.needAlertsOn') : t('remote.b3.needAlertsOff')}</button>}
      </div>}
    </div>
  </div>
}
