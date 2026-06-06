import { useState } from 'react'
import { Dialog } from './ui/Dialog'
import { PROFILE_LIST, PROFILES, type ProfileId } from '../features/profiles'
import type { View } from '../features/registry'

interface Props {
  /** Apply the chosen goal profile(s) and navigate to `landing`. */
  onApply: (ids: ProfileId[], landing: View) => void
  /** Close without changing the current feature set (also ESC / backdrop). */
  onSkip: () => void
}

// Goal cards are the five goal profiles; "Everything" is its own one-click button.
const GOALS = PROFILE_LIST.filter((p) => p.id !== 'everything')

/**
 * First-run setup wizard — a GOAL-driven preset selector (never asks for
 * self-rated experience). Pick one or more goals → the matching feature bundles
 * turn on; everything stays changeable later in Settings. Shown once on a fresh
 * install (and re-openable from Settings). Built on the Radix [`Dialog`] for
 * focus-trap, ESC, and backdrop dismissal. See feature-modularity.md §4.6.
 */
export function SetupWizard({ onApply, onSkip }: Props) {
  const [selected, setSelected] = useState<Set<ProfileId>>(new Set())
  const toggle = (id: ProfileId) =>
    setSelected((s) => {
      const n = new Set(s)
      if (n.has(id)) n.delete(id)
      else n.add(id)
      return n
    })

  const ids = [...selected]
  const landing: View = ids.length === 1 ? PROFILES[ids[0]].landing : 'operate'
  const goLabel =
    ids.length === 0
      ? 'Choose a goal'
      : ids.length === 1
        ? `Set up ${PROFILES[ids[0]].label}`
        : `Set up ${ids.length} goals`

  return (
    <Dialog
      open
      // ESC / backdrop / close → skip (keeps the current set, marks seen).
      onOpenChange={(o) => {
        if (!o) onSkip()
      }}
      title="Set up Nexus"
      hideTitle
    >
      <h2 className="wizard-title">What do you mostly want to do?</h2>
      <p className="wizard-sub">
        Pick one or more — we’ll turn on the right features. You can change everything later in
        Settings → Features.
      </p>

      <div className="wizard-goals">
        {GOALS.map((p) => (
          <button
            key={p.id}
            type="button"
            className={`wizard-goal${selected.has(p.id) ? ' sel' : ''}`}
            aria-pressed={selected.has(p.id)}
            onClick={() => toggle(p.id)}
          >
            <span className="wizard-goal-label">{p.label}</span>
            <span className="wizard-goal-blurb">{p.blurb}</span>
          </button>
        ))}
      </div>

      <div className="wizard-actions">
        <button
          type="button"
          className="wizard-everything"
          onClick={() => onApply(['everything'], 'operate')}
        >
          Turn everything on (expert)
        </button>
        <div className="wizard-actions-right">
          <button type="button" className="wizard-skip" onClick={onSkip}>
            I’ll set it up myself
          </button>
          <button
            type="button"
            className="wizard-go"
            disabled={ids.length === 0}
            onClick={() => onApply(ids, landing)}
          >
            {goLabel}
          </button>
        </div>
      </div>
    </Dialog>
  )
}
