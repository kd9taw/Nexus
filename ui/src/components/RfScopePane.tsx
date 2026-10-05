// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts): its words are the pane's
// title, from the catalog. Everything else on screen is PhoneScope's.
//
// THE RF SCOPE PANE — the radio's own panadapter as an opt-in pane in the digital cockpits.
//
// The operator's pick (2026-10-03): "Yes, opt-in pane". An RF scope pane, OFF by default, in the FT,
// JS8, RTTY, PSK and SSTV cockpits, with the audio waterfall staying the default. On an IC-9700 in FT8
// (the default mode) the rig's scope was never drawn at all: a data mode kept the CI-V scope stream
// off, because the cockpit's waterfall is the 0-4000 Hz audio FFT and an RF row is not that picture.
//
// IT IS PhoneScope, drawing through the spectrum renderer, with `feed="rf"`: the radio's panadapter
// and nothing else (Icom over CI-V, FlexRadio, the FT-710's RF scope). While none streams it says so
// rather than drawing the audio FFT a second time beside the waterfall. It draws the radio's whole
// sweep (the span is the radio's own) with the dial marked, and it is display only: no click tunes the
// radio from here, so nothing in a data mode moves the dial by accident.
//
// ITS OVERLAYS (`spectrum/overlays.ts`) are display only too: the licence-class band edges for the live
// section, and in the FT cockpit the newest slot's FT8/FT4 decodes and the RX/TX offsets at dial ±
// offset. No tag here is a click target.
//
// ITS POLL IS ITS REQUEST (api.ts `getRfFrame`): in a data mode the station streams the Icom scope only
// while this pane is on screen and polling. Hidden, kept alive behind another screen (`active` false),
// or in a minimised window, it stops asking and the stream stops two seconds later. That stream shares
// the CAT link with the PTT of every over, which is why it never runs for nobody.
//
// UNDER THE STOP LINE (features/panelState.ts) it is the plainest kind of entry: it hosts no control
// that stops a transmission and none that starts one, and hiding it ends nothing in flight, so it
// carries no ⊞ note. Its size is its host's: a fill frame wherever the cockpit puts it, with the
// comfortable minimum on its content wrapper (styles.css `.pane-body > .rf-scope`).
import { t } from '../i18n'
import type { FtOverlay } from '../spectrum/overlays'
import { CockpitPaneFrame } from './panes/CockpitPaneFrame'
import { PhoneScope } from './PhoneScope'

/** The radio's whole sweep: PhoneScope's view window clamps to the row, so these take all of it. */
const WHOLE_SWEEP_LO = -1e9
const WHOLE_SWEEP_HI = 1e9

export function RfScopePane({
  closeProps,
  dialMhz,
  keyed,
  theme,
  active,
  privilegeMode,
  ft,
}: {
  /** The pane's own ✕ — the cockpit's `panelHost.closeProps('rfScope')`, the same act as the tick. */
  closeProps: { onRemove?: () => void; hideNote?: string }
  /** The live dial, MHz (0 = unknown): the RF axis is anchored on it and the DIAL line drawn there. */
  dialMhz: number
  /** The cockpit's own keyed state: the picture's range is held still while the station transmits. */
  keyed: boolean
  theme: string
  /** False while the cockpit is kept alive behind another screen: no poll, so no request. */
  active: boolean
  /** The live operating section (the snapshot's `radio.operatingMode`), for its licence-class band edges. */
  privilegeMode?: string
  /** The FT cockpit's newest decodes and its RX/TX offsets (`spectrum/overlays.ts` `ftOverlay`). */
  ft?: FtOverlay | null
}) {
  return (
    <CockpitPaneFrame title={t('rfScope.title')} paneId="rfScope" {...closeProps}>
      <div className="rf-scope">
        <PhoneScope
          feed="rf"
          cockpit="rfpan"
          hideSmeter
          transmitting={keyed}
          theme={theme}
          active={active}
          viewLoHz={WHOLE_SWEEP_LO}
          viewHiHz={WHOLE_SWEEP_HI}
          dialHz={dialMhz > 0 ? Math.round(dialMhz * 1e6) : null}
          privilegeMode={privilegeMode}
          ft={ft}
        />
      </div>
    </CockpitPaneFrame>
  )
}
