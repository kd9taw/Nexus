# Station monitor development view

This foundation exposes station/radio/amplifier observations. It provides no remote
connection, TX, logging, tuning or amplifier command capability.

The shared React view accepts one `MonitorSource.read` capability. The native source
invokes only `get_remote_monitor_frame`, validates the v1 result and has no fixture
fallback. Rust copies a bounded observation without the full snapshot/logbook scan.
Native readers share a 500 ms cache and do not queue behind a busy engine. Stale
publication sequences cannot refresh the UI, and a hung read occupies just one slot.
Publication time is not a hardware measurement timestamp.

## Preview

From `ui`, run `npm exec -- vite --config vite.monitor.config.ts`, then open
`http://127.0.0.1:5189/monitor.html`. The page explicitly identifies its example
station. Preview controls select Rust-generated scenarios, pause updates and enlarge
the display. The default desktop build does not include this entry or its fixtures.
Build the separate preview with `npm exec -- vite build --config vite.monitor.config.ts`.

## Native development window

Run Nexus with the normal Tauri development workflow. From the main window's developer
console, call the existing panel-opening command:

```js
window.__TAURI_INTERNALS__.invoke('open_panel_window', { panel: 'remoteMonitor' })
```

The `?panel=remoteMonitor` route is development-only. It reads that process's actual
station state while the main Nexus window remains available. A plain browser using
this native route reports unavailable data; it never substitutes example readings.

## Contract fixtures and checks

The `remote_monitor_fixtures` Rust example serializes actual engine observations after
synthetic amp status/miss inputs. It does not connect to hardware. From the repository
root, regenerate and check the committed fixture with:

```sh
cargo run -p tempo-app --example remote_monitor_fixtures > ui/src/remote-monitor/fixtures.v1.json
cargo run -p tempo-app --example remote_monitor_fixtures -- --check
```

Run the repository's CI gates, including the standalone Tauri crate with `--features
radio`. Focused coverage lives in `crates/tempo-app/tests/remote_monitor.rs`, the Rust
monitor module/publisher tests and `ui/src/remote-monitor/*.test.*`. Browser geometry
and physical radio/amp tests provide different evidence; unit tests cannot establish
real-device layout, capture performance or RF stop behavior.
