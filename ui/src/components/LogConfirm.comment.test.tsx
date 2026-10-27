// @vitest-environment jsdom
//
// #371: the prompt-to-log popup carries a Comments field, so a note goes into the contact as it
// is logged (ADIF COMMENT) instead of being added on QRZ afterwards. The field shows the comment
// the contact already holds (our own dB-reports one, when that option is on), and what it holds
// when Log is pressed is what the confirm sends.

import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { LogConfirm } from './LogConfirm'
import { t } from '../i18n'
import type { LoggedQso } from '../types'

const QSO = {
  call: 'DL1ABC',
  grid: 'JO62',
  band: '20m',
  mode: 'FT8',
  rstSent: '-12',
  rstRcvd: '-09',
  comment: 'FT8  Sent: -12  Rcvd: -09',
} as unknown as LoggedQso

afterEach(cleanup)

function open(record: LoggedQso = QSO) {
  const onConfirm = vi.fn()
  render(<LogConfirm record={record} onConfirm={onConfirm} onDiscard={() => {}} />)
  const box = screen.getByLabelText(t('logPrompt.comment.label')) as HTMLInputElement
  return { onConfirm, box, log: () => fireEvent.click(screen.getByRole('button', { name: t('logPrompt.log') })) }
}

describe('the log prompt Comments field (#371)', () => {
  it('shows the comment the contact holds', () => {
    const { box } = open()
    expect(box.value).toBe('FT8  Sent: -12  Rcvd: -09')
  })

  it('logs what the operator typed as the COMMENT', () => {
    const { box, onConfirm, log } = open()
    fireEvent.change(box, { target: { value: '  Nice signal, 5 W to a vertical ' } })
    log()
    expect(onConfirm).toHaveBeenCalledTimes(1)
    expect(onConfirm.mock.calls[0][0].comment).toBe('Nice signal, 5 W to a vertical')
  })

  it('sends the held comment back untouched when the field is left alone', () => {
    const { onConfirm, log } = open()
    log()
    expect(onConfirm.mock.calls[0][0].comment).toBe('FT8  Sent: -12  Rcvd: -09')
  })

  it('a cleared field logs no comment', () => {
    const { box, onConfirm, log } = open()
    fireEvent.change(box, { target: { value: '   ' } })
    log()
    expect(onConfirm.mock.calls[0][0].comment).toBeNull()
  })

  it('a contact with no comment opens an empty field', () => {
    const { box } = open({ ...QSO, comment: null })
    expect(box.value).toBe('')
  })
})
