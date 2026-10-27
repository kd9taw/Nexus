// The Remote site's first visit and its empty state. The landing carries the same four sections as
// hamradiotools.io/remote, in short, and links there for the long version; keep the two in step.
import { t } from '../i18n'
import { T } from '../i18n/T'
import { BetaNote } from './BetaNote'

// Tokens, the same in every language: the address, the log file and its folder, the stream page's
// Stop TX, and the site's pages.
const CONTACT = 'contact@hamradiotools.io'
const DIAG_LOG = 'nexus-diag.log'
const DIAG_FOLDER = '%LOCALAPPDATA%\\Nexus'
const STOP_TX = 'Stop TX'
const SITE_REMOTE = 'https://hamradiotools.io/remote'
const SITE_HELP = 'https://hamradiotools.io/remote/help'

export function RemoteLanding({ betaChannel }: { betaChannel: string }) {
  const out = { target: '_blank', rel: 'noopener noreferrer' }
  return <>
    <p className="remote-site-status" role="note">{t('remote.landing.free')}</p>
    <BetaNote />
    <div className="remote-site-guide">
      <section className="rm-card remote-section">
        <h2>{t('remote.landing.start.title')}</h2>
        <ol>
          <li>{t('remote.landing.start.shack')}</li>
          <li>{t('remote.landing.start.turnOn')}</li>
          <li>{t('remote.landing.start.here')}</li>
          <li>{t('remote.landing.start.approve')}</li>
        </ol>
      </section>
      <section className="rm-card remote-section">
        <h2>{t('remote.landing.works.title')}</h2>
        <ul>
          <li>{t('remote.landing.works.stream')}</li>
          <li>{t('remote.landing.works.listen')}</li>
          <li>{t('remote.landing.works.phone', { stop: STOP_TX })}</li>
        </ul>
        <h3>{t('remote.landing.limits.title')}</h3>
        <ul>
          <li>{t('remote.landing.limits.windows')}</li>
          <li>{t('remote.landing.limits.apps')}</li>
          <li>{t('remote.landing.limits.idle')}</li>
        </ul>
      </section>
      <section className="rm-card remote-section">
        <h2>{t('remote.landing.safety.title')}</h2>
        <ul>
          <li>{t('remote.landing.safety.shack')}</li>
          <li>{t('remote.landing.safety.approve')}</li>
          <li>{t('remote.landing.safety.stop', { stop: STOP_TX })}</li>
          <li>{t('remote.landing.safety.drop')}</li>
          <li>{t('remote.landing.safety.licence')}</li>
        </ul>
      </section>
      <section className="rm-card remote-section">
        <h2>{t('remote.landing.help.title')}</h2>
        <p><T k="remote.landing.help.email" vals={{ email: CONTACT }}
          tags={{ mail: <a href={`mailto:${CONTACT}`} />, chat: <a href={betaChannel} {...out} /> }} /></p>
        <p><T k="remote.landing.help.log" vals={{ file: DIAG_LOG, folder: DIAG_FOLDER }} tags={{ code: <code /> }} /></p>
        <p className="remote-site-links">
          <a href={SITE_REMOTE} {...out}>{t('remote.landing.help.more')}</a>
          <a href={SITE_HELP} {...out}>{t('remote.landing.help.trouble')}</a>
        </p>
      </section>
    </div>
  </>
}

/** Signed in, with no station yet: the one next step is at the shack. */
export function AddStation() {
  return <section className="rm-card remote-section remote-site-card--action">
    <h2>{t('remote.empty.title')}</h2>
    <ol>
      <li>{t('remote.empty.open')}</li>
      <li>{t('remote.empty.signIn')}</li>
      <li>{t('remote.empty.turnOn')}</li>
    </ol>
    <p>{t('remote.empty.after')}</p>
  </section>
}
