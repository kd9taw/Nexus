// THE WINDOW'S TRANSMIT SURFACES, which nothing drawn over the cockpit may cover: the call card
// (CallCard.tsx) and the pop-up column (Toasts.tsx, features/toastRoom.ts) keep off them.

/** Every transmit surface: the TX strip under each cockpit's scope, the TX dock, the dock's keep row,
 *  FT's QSO strip (its Stop TX and sequencer), SSTV's pinned transmit bar, and any Stop button wherever
 *  it is drawn (the top bar's cluster, the log confirm). A hidden keep-alive cockpit's copies have no
 *  box and drop out.
 *
 *  THE KEEP ROW IS NAMED ON ITS OWN. Where the dock does not fit it folds (cockpit-panes.css
 *  `.cockpit-txdock[data-fold='on']`): the dock becomes `display: contents` and has NO box, so measuring it
 *  finds nothing, and only its keep row — Send, PTT, Esc/Stop and the TX latch — stays parked at the foot.
 *  Unfolded, the row is inside the dock's box and naming it changes nothing. Its own box, not the boxes of
 *  every row of a folded dock: the other rows then scroll with the panes, and keeping off them would leave a
 *  pop-up no room at the end of the scroll. */
export const TX_AVOID = '.cockpit-txstrip, .cockpit-txdock, .cockpit-txdock-keep, .cockpit-qso, .sstv-tx-bar, .op-controls, .op-btn.stop'
