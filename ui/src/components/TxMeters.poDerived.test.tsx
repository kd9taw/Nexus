// @vitest-environment jsdom
//
// A DERIVED PO SAYS SO (#489). A radio whose Hamlib backend gives no watts (the FT-450D, model
// 1046, among eight catalog models) reports its power only as a share of its meter, and the
// engine shows that share of the radio's rated watts (`txPoDerived`). The figure is an estimate,
// so the meter marks it: the label reads PO≈ and the tooltip says where the number comes from.
//
// Every check is a pair with the radio's own watts, which keep PO and the plain tooltip: on the
// unmarked component both renders were identical.
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { TxMeters } from './TxMeters'
import type { RadioStatus } from '../types'
import { t } from '../i18n'

afterEach(cleanup)

function radio(txPoW: number | null, txPoDerived?: boolean): RadioStatus {
  // An SWR reading too, so the panel draws its rows when Po has none (an unpinned panel with no
  // reading at all draws nothing).
  return { rigKeyed: true, txSwr: 1.2, txPoW, txPoDerived, ratedWatts: 100 } as unknown as RadioStatus
}

function poRow(root: HTMLElement) {
  const row = [...root.querySelectorAll<HTMLElement>('.ph-txmeter')].find((r) =>
    (r.querySelector('.ph-txmeter-label')?.textContent ?? '').startsWith('PO'),
  )
  return {
    label: row?.querySelector('.ph-txmeter-label')?.textContent ?? '',
    value: row?.querySelector('.ph-txmeter-value')?.textContent ?? '',
    title: row?.getAttribute('title') ?? '',
  }
}

describe('a derived forward power is marked as derived', () => {
  it('the radio’s own watts: PO, and the plain tooltip (the control)', () => {
    const r = poRow(render(<TxMeters radio={radio(45)} />).container)
    expect(r.label).toBe('PO')
    expect(r.title).toBe(t('meters.tx.po.title'))
    expect(r.value).toBe('45 W')
  })

  it('a derived figure: PO≈, and a tooltip that says where it comes from', () => {
    const r = poRow(render(<TxMeters radio={radio(45, true)} />).container)
    expect(r.label).toBe('PO≈')
    expect(r.title).toBe(t('meters.tx.po.derived'))
    expect(r.title).not.toBe(t('meters.tx.po.title'))
    expect(r.value, 'the reading itself is shown as before').toBe('45 W')
  })

  it('the mark needs a reading to be about: no reading, plain PO', () => {
    expect(poRow(render(<TxMeters radio={radio(null, true)} />).container).label).toBe('PO')
  })

  it('in every variant: the panel, the Operate cell and the TX strip', () => {
    for (const v of [{}, { inline: true }, { strip: true }]) {
      const where = JSON.stringify(v)
      expect(poRow(render(<TxMeters radio={radio(45, true)} {...v} />).container).label, where).toBe('PO≈')
      cleanup()
      expect(poRow(render(<TxMeters radio={radio(45)} {...v} />).container).label, where).toBe('PO')
      cleanup()
    }
  })
})
