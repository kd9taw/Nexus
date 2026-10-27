// The activation, said where the operator is rather than on the POTA/SOTA view alone.
//
// One the last session left running and this launch took up: "Still activating …?", once a
// session, sticky until dismissed, with "Still activating" on the toast. It is PAUSED until then —
// nothing is tagged with the park — because the file may be days old and the contacts made at home
// since are not the park's; Stop is on the activation strip. A crash or a battery swap mid-
// activation no longer silently stops the park tagging, and no longer silently keeps it either.
// And 0000Z: the count starts again at 0 of 10 and the activation runs on, so the operator is
// told the day turned over rather than left to wonder why the count dropped.

import { useEffect, useRef } from 'react'
import { resumeActivation } from './api'
import { activationParks } from './features/potaActivation'
import { t } from './i18n'
import { pushToast, withErrorToast } from './toast'
import type { Activation } from './types'

export function useActivationNotices(activation: Activation | null | undefined): void {
  const restoredSaid = useRef(false)
  const lastDay = useRef<number | null>(null)
  const reference = activation?.reference ?? null
  const utcDay = activation?.utcDay ?? null
  const restored = activation?.restored === true
  const parks = activationParks(activation).join(', ')
  useEffect(() => {
    if (reference == null) {
      lastDay.current = null
      return
    }
    if (restored && !restoredSaid.current) {
      restoredSaid.current = true
      pushToast(t('ota.activation.restored', { reference: parks }), 'info', 0, {
        action: () => void withErrorToast(() => resumeActivation(), t('ota.activation.resumeFailed')),
        actionLabel: t('ota.activation.resume.label'),
      })
    }
    // A later UTC day seen while it ran: never the first sight of one (a launch, a Start).
    if (lastDay.current != null && utcDay != null && utcDay > lastDay.current) {
      pushToast(t('ota.activation.newDay', { reference: parks }), 'info', 0)
    }
    lastDay.current = utcDay
  }, [reference, utcDay, restored, parks])
}
