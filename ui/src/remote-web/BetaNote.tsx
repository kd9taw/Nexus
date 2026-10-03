import { t } from '../i18n'

/** Remote streaming is a beta, said in words wherever it is entered (the operator, 2026-10-02: "we
 *  need a clear warning that this is a beta feature in nexus and access could be revoked at any
 *  time"): every station card on the Remote site, the stream page, and the Remote access card in
 *  Nexus at the shack. A visible mark and one plain line, never behind a click or a tooltip. */
export function BetaNote({ className }: { className?: string }) {
  return <p className={className ? `remote-beta ${className}` : 'remote-beta'} role="note">
    <span className="remote-beta-mark">{t('remote.beta.mark')}</span> {t('remote.beta.notice')}
  </p>
}
