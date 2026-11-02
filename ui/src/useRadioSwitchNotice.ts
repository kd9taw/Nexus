// The radio switch notice: one line when Nexus switches the active radio BY ITSELF ("Switched to
// IC-9700 for 2m · Weak-signal digital"), with Undo and Why?.
//
// What raises it is the engine's own record of the switch (`AppSnapshot.autoRadioSwitch`, written
// only in the routing branch of a tune), never a guess from the active id changing: the operator's
// own radio pick, a Remote selection and a radio disabled in Settings change the active id too,
// and none of those is Nexus acting by itself.
//
// Shown only when the station's policy is `tell` (a new install's default); an existing settings
// file keeps the silent switch it always had until the operator changes it in Settings ▸ Radio ▸
// Radios. The switch itself is the same either way: this only says it happened.
//
// The notice speaks for one switch only while the station is still on the radio it names: it closes
// as soon as the active radio is another one, whoever moved it (the operator's pick, Settings ▸ Make
// active, a Remote client, a later switch). Undo checks that again at the click, so a stale notice
// never moves the station, and it is refused on the air: Undo is never what swaps rigs mid-over.
import { useEffect, useRef } from 'react'
import { t } from './i18n'
import { dismissToast, pushToast } from './toast'
import { ROUTE_MODE_LABEL } from './settings/describeRouting'
import { isOnAir, type AppSnapshot, type RadioSwitchPolicy } from './types'

/** How long the notice stays, ms. Long enough to read and reach Undo from the rig. */
export const RADIO_SWITCH_NOTICE_MS = 12_000

export interface RadioSwitchNoticeDeps {
  snap:
    | (Pick<AppSnapshot, 'autoRadioSwitch' | 'radios' | 'activeRadioId'> & {
        radio: Pick<AppSnapshot['radio'], 'transmitting' | 'txBusyReason' | 'rigKeyed'>
      })
    | null
    | undefined
  /** The station's policy; absent (settings not read yet) shows nothing. */
  policy: RadioSwitchPolicy | undefined
  /** Select a radio again through the operator's own radio pick (`set_active_radio`), answering with
   *  the snapshot after it. A rejection keeps the notice up with the reason, so Undo can be pressed again. */
  selectRadio: (id: number) => Promise<Pick<AppSnapshot, 'activeRadioId'> | null | undefined>
  /** Open Settings at a section id (App's `openSettingsAt`). */
  openSettingsAt: (where: string) => void
}

export function useRadioSwitchNotice({ snap, policy, selectRadio, openSettingsAt }: RadioSwitchNoticeDeps): void {
  // The last switch already accounted for. `undefined` until the first snapshot: a switch made
  // before this window opened (a reload, a pop-out) is history, not news.
  const seen = useRef<number | null | undefined>(undefined)
  // The notice up now, and the radio it says the station switched to.
  const shown = useRef<{ id: number; toId: number } | null>(null)
  const latest = useRef({ snap, selectRadio, openSettingsAt })
  latest.current = { snap, selectRadio, openSettingsAt }
  const sw = snap?.autoRadioSwitch
  const seq = snap ? (sw?.seq ?? null) : undefined
  const activeId = snap?.activeRadioId

  // Another radio is active: the notice no longer says what is true, and its Undo would act on it.
  useEffect(() => {
    if (shown.current && activeId !== shown.current.toId) {
      dismissToast(shown.current.id)
      shown.current = null
    }
  }, [activeId])

  useEffect(() => {
    if (seq === undefined) return
    if (seen.current === undefined) {
      seen.current = seq
      return
    }
    if (seq === null || seq === seen.current || !sw) return
    seen.current = seq
    // The routing chose a radio another Nexus window runs, and the station stayed put. Said
    // whatever the policy: the band asked for is not on the radio the operator may expect.
    if (sw.refused) {
      const radio = snap?.radios?.find((r) => r.id === sw.toId)?.name ?? t('radioSwitch.radio', { id: sw.toId })
      pushToast(t('radioSwitch.refused', { radio }), 'error', RADIO_SWITCH_NOTICE_MS)
      return
    }
    if (policy !== 'tell') return
    // Already moved on (another pick between two snapshots): nothing left to tell or undo.
    if (snap?.activeRadioId !== sw.toId) return
    const name = (id: number) => snap?.radios?.find((r) => r.id === id)?.name ?? t('radioSwitch.radio', { id })
    const mode = ROUTE_MODE_LABEL[sw.mode] ?? sw.mode
    const message = sw.band
      ? t('radioSwitch.notice', { radio: name(sw.toId), band: sw.band, mode })
      : t('radioSwitch.notice.noBand', { radio: name(sw.toId), mode })
    // One notice at a time: a newer switch replaces the one still up.
    if (shown.current) dismissToast(shown.current.id)
    const { fromId: from, toId: to } = sw
    const undo = async () => {
      const now = latest.current.snap
      // Not the switch on screen any more: do nothing (the notice closes).
      if (!now || now.activeRadioId !== to) return
      if (isOnAir(now.radio)) throw new Error(t('radioSwitch.undo.onAir'))
      const after = await latest.current.selectRadio(from)
      // The engine leaves a missing or disabled radio alone and still answers: say so, not "done".
      if (after?.activeRadioId !== from) throw new Error(t('radioSwitch.undo.unavailable', { radio: name(from) }))
    }
    const id = pushToast(message, 'info', RADIO_SWITCH_NOTICE_MS, {
      action: undo,
      actionLabel: t('radioSwitch.undo'),
      secondaryAction: () => latest.current.openSettingsAt('radios'),
      secondaryLabel: t('radioSwitch.why'),
    })
    shown.current = { id, toId: to }
    // `snap` and `sw` are read for the switch `seq` names; a new switch is a new seq.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [seq, policy])
}
