// "Where your contacts go": the logbook services, inside the first-run wizard's log step, for an
// operator switching from another program (the Experienced answer, `settings/journeys.ts`).
//
// It READS and it TESTS, nothing more. Each service that sends contacts outward is listed as
// Settings ▸ Connections lists it (`settings/connHealth.ts`: the dot, the account, the state), and
// QRZ Logbook keeps its Test (`qrz_test_connection`, a status round trip that inserts nothing). No
// login is typed here and nothing is uploaded: logins live in Settings ▸ Confirmations, beside each
// service's upload switch, and the line under the heading says what saving one there sends. The
// one choice here is the operator's own: open those settings when the wizard finishes.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The service names and the
// account beside each are data from the backend, shown as they are.
import { useEffect, useState } from 'react'
import { getCredentialsStatus, qrzTestConnection } from '../api'
import { t } from '../i18n'
import { T } from '../i18n/T'
import { connState, dotClass, stateLabel, whenText } from '../settings/connHealth'
import type { CredStatus } from '../types'

export function WizardServices({
  openAfter,
  onOpenAfter,
}: {
  /** Open Settings ▸ Confirmations once the wizard finishes. */
  openAfter: boolean
  /** Omitted ⇒ the host cannot open Settings afterwards, and no offer is shown. */
  onOpenAfter?: (open: boolean) => void
}) {
  const [creds, setCreds] = useState<CredStatus[] | null>(null)
  const [qrzTest, setQrzTest] = useState<{ state: 'idle' | 'testing' | 'ok' | 'fail'; msg: string }>({
    state: 'idle',
    msg: '',
  })
  useEffect(() => {
    let live = true
    getCredentialsStatus()
      .then((c) => live && setCreds(c))
      .catch(() => live && setCreds([]))
    return () => {
      live = false
    }
  }, [])
  const runQrzTest = () => {
    setQrzTest({ state: 'testing', msg: '' })
    qrzTestConnection()
      .then((msg) => setQrzTest({ state: 'ok', msg }))
      .catch((e) => setQrzTest({ state: 'fail', msg: String(e) }))
  }
  // Only the services contacts go to; a lookup-only one (the QRZ callbook) has nothing to send.
  const outward = (creds ?? []).filter((c) => c.uploads)

  return (
    <div className="wizard-services">
      <h3 className="wizard-modes-title">{t('setup.services.title')}</h3>
      <p className="wizard-license-sub">{t('setup.services.sub')}</p>
      <div className="conn-status-grid">
        {outward.map((c) => {
          const state = connState(c)
          const dot = dotClass(state)
          const when = whenText(c, state)
          return (
            <div key={c.id} className="conn-status-row">
              <span className={`conn-dot ${dot}`} aria-hidden="true" />
              <span className="conn-name">{c.connector}</span>
              <span className="conn-id">{c.identity || '—'}</span>
              <span className={`conn-state ${dot}`}>{stateLabel(state)}</span>
              {when && <span className="conn-when">{when}</span>}
              {c.id === 'qrz-logbook' && (
                <button
                  type="button"
                  className="settings-test-btn"
                  onClick={runQrzTest}
                  disabled={qrzTest.state === 'testing'}
                  title={t('settings.connections.qrz.test.title')}
                >
                  {qrzTest.state === 'testing'
                    ? t('settings.connections.test.busy')
                    : t('settings.connections.qrz.test.action')}
                </button>
              )}
            </div>
          )
        })}
      </div>
      {qrzTest.state === 'ok' && (
        <p className="conn-test-result ok" role="status">
          {t('settings.connections.qrz.test.ok', { detail: qrzTest.msg })}
        </p>
      )}
      {qrzTest.state === 'fail' && (
        <p className="conn-test-result fail" role="status">
          <T k="settings.connections.qrz.test.fail" tags={{ b: <strong /> }} vals={{ detail: qrzTest.msg }} />
        </p>
      )}
      {onOpenAfter && (
        <div className="wizard-modes">
          <button
            type="button"
            className={`wizard-mode${openAfter ? ' sel' : ''}`}
            aria-pressed={openAfter}
            onClick={() => onOpenAfter(!openAfter)}
          >
            <span className="wizard-mode-label">{t('setup.services.openAfter.label')}</span>
            <span className="wizard-mode-blurb">{t('setup.services.openAfter.blurb')}</span>
          </button>
        </div>
      )}
    </div>
  )
}
