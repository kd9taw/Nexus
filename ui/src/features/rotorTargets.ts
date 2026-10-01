// THE ROTOR PANE'S PENDING TARGETS — the pure half. An az/el rotator is pointed one axis at a time
// from the pane (a bearing, then an elevation), and rotctld's `P` always carries both, so each
// move has to say what the OTHER axis should do. The backend keeps an axis it is not given where
// the rotator reports it (`point_keeping`); the pane hands over the other axis's target only while
// the mast is still on its way to it. Otherwise typing 200° and then 30° would stop the first slew
// wherever the mast was when the second one went out.
//
// ⚠️ A TARGET MUST NOT OUTLIVE THE SLEW. Handed over after the mast had arrived, or after something
// else had moved it (STOP, a satellite pass, ↗ from another screen, the controller's own buttons),
// it would pull the mast back to where the operator last sent it from this pane — moving a mast
// nobody asked to move. So a target is dropped when the mast arrives, and when two readings in a
// row bring it no closer (stopped, stuck, or heading somewhere else). Degrees throughout.

/** A commanded position one axis is still on its way to. `gap` is how far off the last reading
 *  was, `stalls` how many readings in a row have not brought it closer. */
export interface Pending {
  deg: number
  gap: number | null
  stalls: number
}

/** Within this of the target, the mast has arrived — the pane's "→" indicator uses the same 2°. */
export const ARRIVED_DEG = 2
/** Readings in a row that bring the mast no closer before the target is dropped (a 2 s poll: 4 s,
 *  which a G-5500 moving at all covers several degrees in). */
export const STALLS_TO_DROP = 2

/** A newly commanded target. */
export function pending(deg: number): Pending {
  return { deg, gap: null, stalls: 0 }
}

/** The gap between two positions: the short way round for an azimuth, straight for an elevation. */
function gap(a: number, b: number, wraps: boolean): number {
  if (!wraps) return Math.abs(a - b)
  const d = (((a - b) % 360) + 360) % 360
  return d > 180 ? 360 - d : d
}

/** Follow a pending target through one reading of its axis: kept while the mast closes on it,
 *  dropped on arrival or after STALLS_TO_DROP readings that do not bring it closer. With no
 *  reading at all there is nothing to judge by, so it is kept: on a rotator that cannot report,
 *  it is the only knowledge of where the mast was sent. */
export function follow(p: Pending | null, reported: number | null, wraps: boolean): Pending | null {
  if (p == null || reported == null) return p
  const g = gap(p.deg, reported, wraps)
  if (g <= ARRIVED_DEG) return null
  const closer = p.gap == null || g < p.gap - 0.5
  const stalls = closer ? 0 : p.stalls + 1
  return stalls >= STALLS_TO_DROP ? null : { deg: p.deg, gap: g, stalls }
}
