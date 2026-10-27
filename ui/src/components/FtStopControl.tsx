/** The FT stop: Stop TX, which cuts even an over already in flight. */
export function FtStopControl({ onHaltTx }: { onHaltTx?: () => void }) {
  return (
    <button
      type="button"
      className="op-btn stop"
      onClick={() => onHaltTx?.()}
      title="Stop transmitting immediately — cuts even an over already in flight"
    >Stop TX</button>
  )
}
