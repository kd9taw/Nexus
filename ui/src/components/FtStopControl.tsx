import { useStationStopControl, useStationStopProgress } from '../stationAccess'
import { t } from '../i18n'
import type { RadioStatus } from '../types'

/** The existing FT stop, also reachable while a Remote log dialog is open.
 * Its authority is independent of stale readings and pending log receipts.
 *
 * ⚠️ A REMOTE STOP IS ANSWERED ON ACCEPTANCE, NOT ON RF — when the station's Engine is held the
 * halt runs afterwards on its own thread. So the state beside the button reads "stop sent" until
 * the station's own `radio` reading shows the transmitter free, and "stopped" only then. Without a
 * reading (`radio` omitted, as in the log dialog) it stays "stop sent": never an assertion nobody
 * made. Locally the call IS the act and nothing is shown. */
export function FtStopControl({ onHaltTx, radio }: { onHaltTx?: () => void; radio?: RadioStatus }) {
  const allowed = useStationStopControl()
  const progress = useStationStopProgress(radio)
  return <>
    <button disabled={!allowed}
      type="button"
      className="op-btn stop"
      data-remote-stop={allowed || undefined}
      onClick={() => onHaltTx?.()}
      title="Stop transmitting immediately — cuts even an over already in flight"
    >Stop TX</button>
    {progress !== 'idle' && (
      <span className={`cockpit-stopstate${progress === 'stopped' ? ' done' : ''}`} role="status"
        title={progress === 'stopped' ? t('remote.stop.stopped.title') : progress === 'sent' ? t('remote.stop.sent.title') : undefined}>
        {progress === 'stopped' ? t('remote.stop.stopped') : progress === 'sent' ? t('remote.stop.sent') : t('remote.stop.sending')}
      </span>
    )}
  </>
}
