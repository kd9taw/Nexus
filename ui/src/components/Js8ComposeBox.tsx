// THE JS8 COMPOSE BOX — the dock's message input with what JS8Call 3.0 draws in its own: PILLS for
// the parts of a message (who it is from, who it goes to, the command; js8Pills.ts) and a red wavy
// UNDERLINE on a word the JSC dictionary codes in pieces, whose right-click offers cheaper words
// (js8Suggest.ts). Both are on by default, as JS8Call ships them, and both are display only: what
// is sent is the text, framed by the station.
//
// HOW IT DRAWS. The input stays a plain <input> — the caret, selection, IME, undo and paste are the
// browser's — with a transparent background; a BACKDROP under it repeats the same text in the same
// font, transparent, and paints the pills' fills and the underlines behind the input's own letters.
// The backdrop takes the input's font and padding as the browser computed them and follows its
// horizontal scroll, so the marks stay under the letters they mark. It catches no pointer: a pill's
// tooltip comes from the input's own title, set from what is under the pointer.
//
// Migrated from birth (i18n/hardcoded-strings.test.ts): the command words, calls and suggested
// words are the mode's vocabulary; every sentence is from the catalog.
import { Fragment, useLayoutEffect, useMemo, useRef, useState, type RefObject } from 'react'
import * as RM from '@radix-ui/react-dropdown-menu'
import { MenuContent, MenuRoot } from './ui/MenuParts'
import { t } from '../i18n'
import { js8Pills, type Js8Pill, type Js8PillCommand } from '../js8Pills'
import { js8Words, useJs8FlaggedWords, type Js8Word } from '../js8Suggest'
import { js8WordSuggestions, type Js8WordSuggestions } from '../api'

interface Props {
  value: string
  onChange: (text: string) => void
  /** Enter: what Send does. */
  onEnter: () => void
  /** The call in the To box: a command typed with no address of its own goes to it. */
  toCall: string
  /** JS8Call's pills (Settings ▸ JS8, on by default). */
  pills: boolean
  /** JS8Call's word suggestions — the setting, where the station can answer (never the Remote). */
  suggest: boolean
  inputRef: RefObject<HTMLInputElement>
  placeholder: string
  ariaLabel: string
  /** Read-only: a MSG or relay going out, which JS8Call's box keeps as it was sent (typeahead). */
  readOnly?: boolean
  /** The box's own title: while set, it is shown in place of a pill's or an underline's. */
  title?: string
}

/** One stretch of the text and what is drawn under it. */
interface Part {
  start: number
  end: number
  pill: number
  flag: number
}

/** The text cut wherever a pill or an underline starts or ends. */
function partsOf(text: string, pills: readonly Js8Pill[], flags: readonly Js8Word[]): Part[] {
  const cuts = new Set([0, text.length])
  for (const p of pills) cuts.add(p.start).add(p.end)
  for (const f of flags) cuts.add(f.start).add(f.end)
  const at = [...cuts].filter((c) => c >= 0 && c <= text.length).sort((a, b) => a - b)
  const parts: Part[] = []
  for (let i = 0; i + 1 < at.length; i++) {
    const [start, end] = [at[i], at[i + 1]]
    parts.push({
      start,
      end,
      pill: pills.findIndex((p) => p.start <= start && end <= p.end),
      flag: flags.findIndex((f) => f.start <= start && end <= f.end),
    })
  }
  return parts
}

/** Literal keys per meaning, so the orphan guard sees each one referenced. */
function commandTitle(c: Js8PillCommand): string {
  switch (c) {
    case 'agnQ': return t('js8.pill.cmd.agnQ')
    case 'qslQ': return t('js8.pill.cmd.qslQ')
    case 'hwCpyQ': return t('js8.pill.cmd.hwCpyQ')
    case 'snrQ': return t('js8.pill.cmd.snrQ')
    case 'infoQ': return t('js8.pill.cmd.infoQ')
    case 'gridQ': return t('js8.pill.cmd.gridQ')
    case 'statusQ': return t('js8.pill.cmd.statusQ')
    case 'queryMsgs': return t('js8.pill.cmd.queryMsgs')
    case 'hearingQ': return t('js8.pill.cmd.hearingQ')
    case 'msgTo': return t('js8.pill.cmd.msgTo')
    case 'heartbeatSnr': return t('js8.pill.cmd.heartbeatSnr')
    case 'queryCall': return t('js8.pill.cmd.queryCall')
    case 'queryMsg': return t('js8.pill.cmd.queryMsg')
    case 'query': return t('js8.pill.cmd.query')
    case 'ditDit': return t('js8.pill.cmd.ditDit')
    case 'status': return t('js8.pill.cmd.status')
    case 'hearing': return t('js8.pill.cmd.hearing')
    case 'cmd': return t('js8.pill.cmd.cmd')
    case 'msg': return t('js8.pill.cmd.msg')
    case 'nack': return t('js8.pill.cmd.nack')
    case 'ack': return t('js8.pill.cmd.ack')
    case 'snr': return t('js8.pill.cmd.snr')
    case 'qsl': return t('js8.pill.cmd.qsl')
    case 'info': return t('js8.pill.cmd.info')
    case 'grid': return t('js8.pill.cmd.grid')
    case 'seventyThree': return t('js8.pill.cmd.seventyThree')
    case 'yes': return t('js8.pill.cmd.yes')
    case 'no': return t('js8.pill.cmd.no')
    case 'rr': return t('js8.pill.cmd.rr')
    case 'sk': return t('js8.pill.cmd.sk')
    case 'fb': return t('js8.pill.cmd.fb')
    case 'cq': return t('js8.pill.cmd.cq')
    case 'cqContest': return t('js8.pill.cmd.cqContest')
    case 'cqField': return t('js8.pill.cmd.cqField')
    case 'cqDx': return t('js8.pill.cmd.cqDx')
    case 'cqQrp': return t('js8.pill.cmd.cqQrp')
    case 'hb': return t('js8.pill.cmd.hb')
    case 'other': return t('js8.pill.cmd.other')
  }
}

