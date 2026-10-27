# Station monitor frames

The station's observation frame and what reads it. The folder exposes no remote connection, and
no TX, logging, tuning or amplifier command.

- `protocol.ts` — the frame's grammar, bounded and validated (`parseFrame`), with its age and
  order rules.
- `session.ts` — `startMonitor`: polls one `MonitorSource.read` capability, and publishes a
  current, unavailable or invalid state. The stream page reads the station's frames through it.
- `relay.ts` — the relay core the Remote service runs (`remote/src/room.ts`): observers, their
  leases and acknowledgements, and the `Entitlement` the service and the browser share.
- `monitor.css` — the shell the Remote pages share: the account page, the stream and the
  stations window on this network.
- `fixtures.v2.json` and `fixtureSource.ts` — recorded frames, for tests only.

The development observer view that used to live here, with its preview page and its `?panel=`
window, has been removed. The Remote stream and the full client are what a remote operator uses.

## Contract fixtures and checks

The `remote_monitor_fixtures` Rust example serializes actual engine observations after
synthetic amp status/miss inputs, plus explicit example CAT/keyed states for protocol
rendering coverage. Those CAT examples are not available native readbacks. It does
not connect to hardware. From the repository root, regenerate and check the committed fixture
with:

```sh
cargo run -p tempo-app --example remote_monitor_fixtures > ui/src/remote-monitor/fixtures.v2.json
cargo run -p tempo-app --example remote_monitor_fixtures -- --check
```

Focused coverage lives in `crates/tempo-app/tests/remote_monitor.rs`, the Rust monitor
module/publisher tests and `ui/src/remote-monitor/*.test.*`. Unit tests cannot establish
real-device layout, capture performance or RF stop behavior.
