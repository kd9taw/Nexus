import { t } from '../i18n'

/** The name this browser gives itself when it asks a station to approve it: the browser and the
 *  system it runs on, read from the user agent, so Nexus at the shack can ask "Let Chrome on Windows
 *  stream this station?" without the operator typing a name first. A name is a label, never evidence:
 *  anyone signed in to the account could send any name, and the key shown beside it on both screens
 *  is the check. The product names are invariant tokens; only the joining words are translated.
 *  Order matters: Edge and Opera carry "Chrome" in their agent, Chrome carries "Safari", and Android
 *  carries "Linux". */
export function browserLabel(agent: string = typeof navigator === 'undefined' ? '' : navigator.userAgent): string {
  const browser = /\bEdg(?:e|A|iOS)?\//.test(agent) ? 'Edge'
    : /\bOPR\//.test(agent) ? 'Opera'
    : /\b(?:Firefox|FxiOS)\//.test(agent) ? 'Firefox'
    : /\b(?:Chrome|CriOS|HeadlessChrome)\//.test(agent) ? 'Chrome'
    : /\bSafari\//.test(agent) ? 'Safari'
    : null
  const system = /Windows/.test(agent) ? 'Windows'
    : /Android/.test(agent) ? 'Android'
    : /iPhone|iPad|iPod/.test(agent) ? 'iOS'
    : /Macintosh|Mac OS X/.test(agent) ? 'macOS'
    : /CrOS/.test(agent) ? 'ChromeOS'
    : /Linux/.test(agent) ? 'Linux'
    : null
  if (browser && system) return t('remote.browserLabel', { browser, system })
  return browser ?? t('remote.browserLabelUnknown')
}
