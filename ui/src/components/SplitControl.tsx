// The manual split control — SPLIT toggle, ±1 kHz steps, and the live offset readout.
//
// ⭐ ONE COPY, USED BY THREE COCKPITS. It shipped in Phone only, so a CW operator working a
// DXpedition split — which is how DX is worked, and the case the FCC segment structure creates
// every day — had no way to set split from his cockpit at all. That is the half of the
// 2026-08-25 field report the privilege-gate fix could not reach: the gate now judges the
// transmit frequency correctly, and the operator still could not tell it what that frequency
// was.
//
// Extracted rather than copied. The logic is small but not obvious — a functional updater so
// rapid bumps that fire inside the IPC round-trip do not all read the same stale value, and an
// effect that adopts the rig's real offset when split is programmed EXTERNALLY (a pile-up spot
// with "UP 5"). Three copies of that would drift, which is exactly the defect class that cost a
// day here: three identical resolver lists where two new entries went into one of them.
//
// ⚠️ THIS IS NOT A STOP CONTROL. It is TX-adjacent — it decides where the transmitter goes —
// but it neither starts nor stops a transmission, so it must NEVER be added to a cockpit's
// stop-line census or to a sweep's `stopControls` (doing so would demand its pane be
// unhideable). See CLAUDE.md, "THE STOP LINE".
//
// Structural size is not set here: the classes are shared with Phone's original markup and the
// sheet owns them.
import { useEffect, useRef, useState } from 'react'
import type { AppSnapshot } from '../types'
import { setSplit } from '../api'
import { t } from '../i18n'

/** kHz per ± press. One is the pile-up convention; the operator holds for more. */
const SPLIT_STEP_KHZ = 1
/** How far either side of the RX dial a manual split may be pushed. */
const SPLIT_LIMIT_KHZ = 90

interface Props {
  snap: AppSnapshot
  /** Apply the snapshot the IPC returns, so the readout does not wait for the next poll. */
  onSnap?: (snap: AppSnapshot) => void
  /** Surface a failure to the operator — silence here reads as "the button is broken". */
  onError?: (msg: string) => void
}

export function SplitControl({ snap, onSnap, onError }: Props) {
  // The desired TX dial lives in the snapshot; a plain retune clears it (backend).
  // Offset is kHz off the RX dial; default +5, the common pileup.
  const [splitOffsetKhz, setSplitOffsetKhz] = useState(5)
  const splitTxMhz = snap.radio.splitTxMhz ?? null
  const splitOn = splitTxMhz != null

  // When split turns on externally — a pile-up spot programming "UP 5" — adopt the rig's real
  // offset so the readout and the next bump start from the truth, not a stale default.
  const wasSplitOn = useRef(false)
  useEffect(() => {
    if (splitOn && !wasSplitOn.current && splitTxMhz != null) {
      setSplitOffsetKhz(Math.round((splitTxMhz - snap.radio.dialMhz) * 1000))
    }
    wasSplitOn.current = splitOn
  }, [splitOn, splitTxMhz, snap.radio.dialMhz])

  const applySplitTx = (offsetKhz: number) =>
    setSplit(snap.radio.dialMhz + offsetKhz / 1000)
      .then((s) => onSnap?.(s))
      .catch(() => onError?.(t('phone.split.setFailed')))

  const toggleSplit = () =>
    splitOn
      ? setSplit(null)
          .then((s) => onSnap?.(s))
          .catch(() => onError?.(t('phone.split.clearFailed')))
      : applySplitTx(splitOffsetKhz)

  // Accumulate on local state (functional updater) so rapid bumps that fire before the
  // IPC/onSnap round-trip don't all read the same stale value and collapse into one step.
  const bumpSplit = (delta: number) =>
    setSplitOffsetKhz((prev) => {
      const next = Math.max(-SPLIT_LIMIT_KHZ, Math.min(SPLIT_LIMIT_KHZ, prev + delta))
      if (splitOn) void applySplitTx(next)
      return next
    })

  return (
    <div className={`ph-split ${splitOn ? 'on' : ''}`}>
      <button
        className="ph-split-toggle"
        onClick={toggleSplit}
        title={
          splitOn
            ? t('phone.split.on.title', { freq: splitTxMhz?.toFixed(3) ?? '' })
            : t('phone.split.off.title')
        }
      >
        SPLIT
      </button>
      <button
        className="ph-split-step"
        onClick={() => bumpSplit(-SPLIT_STEP_KHZ)}
        title={t('phone.split.lower.title', { step: SPLIT_STEP_KHZ })}
      >
        −
      </button>
      <span className="ph-split-amt mono" title={t('phone.split.offset.title')}>
        {splitOffsetKhz >= 0 ? `+${splitOffsetKhz}` : `${splitOffsetKhz}`}
      </span>
      <button
        className="ph-split-step"
        onClick={() => bumpSplit(SPLIT_STEP_KHZ)}
        title={t('phone.split.higher.title', { step: SPLIT_STEP_KHZ })}
      >
        +
      </button>
    </div>
  )
}
