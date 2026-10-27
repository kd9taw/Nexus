// JS8Call's one-click message buttons by its compose box — REPLY, SNR, INFO and STATUS
// (`on_replyMacroButton_clicked` … `on_statusMacroButton_clicked`, mainwindow.cpp:4579-4640) and
// the Saved menu (`buildSavedMessagesMenu`, :4960-4990), which here also holds JS8Call's "Restore
// Previous Message" (its compose box's own menu, UI_Constructor.cpp:855-873). What each puts in
// the message, and whether it sends at once or only fills the compose box (Settings ▸ JS8, JS8Call's
// "Immediately transmit…", on by default), is the cockpit's (Js8Cockpit); this draws the buttons
// and says what they will do.
//
// SENDERS in the TX dock's CQ row, with no ⊞ id: nothing here stops a transmission, and every send
// it asks for goes through the engine's JS8 send and its gates (THE STOP LINE, CLAUDE.md). They lead
// that row, so the dock keeps its two rows (JS8Call has them in one row with CQ and HB too).
//
// Migrated from birth (i18n/hardcoded-strings.test.ts): REPLY and the command words are the
// mode's vocabulary; every sentence is from the catalog.
import * as RM from '@radix-ui/react-dropdown-menu'
import { t } from '../i18n'
import { JS8_COMMANDS, REPLY } from '../js8Vocab'

/** One saved message: as stored, and as the menu shows it (its macros replaced where known). */
export interface Js8SavedShown {
  text: string
  shown: string
}

interface Props {
  /** A message waits to go or the key is down (JS8Call's `isMessageQueuedForTransmit()`, the
   *  cockpit's `queued`): JS8Call greys these out until it has gone, on the predicate it greys CQ
   *  and HB on (`updateButtonDisplay`, mainwindow.cpp:5675-5689). */
  busy: boolean
  /** The call in the To box, JS8Call's selected call. */
  toCall: string
  /** The To box's call is a station heard: SNR has a report to send. */
  heard: boolean
  /** INFO is set in Settings (an empty INFO has nothing to send). */
  infoSet: boolean
  /** On: the buttons send at once; off: they fill the compose box. */
  sendAtOnce: boolean
  saved: readonly Js8SavedShown[]
  /** A message has gone out this session, so there is one to restore. */
  hasPrevious: boolean
  /** The compose box holds text, so there is a message to save. */
  composing: boolean
  onReply: () => void
  onSnr: () => void
  onInfo: () => void
  onStatus: () => void
  onSaved: (index: number) => void
  onRestore: () => void
  onSaveCurrent: () => void
  /** Settings ▸ JS8, where saved messages are edited; absent where there is no Settings. */
  onEditSaved?: () => void
}

const SNR_WORD = JS8_COMMANDS[25].label
const INFO_WORD = JS8_COMMANDS[17].label
const STATUS_WORD = JS8_COMMANDS[7].label

export function Js8ComposeActions(p: Props) {
  const to = p.toCall.trim().toUpperCase()
  const noCall = to === ''
  // One sentence per button and mode — literal keys, so the orphan guard sees each referenced.
  const replyTitle = noCall
    ? t('js8.dock.quick.needsCall')
    : p.sendAtOnce
      ? t('js8.dock.quick.reply.send', { call: to })
      : t('js8.dock.quick.reply.fill', { call: to })
  const snrTitle = noCall
    ? t('js8.dock.quick.needsCall')
    : !p.heard
      ? t('js8.dock.quick.snr.notHeard', { call: to })
      : p.sendAtOnce
        ? t('js8.dock.quick.snr.send', { call: to })
        : t('js8.dock.quick.snr.fill', { call: to })
  const infoTitle = !p.infoSet
    ? t('js8.dock.quick.info.unset')
    : p.sendAtOnce
      ? t('js8.dock.quick.info.send')
      : t('js8.dock.quick.info.fill')
  const statusTitle = p.sendAtOnce ? t('js8.dock.quick.status.send') : t('js8.dock.quick.status.fill')
  return (
    <span className="js8-quick-group" role="group" aria-label={t('js8.dock.quick.aria')}>
      <button type="button" className="cw-macro js8-quick js8-reply" disabled={p.busy || noCall} onClick={p.onReply} title={replyTitle}>
        <span className="cw-macro-label">{REPLY}</span>
      </button>
      <button
        type="button"
        className="cw-macro js8-quick js8-snr-send"
        disabled={p.busy || noCall || !p.heard}
        onClick={p.onSnr}
        title={snrTitle}
      >
        <span className="cw-macro-label">{SNR_WORD}</span>
      </button>
      <button type="button" className="cw-macro js8-quick js8-info" disabled={p.busy || !p.infoSet} onClick={p.onInfo} title={infoTitle}>
        <span className="cw-macro-label">{INFO_WORD}</span>
      </button>
      <button type="button" className="cw-macro js8-quick js8-status" disabled={p.busy} onClick={p.onStatus} title={statusTitle}>
        <span className="cw-macro-label">{STATUS_WORD}</span>
      </button>
      <RM.Root modal={false}>
        <RM.Trigger asChild>
          <button
            type="button"
            className="cw-macro js8-quick js8-saved"
            disabled={p.busy}
            title={p.sendAtOnce ? t('js8.dock.saved.title.send') : t('js8.dock.saved.title.fill')}
          >
            {/* One label, the ▾ inside it: a macro key stacks its children. */}
            <span className="cw-macro-label">
              {t('js8.dock.saved.label')}
              <span aria-hidden="true"> ▾</span>
            </span>
          </button>
        </RM.Trigger>
        <RM.Portal>
          <RM.Content className="ui-menu js8-saved-menu" side="top" align="start" sideOffset={4} collisionPadding={8}>
            {/* The portal escapes `.app`'s zoom, so the content re-applies it (Menu.tsx's reason). */}
            <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
              <RM.Item className="ui-menu-item js8-restore" disabled={!p.hasPrevious} onSelect={p.onRestore}>
                {t('js8.dock.saved.restore')}
              </RM.Item>
              <RM.Separator className="box-menu-sep" />
              {p.saved.length === 0 ? (
                <RM.Item className="ui-menu-item" disabled>
                  {t('js8.dock.saved.none')}
                </RM.Item>
              ) : (
                p.saved.map((m, i) => (
                  <RM.Item key={`${i}-${m.text}`} className="ui-menu-item js8-saved-item" onSelect={() => p.onSaved(i)}>
                    {m.shown}
                  </RM.Item>
                ))
              )}
              <RM.Separator className="box-menu-sep" />
              {p.onEditSaved && (
                <RM.Item className="ui-menu-item js8-saved-edit" onSelect={p.onEditSaved}>
                  {t('js8.dock.saved.edit')}
                </RM.Item>
              )}
              <RM.Item className="ui-menu-item js8-saved-keep" disabled={!p.composing} onSelect={p.onSaveCurrent}>
                {t('js8.dock.saved.keep')}
              </RM.Item>
            </div>
          </RM.Content>
        </RM.Portal>
      </RM.Root>
    </span>
  )
}
