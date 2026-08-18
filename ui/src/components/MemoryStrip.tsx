// The cockpit quick-recall strip — the ★-favorite memories as a compact,
// SINGLE-LINE, side-scrolling row of chips in the cockpit header. One click
// recalls (the host owns the retune + auto-section-switch via App's
// recallMemory); "＋" saves the current dial as a favorite; "≡" opens the
// full Memories section. Replaces the old MemoryBank header list, whose
// unbounded inline column grew the top bar with every save (the reported
// scroll bug).
//
// The row is ONE line (flex-wrap: nowrap) and that IS the header-height bound —
// never let it become a column again. Bounded sideways too: only the first
// STRIP_FAVORITE_LIMIT favorites get a chip, because past that they sit off the
// right edge of a header-width row, unreadable and (past Ctrl+1..9) unreachable
// by keyboard. The surplus is COUNTED on ≡, which is the button that opens the
// one place they can be re-ranked. ＋ therefore saves at rank 1: appended, a new
// star with a full strip would be saved and invisible — the same silent "＋ did
// nothing" the star-the-existing branch below exists to prevent.
import {
  memoriesStore,
  saveFavoriteFromDial,
  STRIP_FAVORITE_LIMIT,
  useMemories,
  type Memory,
  type MemoryKind,
} from '../features/memories'
import { modChord } from '../platform'

export interface MemoryStripProps {
  /** Current dial (MHz) — what "＋ Save" captures + the active-chip highlight. */
  dialMhz: number
  /** Current mode string (USB / LSB / FM / CW / FT8 …) captured with the dial. */
  mode: string
  /** Recall a memory (App's recallMemory: settings + retune + section switch). */
  onRecall?: (m: Memory) => void
  /** Open the full Memories section (manage, groups, import/export). */
  onManage?: () => void
}

// Dial-match tolerance for the active-chip highlight (mirrors FrequencyControl).
const MATCH_EPS = 0.0005

/** A sensible kind for a channel saved straight off the dial. */
function kindForMode(mode: string): MemoryKind {
  const u = mode.toUpperCase()
  if (u === 'FM' || u === 'NFM') return 'simplex'
  if (u === 'FT8' || u === 'FT4') return 'digital'
  return 'other'
}

export function MemoryStrip({ dialMhz, mode, onRecall, onManage }: MemoryStripProps) {
  const bank = useMemories()
  const favorites = bank.memories.filter((m) => m.favorite)
  // Master-array order — the same order hotkeyRecallTarget counts and the ★ view ranks.
  const shown = favorites.slice(0, STRIP_FAVORITE_LIMIT)
  const overflow = favorites.length - shown.length

  const saveCurrent = () => {
    // Idempotent + always-visible: saving the same freq+mode twice never piles duplicates,
    // and if a matching NON-favorite already exists we star it so a chip always appears
    // (no silent "＋ did nothing"). Either way it lands at rank 1, which is what keeps the
    // new chip inside the cap when the strip is already full.
    memoriesStore.update(
      (b) => saveFavoriteFromDial(b, { rxMhz: dialMhz, mode, kind: kindForMode(mode) }).bank,
    )
  }

  return (
    <div className="mem-strip" role="group" aria-label="Memory quick recall">
      <span className="mem-strip-label" title="Memory quick recall — your ★-starred memories">
        MEM
      </span>
      <button
        type="button"
        className="mem-strip-save"
        onClick={saveCurrent}
        title={`Save ${dialMhz.toFixed(3)} ${mode} as a favorite memory`}
      >
        ＋
      </button>
      {shown.map((m, i) => {
        const active = Math.abs(m.rxMhz - dialMhz) < MATCH_EPS
        // The first 9 favorites are recallable from any section via Ctrl/⌘+1..9 (App's
        // global hotkey); surface it in the tooltip so it's discoverable — in the
        // platform's own vocabulary (Ctrl on a Mac is the Spaces chord, so it reads ⌘).
        const hotkey = i < 9 ? ` · ${modChord(i + 1)}` : ''
        return (
          <button
            key={m.id}
            type="button"
            className={`mem-chip${active ? ' active' : ''}`}
            onClick={() => onRecall?.(m)}
            title={`${m.name} — ${m.rxMhz.toFixed(4)} MHz ${m.mode}${
              m.ctcssEncHz ? ` · tone ${m.ctcssEncHz.toFixed(1)}` : ''
            } (click to tune${hotkey})`}
          >
            {m.name}
          </button>
        )
      })}
      {onManage && (
        <button
          type="button"
          className="mem-strip-manage"
          onClick={onManage}
          title={
            overflow > 0
              ? `Open Memories — ${overflow} more favorite${overflow === 1 ? '' : 's'} past the ${STRIP_FAVORITE_LIMIT} this strip shows. Re-rank them with ▲▼ under ★ Favorites.`
              : 'Open Memories — manage channels, groups, nets, and CHIRP import/export'
          }
        >
          ≡{overflow > 0 && <span className="mem-strip-more">{overflow}</span>}
        </button>
      )}
    </div>
  )
}
