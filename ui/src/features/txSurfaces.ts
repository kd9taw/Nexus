// THE WINDOW'S TRANSMIT SURFACES, which nothing drawn over the cockpit may cover: the call card
// (CallCard.tsx) and the pop-up column (Toasts.tsx, features/toastRoom.ts) keep off them.

/** Every transmit surface: the TX strip under each cockpit's scope, the TX dock, FT's QSO strip (its
 *  Stop TX and sequencer), SSTV's pinned transmit bar, and any Stop button wherever it is drawn (the
 *  top bar's cluster, the log confirm). A hidden keep-alive cockpit's copies have no box and drop out. */
export const TX_AVOID = '.cockpit-txstrip, .cockpit-txdock, .cockpit-qso, .sstv-tx-bar, .op-controls, .op-btn.stop'
