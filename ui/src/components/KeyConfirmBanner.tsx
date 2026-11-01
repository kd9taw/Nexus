// The saved keys this station holds back until the operator confirms them: after a settings
// restore set a Cloudlog URL or an Icom network radio's address where the station had none (a
// fresh install, or reset settings), the system keychain may still hold a key it cannot say was
// typed for that address. Until the operator answers, Cloudlog uploads wait and the radio does not
// sign in with it. One row per key: the question with its address, Use (it is the key for that
// address, and only that one: the station refuses Use once the address has changed since) and
// Remove (forget it). A key kept only because removing it failed (it was saved for another
// address, or for a radio no longer on the roster) is said so, and offered Remove alone.
//
// It renders nothing when no key waits, and nowhere but the desktop app itself: the hosted page
// and the TV page have no such question, and a computer driving another station is refused the
// question (credentials are answered at the shack), so it stops asking there. It asks again every
// few seconds, because a restore made in Settings is what starts a wait.
//
// An in-flow strip, a child of `.app` like the client banner: it takes its own height from the
// shell below and covers nothing, so no control of any cockpit, Stop included, is ever under it.

import { useCallback, useEffect, useRef, useState } from 'react'
import { answerSavedKey, isTauri, keysToConfirm, type KeyToConfirm } from '../api'
import { t } from '../i18n'
import { pushToast } from '../toast'

/** How often the banner asks which saved keys wait. */
export const KEYS_TO_CONFIRM_EVERY_MS = 5_000

/** The station's refusals of an answer, word for word (`src-tauri/src/lib.rs`). */
export const KEY_ADDRESS_CHANGED =
  'That address changed since the notice asked, so nothing was let go. Check the notice again, then choose.'
export const ICOM_PASSWORD_OF_NO_RADIO =
  'That radio is no longer on the roster, so its saved network password can only be removed: choose Remove.'
export const KEY_NOT_REMOVED =
  'Removing this saved key failed, and it was saved for another address, so it can only be removed: choose Remove.'

/** A refused answer in the operator's language when it is one of the station's own; any other as
 *  it came. */
export function answerRefusal(err: unknown): string {
  const said = err instanceof Error ? err.message : String(err)
  switch (said) {
    case KEY_ADDRESS_CHANGED:
      return t('keyConfirm.refused.addressChanged')
    case ICOM_PASSWORD_OF_NO_RADIO:
      return t('keyConfirm.refused.noRadio')
    case KEY_NOT_REMOVED:
      return t('keyConfirm.refused.notRemoved')
    default:
      return said
  }
}

export function KeyConfirmBanner() {
  const [keys, setKeys] = useState<KeyToConfirm[]>([])
  const [answering, setAnswering] = useState(false)
  const [refused, setRefused] = useState(false)
  // Only the newest ask is shown: one sent before an answer can come back after the ask that
  // follows the answer, and would bring back a row already answered.
  const asked = useRef(0)
  const ask = useCallback(() => {
    const n = ++asked.current
    // An answer that is not a list (another build's station, a stand-in) shows nothing, as a
    // refusal does.
    void keysToConfirm()
      .then((answer) => {
        if (n === asked.current) setKeys(Array.isArray(answer) ? answer : [])
      })
      .catch(() => {
        if (n !== asked.current) return
        setKeys([])
        setRefused(true)
      })
  }, [])
  useEffect(() => {
    if (!isTauri() || refused) return
    ask()
    const every = window.setInterval(ask, KEYS_TO_CONFIRM_EVERY_MS)
    return () => window.clearInterval(every)
  }, [ask, refused])

  if (keys.length === 0) return null
  const answer = (key: KeyToConfirm, useIt: boolean) => {
    setAnswering(true)
    void answerSavedKey(key.radioId, useIt, key.address)
      .catch((e: unknown) => pushToast(answerRefusal(e), 'error'))
      .finally(() => {
        setAnswering(false)
        ask()
      })
  }
  return (
    <div className="key-confirm-banner" role="group" aria-label={t('keyConfirm.label')}>
      {keys.some((key) => !key.removalFailed) && <span className="key-confirm-why">{t('keyConfirm.why')}</span>}
      {keys.map((key) => (
        <span className="key-confirm-row" key={key.radioId ?? 'cloudlog'}>
          <span className="key-confirm-question">
            {key.removalFailed
              ? key.radioId === null
                ? t('keyConfirm.notRemoved.cloudlog')
                : t('keyConfirm.notRemoved.icomLan', { radio: key.radio })
              : key.radioId === null
                ? t('keyConfirm.cloudlog', { address: key.address })
                : t('keyConfirm.icomLan', { radio: key.radio, address: key.address })}
          </span>
          {!key.removalFailed && (
            <button type="button" disabled={answering} onClick={() => answer(key, true)}>
              {t('keyConfirm.use')}
            </button>
          )}
          <button type="button" disabled={answering} onClick={() => answer(key, false)}>
            {t('keyConfirm.remove')}
          </button>
        </span>
      ))}
    </div>
  )
}
