import { useId, useState } from 'react'
import { Info } from 'lucide-react'
import { t } from '../i18n'
import type { OperationClient } from './operation-client'
import { LoggingAuthority } from './operations'

// Presentation belongs to this browser. Expanding help cannot acquire, release
// or replace station authority, and never remounts the underlying Nexus app.
export function SessionStatus({ client, stale, disconnect }: {
  client?: OperationClient | null; stale: boolean; disconnect: () => void
}) {
  const [expanded, setExpanded] = useState(false)
  const id = useId()
  return <div className="remote-application-status" role="region" aria-label={t('remote.browserWorkspace')}>
    <div className="remote-session-row">
      {client?.enabled ? <LoggingAuthority client={client} /> : <span role="status">{t('monitor.observer')}</span>}
      <button type="button" className="remote-button remote-session-toggle"
        aria-label={t('remote.settingsLegend')} title={t('remote.settingsLegend')}
        aria-expanded={expanded} aria-controls={id} onClick={() => setExpanded(value => !value)}>
        <Info size={20} aria-hidden="true" />
        <span>{t('remote.settingsLegend')}</span>
      </button>
    </div>
    {stale && <p className="remote-session-unavailable" role="alert">{t('remote.applicationUnavailable')}</p>}
    <div className="remote-session-info" id={id} hidden={!expanded}>
      <strong>{t('remote.browserWorkspace')}</strong>
      <p>{(client?.operationVersion ?? 0) >= 2 ? t('remote.controlPreview')
        : client?.enabled ? t('remote.applicationLoggingPreview') : t('remote.applicationObserver')}</p>
      <button type="button" className="remote-button" onClick={disconnect}>{t('remote.disconnect')}</button>
    </div>
  </div>
}
