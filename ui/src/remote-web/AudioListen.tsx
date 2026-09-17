import { useEffect, useSyncExternalStore } from 'react'
import { Headphones, VolumeX } from 'lucide-react'
import { t } from '../i18n'
import type { AudioLink, AudioPhase } from './audio-listen'
import type { OperationClient } from './operation-client'

/** Muted by default, always. A page that starts making noise when it opens is hostile:
 *  an operator may be at work, on a call, or on a phone in a hotel lobby, and the only
 *  safe assumption is that nobody asked for sound yet. */
export function AudioListen({ audio, client }: { audio: AudioLink; client?: OperationClient | null }) {
  const view = useSyncExternalStore(audio.subscribe, audio.getSnapshot)
  const state = useSyncExternalStore(client?.subscribe ?? idleSubscribe, client?.getSnapshot ?? idleView)
  const station = state?.state
  // The station said whether it can do this at all. An older station never advertises the
  // hint, so its operator is never shown a control it could not honour - and an older
  // browser never knew the hint's name, so the negotiation holds from both ends.
  const offered = !!station?.controls?.capabilities.includes('audioListen')
  const lease = station?.phase === 'controlling' ? station.leaseId : null
  // `ended` is deliberately NOT "on": the audio has stopped, so the button has to go
  // back to offering a start. The reason why it stopped is said beside it, not instead
  // of it - an operator who is told only that it ended has nothing to press.
  const on = view.phase === 'connecting' || view.phase === 'live' || view.phase === 'gap' || view.phase === 'stalled'

  // Listening is given under station control, so losing control ends it. The station
  // stops on its own re-check too; this is the half that stops the SOUND at once rather
  // than up to a second later, which is the half the operator actually hears.
  useEffect(() => {
    if (on && !lease) audio.release('notController')
  }, [on, lease, audio])
  // Unmount RELEASES; it never closes. The link is one per connection and outlives this
  // control: the workspace host renders this control in its pre-boot shell and again, as
  // a fresh mount, inside the booted workspace. A close() here made the first unmount
  // permanent, and the remounted button was enabled and sent nothing for the rest of the
  // session. The device is freed by HostedConnection.stop(), where the connection ends.
  useEffect(() => () => { audio.release() }, [audio])

  if (!offered) return null
  if (!view.supported) return <span className="remote-audio remote-audio--off" role="note">{t('remote.audio.unsupported')}</span>

  return <span className="remote-audio">
    <button type="button" className="remote-button remote-audio-toggle"
      aria-pressed={on} disabled={!on && !lease}
      onClick={() => { if (on) audio.release(); else if (lease) audio.listen(lease) }}>
      {on ? <Headphones size={18} aria-hidden="true" /> : <VolumeX size={18} aria-hidden="true" />}
      <span>{on ? t('remote.audio.stop') : t('remote.audio.start')}</span>
    </button>
    {/* The state is its own line and it never disappears while listening: "connecting",
        "live", "gap" and "stalled" are four different things to do about it, and a
        control that only said on/off would make a dead link look like a dead band. */}
    {on && <span className={`remote-audio-state remote-audio-state--${view.phase}`} role="status">{caption(view.phase)}</span>}
    {view.phase === 'ended' && <span className="remote-audio-state remote-audio-state--ended" role="alert">{ended(view.reason)}</span>}
  </span>
}

function caption(phase: AudioPhase): string {
  return phase === 'connecting' ? t('remote.audio.connecting')
    : phase === 'gap' ? t('remote.audio.gap')
    : phase === 'stalled' ? t('remote.audio.stalled')
    : t('remote.audio.live')
}
/** Written out rather than looked up in a map: the catalog's orphan check scans for
 *  literal t() calls, so a dynamic lookup reads as "nobody uses these keys". */
function ended(reason: string | null): string {
  return reason === 'sourceChanged' ? t('remote.audio.sourceChanged')
    : reason === 'audioInUse' ? t('remote.audio.inUse')
    : reason === 'notController' ? t('remote.audio.notController')
    : reason === 'audioStopped' ? t('remote.audio.stopped')
    : t('remote.audio.unavailable')
}

const idleSubscribe = () => () => {}
const idleView = () => undefined