/** A command's argument, named; the commands that take one say when it is missing. */
function argTitle(p: Js8Pill): string | null {
  const arg = p.arg ?? null
  switch (p.command) {
    case 'msgTo':
    case 'queryCall':
      return arg ? t('js8.pill.arg.call', { call: arg }) : t('js8.pill.arg.callMissing')
    case 'queryMsg':
      return arg ? t('js8.pill.arg.id', { id: arg }) : t('js8.pill.arg.idMissing')
    case 'grid':
      return arg ? t('js8.pill.arg.grid', { grid: arg }) : t('js8.pill.arg.gridMissing')
    case 'cq':
    case 'cqContest':
    case 'cqField':
    case 'cqDx':
    case 'cqQrp':
    case 'hb':
      return arg ? t('js8.pill.arg.grid', { grid: arg }) : null
    default:
      return null
  }
}

/** What a pill says on hover. */
export function js8PillTitle(p: Js8Pill): string {
  switch (p.kind) {
    case 'sender':
      return t('js8.pill.sender', { call: p.calls[0] ?? '' })
    case 'recipient':
      return t('js8.pill.recipient', { call: p.calls[0] ?? '' })
    case 'group':
      return t('js8.pill.group', { group: p.calls[0] ?? '' })
    case 'relay': {
      const to = p.calls[p.calls.length - 1] ?? ''
      return p.calls.length > 1
        ? t('js8.pill.relay.via', { via: p.calls.slice(0, -1).join(' › '), to })
        : t('js8.pill.relay.to', { to })
    }
    case 'command': {
      const what = commandTitle(p.command ?? 'other')
      const arg = argTitle(p)
      const said = arg ? t('js8.pill.withArg', { what, arg }) : what
      return p.toBox ? t('js8.pill.toBox', { what: said, call: p.toBox }) : said
    }
  }
}

