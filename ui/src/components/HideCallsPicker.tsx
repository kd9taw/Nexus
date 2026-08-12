// The wildcard call-hide control for the Band Activity chip bar (F4MQS). A small popover —
// PORTALED for the same reason CountryExcludePicker is (the chip bar's clip would swallow an
// inline popover in the narrow rail) — with one text field. The list itself lives in
// features/hideCalls.ts; every pane subscribes, so a pane and its control never disagree.
import * as RM from '@radix-ui/react-dropdown-menu'
import { useEffect, useState } from 'react'
import { useHideCalls } from '../features/hideCalls'

export function HideCallsPicker() {
  const { entries, setEntries } = useHideCalls()
  // Local edit buffer so typing is smooth; committed on blur / Enter / menu close.
  const [text, setText] = useState(entries.join(' '))
  useEffect(() => setText(entries.join(' ')), [entries])
  const commit = () => setEntries(text)

  return (
    <RM.Root modal={false} onOpenChange={(open) => !open && commit()}>
      <RM.Trigger asChild>
        <button
          type="button"
          className={`od-chip${entries.length > 0 ? ' active' : ''}`}
          title="Hide callsigns (or VP8*-style prefixes) from this pane — a display filter only; decoding, logging, alerts and the auto-responder are untouched"
        >
          Hide calls{entries.length > 0 ? ` · ${entries.length}` : ''}
        </button>
      </RM.Trigger>
      <RM.Portal>
        <RM.Content className="ui-menu country-menu" sideOffset={4} align="end" collisionPadding={8}>
          <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
            <div className="country-menu-head">Hide these callsigns</div>
            <div style={{ padding: '0.4rem 0.6rem' }}>
              <input
                className="settings-input"
                type="text"
                value={text}
                placeholder="e.g. VP8* R0* K1ABC"
                autoComplete="off"
                spellCheck={false}
                onChange={(e) => setText(e.target.value)}
                onBlur={commit}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') commit()
                }}
                // Radix would treat typing as a typeahead over menu items; keep keys local.
                onKeyDownCapture={(e) => e.stopPropagation()}
                style={{ width: '16rem', maxWidth: '70vw' }}
              />
            </div>
            <div className="country-menu-note">
              Space-separated. A trailing <code>*</code> is a prefix ("VP8*" hides every VP8).
              A view filter only — the stations you are working and those calling you always
              show. To stop your auto-CQ from answering a call, Alt-double-click it instead.
            </div>
          </div>
        </RM.Content>
      </RM.Portal>
    </RM.Root>
  )
}
