// THE STATION CLOCK — HH:MM:SS, ticking once a second: UTC by default, this computer's local time
// with `local` (#253, the optional second clock, Settings ▸ Workspace). The top bar's clock, in a module
// of its own (2026-10-01) so Connect's header can carry the same one: the top bar left Connect with its
// radio controls, and the operator kept the time ("things like time are very good"). One clock, so the
// two places can never read differently.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). `UTC` is a token, the same on
// every ham's screen; the local label and both hovers come from the catalog.
import { useEffect, useState } from 'react'
import { t } from '../i18n'

const UTC = 'UTC'

/** Live clock (HH:MM:SS), ticking once a second. UTC by default; `local` shows this computer's
 *  local time instead (#253, the optional second clock) with the same look, so the two read as a
 *  pair and UTC keeps its place. */
export function UtcClock({ local = false }: { local?: boolean }) {
  const [now, setNow] = useState(() => new Date())
  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 1000)
    return () => window.clearInterval(id)
  }, [])
  const p = (n: number) => String(n).padStart(2, '0')
  const hhmmss = local
    ? `${p(now.getHours())}:${p(now.getMinutes())}:${p(now.getSeconds())}`
    : `${p(now.getUTCHours())}:${p(now.getUTCMinutes())}:${p(now.getUTCSeconds())}`
  return (
    <div
      className={`utc-clock${local ? ' local-clock' : ''}`}
      title={local ? t('topbar.localClock.title') : t('topbar.utc.title')}
    >
      <span className="utc-time">{hhmmss}</span>
      <span className="utc-label">{local ? t('topbar.localClock.label') : UTC}</span>
    </div>
  )
}
