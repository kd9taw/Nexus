// The wire vocabulary of a self-spot's two results, and nothing else.
//
// This is a LEAF module on purpose. `remote-web/operation-protocol.ts` validates a station's
// self-spot receipt against these lists, and the Cloudflare Worker (remote/src/room.ts) imports
// that protocol module — under a tsconfig with no DOM lib and no JSX. Keeping the lists here,
// away from `selfSpot.ts` and its confirm dialog, toasts and catalog lookups, is what stops the
// browser half of the app being dragged into the Worker's type-check and its bundle.
// Add nothing to this file that touches the DOM, the i18n catalog or React.

/** What each target did — mirrors `self_spot::Report` in src-tauri. */
export const POTA_SPOT_RESULTS = ['posted', 'loginRequired', 'failed', 'throttled', 'notPark', 'unknownMode', 'invalid'] as const
export const CLUSTER_SPOT_RESULTS = ['queued', 'unavailable', 'failed', 'throttled', 'invalid'] as const
export type SelfSpotReport = {
  pota: (typeof POTA_SPOT_RESULTS)[number]
  cluster: (typeof CLUSTER_SPOT_RESULTS)[number]
}
