// The words for each store's kept file: its own sentence, naming where the file is now. A store
// this build has no words for reads as the general sentence, never as another store's.
import { describe, expect, it } from 'vitest'
import { t } from '../i18n'
import { keptFileMessage } from './keptFiles'

const PATH = '/home/op/.config/tempo/x.unreadable-20260930-142233.json'

describe('keptFileMessage', () => {
  it.each([
    ['pendingQso', 'shell.keptFile.pendingQso'],
    ['fieldDay', 'shell.keptFile.fieldDay'],
    ['js8Inbox', 'shell.keptFile.js8Inbox'],
    ['pendingMsgs', 'shell.keptFile.pendingMsgs'],
    ['assistance', 'shell.keptFile.assistance'],
    ['conversations', 'shell.keptFile.conversations'],
  ] as const)(
    'says what a %s file held, and where it is now',
    (store, key) => {
      const said = keptFileMessage({ store, path: PATH, keptInPlace: false })
      expect(said).toBe(t(key, { path: PATH }))
      expect(said).toContain(PATH)
      expect(said, 'its own sentence, not the general one').not.toBe(t('shell.keptFile.other', { path: PATH }))
    },
  )

  it('reads a store it has no words for as the general sentence', () => {
    expect(keptFileMessage({ store: 'aNewerStore', path: PATH, keptInPlace: false })).toBe(
      t('shell.keptFile.other', { path: PATH }),
    )
  })

  it('says a file left in place is not written over, whatever the store', () => {
    for (const store of ['pendingQso', 'aNewerStore']) {
      expect(keptFileMessage({ store, path: PATH, keptInPlace: true })).toBe(
        t('shell.keptFile.keptInPlace', { path: PATH }),
      )
    }
  })
})
