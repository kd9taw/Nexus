// The saved keys this station holds back until the operator confirms them: after a settings
// restore set a Cloudlog URL or an Icom network radio's address where the station had none (a
// fresh install, or reset settings), the system keychain may still hold a key it cannot say was
// typed for that address. Until the operator answers, Cloudlog uploads wait and the radio does not
// sign in with it. One row per key: the question with its address, Use (it is the key for that
// address) and Remove (forget it).
//
// It renders nothing when no key waits, and nowhere but the desktop app itself: the hosted page
// and the TV page have no such question, and a computer driving another station is refused the
// question (credentials are answered at the shack), so it stops asking there. It asks again every
// few seconds, because a restore made in Settings is what starts a wait.
//
// An in-flow strip, a child of `.app` like the client banner: it takes its own height from the
// shell below and covers nothing, so no control of any cockpit, Stop included, is ever under it.

import { useCallback, useEffect, useState } from 'react'
import { answerSavedKey, isTauri, keysToConfirm, type KeyToConfirm } from '../api'
import { t } from '../i18n'
import { pushToast } from '../toast'

/** How often the banner asks which saved keys wait. */
export const KEYS_TO_CONFIRM_EVERY_MS = 5_000

export function KeyConfirmBanner() {
  const [keys, setKeys] = useState<KeyToConfirm[]>([])
  const [answering, setAnswering] = useState(false)
  const [refused, setRefused] = useState(false)
  const ask = useCallback(() => {
    // An answer that is not a list (another build's station, a stand-in) shows nothing, as a
    // refusal does.
    void keysToConfirm()
      .then((answer) => setKeys(Array.isArray(answer) ? answer : []))
      .catch(() => {
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
    void answerSavedKey(key.radioId, useIt)
      .catch((e: unknown) => pushToast(e instanceof Error ? e.message : String(e), 'error'))
      .finally(() => {
        setAnswering(false)
        ask()
      })
  }
  return (
    <div className="key-confirm-banner" role="group" aria-label={t('keyConfirm.label')}>
      <span className="key-confirm-why">{t('keyConfirm.why')}</span>
      {keys.map((key) => (
        <span className="key-confirm-row" key={key.radioId ?? 'cloudlog'}>
          <span className="key-confirm-question">
            {key.radioId === null
              ? t('keyConfirm.cloudlog', { address: key.address })
              : t('keyConfirm.icomLan', { radio: key.radio, address: key.address })}
          </span>
          <button type="button" disabled={answering} onClick={() => answer(key, true)}>
            {t('keyConfirm.use')}
          </button>
          <button type="button" disabled={answering} onClick={() => answer(key, false)}>
            {t('keyConfirm.remove')}
          </button>
        </span>
      ))}
    </div>
  )
}
