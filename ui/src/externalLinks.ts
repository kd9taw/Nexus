// External-link interceptor — the one shared fix for every `<a target="_blank">` in the app.
//
// Raw `_blank` anchors are DEAD in the Tauri webview on every platform: tauri-plugin-opener's
// injected click handler (its Builder default `open_js_links_on_click`) preventDefaults the
// click and then invokes `plugin:opener|open_url`, which the capability ACL denies — we grant
// the opener no webview permission, deliberately (project convention is to open URLs from Rust
// commands like open_qrz_page; see capabilities/default.json). The click is swallowed with no
// fallback, so the CAT-driver download, the repeater-source credits, the contest-calendar
// rules links and the APRS station links all silently did nothing.
//
// This document-level listener routes those anchors through the `open_external_url` command
// instead. Ordering makes it safe against the plugin's own handler: clicks bubble through
// `document` BEFORE `window` (where the plugin listens), and the plugin bails on
// `e.defaultPrevented` — so our preventDefault also neutralizes its (denied) invoke, and a
// future ACL grant could never double-open. Unlike the plugin's handler this one accepts
// modifier clicks, so Cmd+click (the mac open-link reflex) works too.

/** The external http(s) URL a click on `target` should open, or null when the click is not
 * on an external `_blank` anchor. Internal navigation (relative/hash hrefs, anchors without
 * `target="_blank"`) is deliberately left alone — only the "open my browser" affordance is
 * routed. `a.href` resolves relative hrefs against the app origin, so the raw attribute is
 * checked for the scheme, not the resolved property. */
export function externalLinkHref(target: EventTarget | null): string | null {
  if (!(target instanceof Element)) return null
  const a = target.closest('a[href]')
  if (!(a instanceof HTMLAnchorElement) || a.target !== '_blank') return null
  const href = a.getAttribute('href') ?? ''
  return /^https?:\/\//i.test(href) ? href : null
}

/** Install the app-wide interceptor. `open` is injected (the api.ts command wrapper in the
 * app; a spy in tests). Returns the uninstall function. */
export function installExternalLinkInterceptor(
  open: (url: string) => void,
  doc: Document = document,
): () => void {
  const onClick = (e: MouseEvent) => {
    // Left button only; a click something else already handled stays handled.
    if (e.defaultPrevented || e.button !== 0) return
    const href = externalLinkHref(e.target)
    if (!href) return
    e.preventDefault()
    open(href)
  }
  doc.addEventListener('click', onClick)
  return () => doc.removeEventListener('click', onClick)
}
