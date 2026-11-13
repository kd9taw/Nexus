// The address that gets the "device added" notice, in the account card (index.ts, account/notice/*). That
// notice is what lets a new device's transmit hold lift, so the service moves it only through a code
// mailed to the new address, and then after a wait in which the old address is told; this view only asks
// and shows. Closed by default, and the service is not asked anything until it is opened.
import { useState } from 'react'
import { t } from '../i18n'
import type { BrowserClient } from './client'

type Notice = { address: string | null; pending: null | { address: string; verified: boolean; effectiveAt: number | null } }

export function NoticeAddress({ client, busy, act }: {
  client: BrowserClient | null; busy: boolean
  /** The page's own runner: one request at a time, and a refusal shown by name in its alert. */
  act: (work: () => Promise<void>) => Promise<void>
}) {
  const [notice, setNotice] = useState<Notice | null>(null)
  const [address, setAddress] = useState('')
  const [code, setCode] = useState('')
  const read = async () => { if (client) setNotice(await client.post<Notice>('account/notice')) }
  const step = (path: string, value: object = {}) => void act(async () => {
    await client?.post(path, value)
    setAddress(''); setCode('')
    await read()
  })
  const pending = notice?.pending ?? null
  return <details onToggle={event => { if ((event.currentTarget as HTMLDetailsElement).open && !notice) void act(read) }}>
    <summary>{t('remote.notice.title')}</summary>
    {notice && <>
      <p>{notice.address ? t('remote.notice.current', { address: notice.address }) : t('remote.notice.none')}</p>
      {pending?.verified && <p role="status">{t('remote.notice.waiting', { address: pending.address,
        date: new Date(pending.effectiveAt ?? 0).toISOString().slice(0, 16).replace('T', ' ') + ' UTC' })}</p>}
      {pending && !pending.verified && <form onSubmit={event => { event.preventDefault(); if (/^\d{8}$/.test(code)) step('account/notice/confirm', { code }) }}>
        <p role="status">{t('remote.notice.codeSent', { address: pending.address })}</p>
        <label>{t('remote.notice.code')}<input inputMode="numeric" autoComplete="one-time-code" maxLength={8} value={code}
          onChange={event => setCode(event.target.value.replace(/\D/g, ''))} /></label>
        <button className="remote-button remote-button--primary" disabled={busy || !/^\d{8}$/.test(code)}>{t('remote.notice.confirm')}</button>
      </form>}
      {pending && <div className="remote-actions">
        <button className="remote-button" type="button" disabled={busy} onClick={() => step('account/notice/cancel')}>{t('remote.notice.cancel')}</button>
      </div>}
      {!pending && <form onSubmit={event => { event.preventDefault(); if (address.trim()) step('account/notice/change', { address: address.trim() }) }}>
        <label>{t('remote.notice.newAddress')}<input type="email" autoComplete="email" maxLength={320} value={address}
          onChange={event => setAddress(event.target.value)} /></label>
        <button className="remote-button" disabled={busy || !address.trim()}>{t('remote.notice.send')}</button>
      </form>}
    </>}
  </details>
}
