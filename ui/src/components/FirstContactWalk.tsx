// The first-contact walk-through: four lines on how an FT8 contact goes, for a new operator.
//
// Opened after the first-run wizard when the operator asked for it there (pre-ticked for "New to
// the hobby"), and any time from Help ▸ Your first FT8 contact. It is READING ONLY: nothing here
// writes a setting, arms TX or touches the radio.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every sentence lives in
// `i18n/en.ts` under `firstContact.*`. What stays here are the transmit buttons' own names, which
// the cockpit's TX strip shows untranslated in every language, so the sentence quotes them as
// they read on screen.
import { t } from '../i18n'
import { Dialog } from './ui/Dialog'
import { useFocusReturn } from '../focusReturn'

/** The TX strip's button names, exactly as the FT8 screen shows them (`OperateQsoStrip.tsx`). */
const TX_BUTTONS = { txOff: 'TX Off', txOn: 'TX On', stop: 'Stop TX' } as const
/** The Settings switch the answer line names when it is off, as Settings ▸ Digital labels it. */
const DOUBLE_CLICK_SETTING = 'Double-click arms TX'

/** `doubleClickSetsTx`: the station's "Double-click arms TX" setting (absent = on, its default).
 *  With it on, a double-click turns TX on and Nexus may start calling in the current period
 *  (`call_station_ctx`); with it off, TX stays off until the operator turns it on. */
export function FirstContactWalk({
  onClose,
  doubleClickSetsTx = true,
}: {
  onClose: () => void
  doubleClickSetsTx?: boolean
}) {
  const returnFocus = useFocusReturn(true)
  return (
    <Dialog
      open
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title={t('firstContact.title')}
      onCloseAutoFocus={returnFocus}
    >
      <p className="wizard-sub">{t('firstContact.sub')}</p>
      <ol className="first-contact-steps">
        <li>{t('firstContact.step.listen')}</li>
        <li>{t('firstContact.step.cq')}</li>
        <li>
          {doubleClickSetsTx
            ? t('firstContact.step.answer')
            : t('firstContact.step.answer.manual', { setting: DOUBLE_CLICK_SETTING, txOff: TX_BUTTONS.txOff })}
        </li>
        <li>{t('firstContact.step.tx', {
            txOff: TX_BUTTONS.txOff,
            txOn: TX_BUTTONS.txOn,
            stop: TX_BUTTONS.stop,
          })}</li>
      </ol>
      <div className="wizard-actions">
        <span />
        <div className="wizard-actions-right">
          <button type="button" className="wizard-go" onClick={onClose}>
            {t('firstContact.close')}
          </button>
        </div>
      </div>
    </Dialog>
  )
}