export function Js8ComposeBox({ value, onChange, onEnter, toCall, pills: pillsOn, suggest, inputRef, placeholder, ariaLabel, readOnly = false, title: boxTitle }: Props) {
  const pills = useMemo(() => (pillsOn ? js8Pills(value, toCall) : []), [pillsOn, value, toCall])
  const flagged = useJs8FlaggedWords(value, suggest)
  const flags = useMemo(() => (suggest ? js8Words(value).filter((w) => flagged.has(w.word)) : []), [suggest, value, flagged])
  const parts = useMemo(() => partsOf(value, pills, flags), [value, pills, flags])
  const marked = pills.length > 0 || flags.length > 0
  const backRef = useRef<HTMLSpanElement>(null)
  const trackRef = useRef<HTMLSpanElement>(null)
  const [title, setTitle] = useState<string | undefined>(undefined)
  // The right-click menu: the underlined word it is for (its index among `flags`, whose span is the
  // menu's anchor) and the station's answer once it is in.
  const [menu, setMenu] = useState<{ flag: number; word: Js8Word; offers: Js8WordSuggestions | null } | null>(null)

  // The backdrop takes the input's text metrics and its scroll, after every render (the value moves
  // the scroll) and on every scroll, so a mark never sits under the wrong letter.
  const follow = () => {
    const input = inputRef.current
    const back = backRef.current
    const track = trackRef.current
    if (!input || !back || !track) return
    const cs = getComputedStyle(input)
    for (const prop of [
      'font-family', 'font-size', 'font-weight', 'font-style', 'font-stretch', 'font-kerning',
      'font-variant', 'font-feature-settings', 'letter-spacing', 'word-spacing',
      'padding-left', 'padding-right', 'border-left-width', 'border-right-width',
    ]) {
      back.style.setProperty(prop, cs.getPropertyValue(prop))
    }
    track.style.transform = `translateX(${-input.scrollLeft}px)`
  }
  useLayoutEffect(follow)

  /** The pill or the underlined word under the pointer, by the backdrop's own boxes. */
  const hit = (x: number, y: number): { pill: number; flag: number } => {
    const inside = (el: Element) => {
      const r = el.getBoundingClientRect()
      return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom
    }
    const back = backRef.current
    const pill = [...(back?.querySelectorAll<HTMLElement>('[data-pill]') ?? [])].find(inside)
    const flag = [...(back?.querySelectorAll<HTMLElement>('[data-flag]') ?? [])].find(inside)
    return { pill: pill ? Number(pill.dataset.pill) : -1, flag: flag ? Number(flag.dataset.flag) : -1 }
  }

  /** A suggestion in place of the word it was offered for — if the text still holds that word. */
  const take = (word: Js8Word, offer: string) => {
    if (value.slice(word.start, word.end).toUpperCase() !== word.word) return
    const next = value.slice(0, word.start) + offer + value.slice(word.end)
    onChange(next)
    const caret = word.start + offer.length
    requestAnimationFrame(() => {
      const input = inputRef.current
      input?.focus({ preventScroll: true })
      input?.setSelectionRange(caret, caret)
    })
  }

  return (
    <MenuRoot open={menu !== null} onOpenChange={(open) => { if (!open) setMenu(null) }}>
      <span className="js8-compose-wrap">
        {marked && (
          <span className="js8-compose-backdrop" ref={backRef} aria-hidden="true">
            <span className="js8-compose-track" ref={trackRef}>
              {parts.map((part, i) => {
                const text = value.slice(part.start, part.end)
                const flagSpan = <span className="js8-word-flag" data-flag={part.flag}>{text}</span>
                // The word a right-click menu is open for anchors it, so the menu stands by the word.
                const anchors = menu !== null && part.flag === menu.flag && (i === 0 || parts[i - 1].flag !== part.flag)
                const inner = part.flag < 0 ? text : anchors ? <RM.Trigger asChild>{flagSpan}</RM.Trigger> : flagSpan
                if (part.pill < 0) return <Fragment key={i}>{inner}</Fragment>
                const p = pills[part.pill]
                // A pill split by an underline is drawn in pieces, each one carrying the pill.
                const first = i === 0 || parts[i - 1].pill !== part.pill
                const last = i + 1 === parts.length || parts[i + 1].pill !== part.pill
                return (
                  <span
                    key={i}
                    className={`js8-pill ${p.kind}${first ? ' first' : ''}${last ? ' last' : ''}`}
                    data-pill={part.pill}
                  >
                    {inner}
                  </span>
                )
              })}
            </span>
          </span>
        )}
        <input
          ref={inputRef}
          className="settings-input cw-type js8-compose"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          readOnly={readOnly}
          onKeyDown={(e) => {
            if (e.key !== 'Enter') return
            e.preventDefault()
            onEnter()
          }}
          onScroll={follow}
          onSelect={follow}
          onMouseMove={(e) => {
            const h = hit(e.clientX, e.clientY)
            const next = h.pill >= 0 ? js8PillTitle(pills[h.pill]) : h.flag >= 0 ? t('js8.compose.flag.title') : undefined
            if (next !== title) setTitle(next)
          }}
          onMouseLeave={() => setTitle(undefined)}
          onContextMenu={(e) => {
            const h = hit(e.clientX, e.clientY)
            // A read-only box takes no word in place of another.
            if (h.flag < 0 || readOnly) return
            e.preventDefault()
            const word = flags[h.flag]
            setMenu({ flag: h.flag, word, offers: null })
            void js8WordSuggestions(word.word)
              .then((offers) => setMenu((m) => (m && m.word === word ? { ...m, offers } : m)))
              .catch(() => setMenu((m) => (m && m.word === word ? { ...m, offers: { words: [], found: true } } : m)))
          }}
          title={boxTitle ?? title}
          placeholder={placeholder}
          aria-label={ariaLabel}
          autoComplete="off"
          spellCheck={false}
        />
        <RM.Portal>
          <MenuContent
            className="ui-menu js8-suggest-menu"
            side="top"
            align="start"
            sideOffset={4}
            collisionPadding={8}
            aria-label={t('js8.compose.suggest.aria')}
            onCloseAutoFocus={(e) => {
              e.preventDefault()
              inputRef.current?.focus({ preventScroll: true })
              inputRef.current?.scrollIntoView?.({ block: 'nearest' })
            }}
          >
            {/* The portal escapes `.app`'s zoom, so the content re-applies it (Menu.tsx's reason). */}
            <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
              {menu?.offers == null ? (
                <RM.Item className="ui-menu-item" disabled>
                  {t('js8.compose.suggest.asking')}
                </RM.Item>
              ) : menu.offers.words.length === 0 ? (
                <RM.Item className="ui-menu-item" disabled>
                  {t('js8.compose.suggest.none')}
                </RM.Item>
              ) : (
                menu.offers.words.map((w) => (
                  <RM.Item key={w} className="ui-menu-item js8-suggest-item" onSelect={() => take(menu.word, w)}>
                    {w}
                  </RM.Item>
                ))
              )}
            </div>
          </MenuContent>
        </RM.Portal>
      </span>
    </MenuRoot>
  )
}
