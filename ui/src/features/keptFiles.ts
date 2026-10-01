// The words for a file the station could not read and KEPT rather than save over
// (`tempo_core::keep_aside`): what it held, where it is now, and that nothing was deleted.
//
// One literal key per store, so the i18n guards can read every call site (a key chosen at run
// time is invisible to them). A store this build has no words for, a newer station's, reads as
// the general sentence, which still says where the file is.
import { t } from '../i18n'
import type { KeptFile } from '../types'

export function keptFileMessage(f: KeptFile): string {
  if (f.keptInPlace) return t('shell.keptFile.keptInPlace', { path: f.path })
  switch (f.store) {
    case 'pendingQso':
      return t('shell.keptFile.pendingQso', { path: f.path })
    case 'fieldDay':
      return t('shell.keptFile.fieldDay', { path: f.path })
    case 'js8Inbox':
      return t('shell.keptFile.js8Inbox', { path: f.path })
    case 'pendingMsgs':
      return t('shell.keptFile.pendingMsgs', { path: f.path })
    case 'assistance':
      return t('shell.keptFile.assistance', { path: f.path })
    default:
      return t('shell.keptFile.other', { path: f.path })
  }
}
