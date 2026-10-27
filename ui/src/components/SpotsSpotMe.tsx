// ⚠️ ON THE MIGRATED LIST's terms: every operator-visible string is a catalog key. The comment is wire
// text (it goes to the cluster as typed), and "CQ <mode>" is that text, the same in every language.
//
// SPOT ME, ON THE SPOTS BOARD (the Spots view, Phone's and CW's Spots panes, and the Spots box in any
// cockpit): one DX cluster spot of the operator's own call on the station's dial, with a short comment,
// "CQ <mode>" until the operator edits it. The last comment typed for each mode is kept for the next
// time that mode comes round; emptying the box gives the default back. The press, the cooldown and the
// self-spot warning are the contest screen's own (`useSpotMe`), and the station holds ONE cooldown for
// both buttons (`contest_spot.rs`). Outside a contest the station posts on one press; with Contest mode
// on, the running contest's self-spot rule holds this button exactly as it holds the contest screen's.
import { useEffect, useState } from 'react'
import { clusterSelfSpot } from '../api'
import { t } from '../i18n'
import { contestLogMode, useKeyboardMode } from '../features/contestMode'
import { useSpotMe } from './ContestSpotMe'

/** The station, as much of it as the comment needs: its operating section (`radio.operatingMode`),
 *  the rig mode it commands (`radio.sideband`: USB, LSB, AM, FM…) and the digital section's tier. */
export interface SpotMeStation {
  section?: string
  /** The Keyboard section's mode on the air (`radio.keyboardMode`: PSK31, QPSK31 or JTTY). */
  keyboardMode?: string
  sideband?: string
  tier?: string
}

/** The mode a self-spot names: the section's mode as its contacts log (`contestLogMode`), and on Phone
 *  the voice mode the rig is set to, so AM and FM say so. */
export function spotMeMode(section: string | undefined, sideband: string | undefined, keyboardMode: string | null, tier?: string): string {
  const { fdMode, mode } = contestLogMode(section, keyboardMode, tier)
  if (fdMode !== 'PH') return mode
  const rig = (sideband ?? '').trim().toUpperCase()
  return /^W?FM/.test(rig) ? 'FM' : rig === 'AM' ? 'AM' : 'SSB'
}

/** The last comment typed for each mode, one record for every window. */
const COMMENTS_KEY = 'nexus.spotMe.comments'
/** The station sends at most this much of a comment (`contest_spot::MAX_COMMENT`). */
const MAX_COMMENT = 30

function storedComments(): Record<string, string> {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(COMMENTS_KEY) ?? '{}')
    if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return {}
    const kept: Record<string, string> = {}
    for (const [mode, text] of Object.entries(raw)) {
      if (typeof text === 'string' && text.trim()) kept[mode] = text.slice(0, MAX_COMMENT)
    }
    return kept
  } catch {
    return {}
  }
}

/** The comment the box opens on for `mode`: the last one typed, else CQ and the mode. */
function commentFor(mode: string): string {
  return storedComments()[mode] ?? `CQ ${mode}`
}

/** A blank comment reads back as none (`storedComments`), so an emptied box opens on the default. */
function rememberComment(mode: string, text: string) {
  try {
    localStorage.setItem(COMMENTS_KEY, JSON.stringify({ ...storedComments(), [mode]: text }))
  } catch {
    // Storage refused: the box still sends what it shows, it only forgets it.
  }
}

export function SpotsSpotMe({ section, keyboardMode, sideband, tier }: SpotMeStation) {
  const mode = spotMeMode(section, sideband, useKeyboardMode(section, keyboardMode), tier)
  const [comment, setComment] = useState(() => commentFor(mode))
  // Follow the station from mode to mode, each with its own comment.
  useEffect(() => {
    setComment(commentFor(mode))
  }, [mode])
  // Looked up at the press, never at render, as the contest screen's (ContestSpotMe).
  const spot = useSpotMe((text, confirmed) => clusterSelfSpot(text, confirmed))

  return (
    <span className="np-spot-me">
      <input
        type="text"
        className="np-spot-comment"
        aria-label={t('spotMe.comment.label')}
        value={comment}
        maxLength={MAX_COMMENT}
        spellCheck={false}
        onChange={(e) => {
          setComment(e.target.value)
          rememberComment(mode, e.target.value)
        }}
      />
      <button
        type="button"
        className="np-spot-btn"
        title={spot.title}
        disabled={spot.disabled}
        onClick={() => spot.press(comment)}
      >
        {spot.label}
      </button>
    </span>
  )
}
