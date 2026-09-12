import { useStationStopControl } from '../stationAccess'

/** The existing FT stop, also reachable while a Remote log dialog is open.
 * Its authority is independent of stale readings and pending log receipts. */
export function FtStopControl({ onHaltTx }: { onHaltTx?: () => void }) {
  const allowed = useStationStopControl()
  return <button disabled={!allowed}
    type="button"
    className="op-btn stop"
    data-remote-stop={allowed || undefined}
    onClick={() => onHaltTx?.()}
    title="Stop transmitting immediately — cuts even an over already in flight"
  >Stop TX</button>
}
