// The stream page's layout for the window it has (the operator's pick, 2026-10-03: the phone layout). A phone held
// upright gets a bar above the picture for Stop TX and a bar under it for the thumbs; turned on its side it gets a
// rail of controls beside the picture, which then has the window's whole height; every other window keeps the header
// row the page has always had. It is decided from the effective size useViewport publishes (`--vw-eff` × `--vh-eff`),
// because the width class alone cannot say it: a phone on its side is `sm` by width, as a 1024 × 768 window is.
import { classifyViewport } from '../useViewport'

export type StreamLayout = 'header' | 'rail' | 'bars'

/** A window on its side and shorter than this takes the rail. A phone turned on its side is 320-430 px tall, less
 *  under its browser's own bar, and a header there took 42% of it (174 of 412 px, measured 2026-10-03); a browser
 *  on the smallest supported screen (768 px) still has about 650, where the header row is the better use of it. */
export const RAIL_BELOW_HEIGHT = 500

export function streamLayout(width: number, height: number): StreamLayout {
  if (width > height && height < RAIL_BELOW_HEIGHT) return 'rail'
  if (width <= height && classifyViewport(width) === 'xs') return 'bars'
  return 'header'
}
