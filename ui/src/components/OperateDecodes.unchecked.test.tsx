// @vitest-environment jsdom
//
// An UNCHECKED decode (the JTDX-style FT8 decoder's FT8S/FT8SD and matched filters: the expected
// message accepted on correlation, with no LDPC and no CRC) is shown with JTDX's `^` and a title
// that says why. Nothing automatic acts on it (the engine's `error_checked`), and the operator
// can still work the station by hand: a double-click works it as any other row.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { OperateDecodes } from './OperateDecodes'
import { t } from '../i18n'
import type { DecodeRow } from '../types'

vi.mock('../api', () => ({ openQrzPage: vi.fn() }))

const decode = (over: Partial<DecodeRow> = {}): DecodeRow => ({
  from: 'W9XYZ',
  snr: -21,
  dtSec: 0.2,
  freqHz: 1500,
  message: 'K2DEF W9XYZ -10',
  isCq: false,
  directedToMe: true,
  worked: false,
  tier: 'FT8',
  rv: 0,
  ...over,
})

function mount(rows: DecodeRow[], onCall = vi.fn()) {
  render(
    <OperateDecodes decodes={rows} slot={100} rxOffsetHz={1500} band="20m" tier="FT8" harqRescues={0} onCall={onCall} />,
  )
  return onCall
}

const row = (call: string) => screen.getAllByRole('option').find((r) => (r.textContent ?? '').includes(call))!

afterEach(cleanup)

describe('an unchecked decode', () => {
  it('is marked ^ with a title that says it was not error-checked', () => {
    mount([decode({ unchecked: true })])
    const mark = row('W9XYZ').querySelector('.decode-marker-unchecked')
    expect(mark?.textContent).toBe('^')
    expect(mark?.getAttribute('title')).toBe(t('operate.decodes.marker.unchecked.title'))
  })

  it('an error-checked decode carries no mark', () => {
    mount([decode({ unchecked: false }), decode({ from: 'N7GHI', message: 'CQ N7GHI DN31', isCq: true })])
    expect(row('W9XYZ').querySelector('.decode-marker-unchecked')).toBeNull()
    expect(row('N7GHI').querySelector('.decode-marker-unchecked')).toBeNull()
  })

  it('can still be worked by hand', () => {
    const onCall = mount([decode({ unchecked: true })])
    fireEvent.doubleClick(row('W9XYZ'))
    expect(onCall).toHaveBeenCalledWith('W9XYZ', undefined, 'K2DEF W9XYZ -10', -21, 1500)
  })
})
