// The JTDX-style FT8 decoder's CPU guard notice: one line when Nexus steps the decoder's preset down
// because a full pass took longer than a reply can wait ("FT8 decoding took 2,900 ms … now runs Keen
// instead of Ludicrous"), and once, when even Easy is too slow.
//
// What raises it is the engine's own record (`AppSnapshot.ft8StepDown`, written only by the guard),
// each step once (`seq`). A step made before this window opened (a reload, a pop-out) is history,
// not news: the first snapshot only sets the mark.
import { useEffect, useRef } from 'react'
import { t } from './i18n'
import { pushToast } from './toast'
import type { AppSnapshot, JtdxPreset } from './types'

/** How long the notice stays, ms. */
export const FT8_STEP_DOWN_NOTICE_MS = 15_000

/** A preset's name, as the Settings chips say it. */
function presetName(p: JtdxPreset): string {
  switch (p) {
    case 'easy':
      return t('settings.digital.jtdxPreset.easy')
    case 'keen':
      return t('settings.digital.jtdxPreset.keen')
    case 'ludicrous':
      return t('settings.digital.jtdxPreset.ludicrous')
    case 'custom':
      return t('settings.digital.jtdxPreset.custom')
  }
}

export function useFt8StepDownNotice(snap: Pick<AppSnapshot, 'ft8StepDown'> | null | undefined): void {
  const seen = useRef<number | null | undefined>(undefined)
  const sd = snap?.ft8StepDown
  const seq = snap ? (sd?.seq ?? null) : undefined
  useEffect(() => {
    if (seq === undefined) return
    if (seen.current === undefined) {
      seen.current = seq
      return
    }
    if (seq === null || seq === seen.current || !sd) return
    seen.current = seq
    pushToast(
      sd.to
        ? t('ft8StepDown.stepped', { ms: sd.decodeMs, budget: sd.budgetMs, from: presetName(sd.from), to: presetName(sd.to) })
        : t('ft8StepDown.floor', { ms: sd.decodeMs, budget: sd.budgetMs }),
      'info',
      FT8_STEP_DOWN_NOTICE_MS,
    )
  }, [seq, sd])
}
