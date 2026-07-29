// The destination a calendar entry opens. Getting this wrong is worse than a
// dead button: it sends the operator somewhere and claims it is the expedition.
import { describe, it, expect } from 'vitest'
import { dxpedLink, dxpedLinkTitle, baseCall } from './dxpedLink'

describe('dxpedLink', () => {
  it('prefers the operation\'s own site when the feed published one', () => {
    const link = dxpedLink({ call: '3Y0L', website: 'https://3y0l.com/' })
    expect(link).toEqual({ kind: 'site', url: 'https://3y0l.com/', label: 'Website' })
  })

  it('falls back to QRZ when no site was announced', () => {
    // Two thirds of NG3K announcements carry no site, so this is the COMMON path,
    // not the edge case.
    for (const website of [undefined, null, '', '   ']) {
      expect(dxpedLink({ call: 'TY5FR', website })).toEqual({
        kind: 'qrz',
        url: 'https://www.qrz.com/db/TY5FR',
        label: 'QRZ',
      })
    }
  })

  it('never offers a non-http scheme, it offers QRZ instead', () => {
    // The site is scraped from third-party HTML. The backend refuses these too —
    // this is the UI half, so the affordance never even displays the hostile URL.
    for (const bad of ['javascript:alert(1)', 'file:///etc/passwd', '/relative', 'ftp://x.example']) {
      expect(dxpedLink({ call: '3Y0L', website: bad })?.kind).toBe('qrz')
    }
  })

  it('drops a portable designator from the QRZ fallback', () => {
    expect(dxpedLink({ call: 'PJ4/K1ABC' })?.url).toBe('https://www.qrz.com/db/K1ABC')
    expect(dxpedLink({ call: 'K1ABC/P' })?.url).toBe('https://www.qrz.com/db/K1ABC')
  })

  it('offers nothing at all when there is no callsign to fall back on', () => {
    expect(dxpedLink({ call: '' })).toBeNull()
    expect(dxpedLink({ call: '///' })).toBeNull()
  })
})

describe('baseCall', () => {
  it('keeps the longest slash-separated token and strips punctuation', () => {
    expect(baseCall('3Y0J/MM')).toBe('3Y0J')
    expect(baseCall('vk9cm')).toBe('VK9CM')
  })
})

describe('dxpedLinkTitle', () => {
  it('names the destination, and says when it is the fallback', () => {
    expect(dxpedLinkTitle({ kind: 'site', url: 'https://3y0l.com/', label: 'Website' })).toContain(
      'https://3y0l.com/',
    )
    const qrz = dxpedLinkTitle({
      kind: 'qrz',
      url: 'https://www.qrz.com/db/TY5FR',
      label: 'QRZ',
    })
    expect(qrz).toContain('No website announced')
    expect(qrz).toContain('https://www.qrz.com/db/TY5FR')
  })
})
