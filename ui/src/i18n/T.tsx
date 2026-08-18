// <T> — translated text that carries emphasis.
//
// Split from ./index.ts so the runtime stays React-free and node-testable (the settings
// registry guard imports it from a node test).
//
// Use `t()` for attributes and plain strings; use this when the sentence has a `<b>…</b>`,
// a `<code>…</code>` or a link in the middle of it. The alternative — three keys glued
// together — cannot be translated into a language that orders the sentence differently, which
// is most of them.
//
//   <T k="reveal.prompt" tags={{ b: <strong /> }} vals={{ achievement, feature }} />
//
// The elements come from HERE, never from the catalog: markers are matched by name against
// `tags`, and any `<…>` without a matching entry renders as literal text. So no catalog string
// — including a machine-translated one — can introduce an element, an attribute or a handler.
// Markers are parsed before values are substituted, so a value containing `<b>` is text too.

import { cloneElement, Fragment, type ReactElement, type ReactNode } from 'react'
import { interpolate, parseRich, rawMessage, type MessageKey, type Params } from './index'
import type { RichNode } from './types'

interface Props {
  /** Catalog key. */
  k: MessageKey
  /** `{{name}}` values. `count` additionally selects the plural form. */
  vals?: Params
  /** Marker name → the element to wrap that span in. `<b />` may be any element you like. */
  tags?: Record<string, ReactElement>
}

function render(nodes: RichNode[], tags: Record<string, ReactElement>, vals?: Params): ReactNode[] {
  return nodes.map((node, i) =>
    typeof node === 'string' ? (
      <Fragment key={i}>{interpolate(node, vals)}</Fragment>
    ) : (
      cloneElement(tags[node.tag], { key: i }, ...render(node.children, tags, vals))
    ),
  )
}

export function T({ k, vals, tags }: Props): ReactElement {
  const markers = tags ?? {}
  return <>{render(parseRich(rawMessage(k, vals), Object.keys(markers)), markers, vals)}</>
}
