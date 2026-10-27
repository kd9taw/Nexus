// THE CW TURN LAMP — the first thing on the CW dock's send row: whose turn it is, in a glyph and two
// or three words (`features/cwTurn.ts` draws it from the station's turn read). A READOUT, never a
// control: it has no click, and nothing it shows sends, holds or arms anything. Like the TX meters
// above it in the dock it has a ⊞ id (`turnCue`), so one tick hides it for good.
//
// Its slot is a fixed share of the row, so its words never move His Call or Send; at a narrow
// window the words are cut and the glyph stays (`.cw-turn` in styles.css). It is not a live region:
// a screen reader hears a turn through the announcement bus, at most once per over, and the lamp's
// text is there to be read on demand.
//
// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts): every word comes from the catalog.
import type { CSSProperties } from 'react'
import { t } from '../i18n'
import type { Lamp } from '../features/cwTurn'

/** The id the lit F-key's description points at (`aria-describedby`): "Suggested now". */
export const CW_TURN_SUGGEST_ID = 'cw-turn-suggest'

export function CwTurnLamp({ lamp, window }: { lamp: Lamp; window: { total: number; left: number } | null }) {
  const left = window && window.total > 0 ? Math.max(0, Math.min(1, window.left / window.total)) : null
  return (
    <div className="cw-turn" data-turn={lamp.state} title={lamp.title}>
      <span className="cw-turn-glyph" aria-hidden="true">
        {lamp.glyph}
      </span>
      <span className="cw-turn-label">{lamp.label}</span>
      {left != null && (
        // What is left of the window, as a ring that empties: steps with the read, never animates.
        <span className="cw-turn-ring" aria-hidden="true" style={{ '--cw-turn-left': left } as CSSProperties} />
      )}
      <span id={CW_TURN_SUGGEST_ID} className="sr-only">
        {t('cw.turn.suggested')}
      </span>
    </div>
  )
}
