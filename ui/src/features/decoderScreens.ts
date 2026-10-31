// THE RECEIVE DECODERS A SCREEN STARTED, AND WHEN THEY STOP.
//
// Opening RTTY, PSK, JTTY, SSTV or APRS starts that mode's decoder (each cockpit's `*AutoArm` on its
// rising `active` edge): a receive screen with a dead receiver was the field bug. Nothing stopped
// them again, so touring the five and going back to FT8 left five decoders running on the receive
// audio all session. This decides when the operator has LEFT one, so App can tell the engine
// (`leave_decoder_screen`, which stops only a decoder the screen started, and only when nothing
// else needs it).
//
// LEAVING MEANS ENTERING ANOTHER SECTION: a screen that operates (RIG_MODE_BY_VIEW: FT8, Tempo,
// JS8, Phone, CW, RTTY, PSK, JTTY) or another decoder screen. A glance at the map, the logbook,
// Settings, Satellites or the contest screen is not a section change and stops nothing, so a
// picture coming in or a contest exchange being copied survives a look at something else.
//
// Pure, so the SEQUENCE can be tested (RTTY → Map → FT8 leaves RTTY at FT8, not at Map). App
// holds the list in a ref; the engine answers whether a decoder is still held (the SSTV picture
// viewer open, the RTTY auto-sequencer on, the APRS iGate on), and App keeps that one on the list
// to ask again at the next section change.

import { RIG_MODE_BY_VIEW } from '../rigModeForView'

/** The screens whose decoder starts when they open. The wire names of `leave_decoder_screen`. */
export const DECODER_SCREENS = ['rtty', 'psk', 'jtty', 'sstv', 'aprs'] as const
export type DecoderScreen = (typeof DECODER_SCREENS)[number]

export function isDecoderScreen(view: string): view is DecoderScreen {
  return (DECODER_SCREENS as readonly string[]).includes(view)
}

/** A screen that is a section of its own: entering it leaves every other decoder screen. */
function isSection(view: string): boolean {
  return RIG_MODE_BY_VIEW[view] !== undefined || isDecoderScreen(view)
}

/**
 * Entering `view` with `held` the decoder screens visited and not yet left: which of them it
 * leaves, and what is held after. A decoder screen holds itself; a section leaves every other
 * held screen; anything else (a glance) changes nothing.
 */
export function enterView(
  held: readonly DecoderScreen[],
  view: string,
): { left: DecoderScreen[]; held: DecoderScreen[] } {
  const self = isDecoderScreen(view) ? [view] : []
  if (!isSection(view)) {
    return { left: [], held: [...held, ...self.filter((s) => !held.includes(s))] }
  }
  return { left: held.filter((s) => s !== view), held: self }
}
