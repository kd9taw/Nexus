// THE SUB RECEIVER STRIP — the Phone cockpit's view of a dual-receiver radio's SECOND receiver.
//
// ⭐ IT RENDERS NOTHING UNLESS THE SNAPSHOT OFFERS A SUB. A radio with one receiver, a radio
// nobody has read a manual for (UNKNOWN — never a "no", and no Sub UI on it, ruling D3), a
// shared-front-end radio this build does not offer a Sub for, and a station older than the
// field all draw exactly what they drew before; a golden in PhoneCockpit.subreceiver.test.tsx
// holds that byte for byte.
//
// ⚠️ A NEW COMPONENT, NOT A WIDENED SHARED ONE. The dual-receiver ruling D9 puts the Sub's
// meters and scope PER HOST and names no host, so no meter or scope host changes: this is not
// `SMeter` (one host), `TxMeters` (three) or `PhoneScope` (two) grown a Sub mode. It has one
// host — Phone's receiver pane — and draws no meter at all: nothing reads the Sub's S-meter in
// this build, and a second meter that never moves is the one thing this programme set out not
// to ship.
//
// What it draws, all of it from the snapshot and the controls table:
//   · the Sub's dial where the engine knows it (the uplink an acknowledged satellite split
//     rides), else "—" — the frequency is not read back;
//   · the Sub's rows the table offers (`subChainControls`): RF, AF and squelch where the vendor
//     table credits the Sub with the stage (D7) and the CAT path can name the Sub — each one
//     showing the value the RADIO ACCEPTED, never a reading;
//   · one line for each honest "no": the connection cannot reach the Sub; a level NOT CONFIRMED
//     for the Sub (never "not on this radio"); and that these levels are set, not read back.
//
// Desktop only: the host draws it for the local station alone. The Remote operation contract
// carries no Sub control, so the hosted page renders none.
import { useEffect, useRef, useState } from 'react'
import type { AppSnapshot, RadioStatus, ReceiverStatus } from '../types'
import { setSubLevel } from '../api'
import { pushToast } from '../toast'
import { t } from '../i18n'
import { formatDialMhz } from './FrequencyReadout'
import {
  deadControlProps,
  subCauseFor,
  subChainControls,
  subUnconfirmedPlates,
  type RigControl,
  type SubState,
} from '../features/rigControls'

/** The rig's own name for its second receiver, as printed on a front panel. INVARIANT. */
const SUB = 'SUB'
/** Not known. */
const DASH = '—'

/** The Sub rows Nexus has built, each with its wire name, the snapshot field its accepted value
 *  arrives in, and its words — spelled out, never a template key, so the catalogue scanners can
 *  see every one. Keyed by the registry id. */
const LEVELS: Record<
  string,
  { level: 'rf' | 'af' | 'sql'; field: 'rfGain' | 'afGain' | 'squelch'; aria: () => string; title: () => string }
> = {
  RF: { level: 'rf', field: 'rfGain', aria: () => t('phone.sub.rf.aria'), title: () => t('phone.sub.rf.title') },
  AF: { level: 'af', field: 'afGain', aria: () => t('phone.sub.af.aria'), title: () => t('phone.sub.af.title') },
  SQL: { level: 'sql', field: 'squelch', aria: () => t('phone.sub.sql.aria'), title: () => t('phone.sub.sql.title') },
}

export function SubReceiverStrip({
  radio,
  catOk,
  describedBy,
  onSnap,
}: {
  radio: RadioStatus
  catOk: boolean
  /** The pane's no-CAT banner, which a dead row points at rather than repeating it. */
  describedBy?: string
  onSnap?: (s: AppSnapshot) => void
}) {
  const receivers = radio.receivers
  const sub = receivers?.sub
  // No hooks above this line, and none in this component at all: the rows own theirs.
  if (!receivers || !sub) return null
  const state: SubState = { catOk, receivers }
  const rows = subChainControls(state)
  const unconfirmed = subUnconfirmedPlates(state)
  const dial = sub.dialMhz
  return (
    <div className="ph-chain ph-subrx" role="group" aria-label={t('phone.sub.aria')} data-receiver="sub">
      <span className="ph-chain-item ph-subrx-head">
        <span className="ph-dsplev-lbl">{SUB}</span>
        <span className="ph-subrx-dial mono" data-sub-dial title={dial == null ? t('phone.sub.dial.unread') : undefined}>
          {dial == null ? DASH : formatDialMhz(dial)}
        </span>
        {dial != null && sub.band ? <span className="ph-subrx-tag mono">{sub.band}</span> : null}
        {dial != null && sub.sideband ? <span className="ph-subrx-tag mono">{sub.sideband}</span> : null}
      </span>
      {rows.map((c) => (
        <SubLevelRow
          key={c.id}
          control={c}
          sub={sub}
          dead={subCauseFor(c, state) === 'noCat'}
          describedBy={describedBy}
          onSnap={onSnap}
        />
      ))}
      {receivers.subCommandable === false && (
        <p className="ph-chain-absent" role="note">
          {t('phone.sub.noRoute')}
        </p>
      )}
      {unconfirmed.length > 0 && (
        <p className="ph-chain-absent" role="note">
          {t('phone.sub.unconfirmed', { plates: unconfirmed.join(' · ') })}
        </p>
      )}
      {rows.length > 0 && (
        <p className="ph-chain-absent" role="note">
          {t('phone.sub.setNotRead')}
        </p>
      )}
    </div>
  )
}

/** One Sub level. The thumb follows the operator's hand; the READOUT is what the radio accepted
 *  — so a value the radio refused shows as the thumb and the readout disagreeing, which is the
 *  truth, rather than as a number nobody confirmed. */
function SubLevelRow({
  control,
  sub,
  dead,
  describedBy,
  onSnap,
}: {
  control: RigControl
  sub: ReceiverStatus
  dead: boolean
  describedBy?: string
  onSnap?: (s: AppSnapshot) => void
}) {
  const spec = LEVELS[control.id]
  const accepted = spec ? sub[spec.field] : null
  const [pct, setPct] = useState(accepted != null ? Math.round(accepted * 100) : 50)
  const dragging = useRef(false)
  useEffect(() => {
    if (accepted != null && !dragging.current) setPct(Math.round(accepted * 100))
  }, [accepted])
  // A row the table offers that this strip has no wiring for is a table/strip mismatch, and it
  // draws nothing rather than a control that commands nothing.
  if (!spec) return null
  const change = (value: number) => {
    setPct(value)
    void setSubLevel(spec.level, value / 100)
      .then((s) => onSnap?.(s))
      .catch(() => pushToast(t('phone.sub.failed', { plate: control.plate }), 'error'))
  }
  return (
    <div className="ph-chain-item" data-chain={control.id}>
      <label className="ph-dsplev" title={spec.title()}>
        <span>{control.plate}</span>
        <input
          {...(dead ? deadControlProps('input', describedBy) : {})}
          type="range"
          min={0}
          max={100}
          value={pct}
          aria-label={spec.aria()}
          aria-valuetext={accepted != null ? undefined : t('phone.sub.level.unknown')}
          onChange={(e) => change(Number(e.target.value))}
          onPointerDown={() => {
            dragging.current = true
          }}
          onPointerUp={() => {
            dragging.current = false
          }}
        />
        <span className="ph-power-val">{accepted != null && !dead ? `${Math.round(accepted * 100)}%` : DASH}</span>
      </label>
    </div>
  )
}
