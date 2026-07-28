# E2E smoke — launch the real app, prove the UI opens

`smoke.mjs` drives the actual Nexus binary through [tauri-driver] (WebDriver)
and asserts the three things unit tests structurally cannot: the app starts,
the first-run wizard renders on a fresh profile, and the Settings panel opens
(the exact class of the public crash-on-Settings incident). Zero npm
dependencies — plain Node 18+ `fetch` speaking the WebDriver protocol.

## Run it locally (Linux)

```sh
# once: the driver pair
cargo install tauri-driver --locked
sudo apt-get install webkit2gtk-driver xvfb dbus   # WebKitWebDriver + headless X

# build the app (assets embedded; radio because src-tauri needs it)
npm --prefix ui ci && npm --prefix ui run build
NEXUS_ALLOW_MISSING_AICW=1 cargo build --manifest-path src-tauri/Cargo.toml \
  --features radio,custom-protocol

# run — ALWAYS with an isolated profile: on a real profile this would mutate
# YOUR app state (and the wizard assertion needs a fresh one anyway)
FAKEHOME=$(mktemp -d)
xvfb-run -a dbus-run-session -- env \
  HOME="$FAKEHOME" XDG_CONFIG_HOME="$FAKEHOME/.config" \
  XDG_DATA_HOME="$FAKEHOME/.local/share" XDG_CACHE_HOME="$FAKEHOME/.cache" \
  WEBKIT_DISABLE_COMPOSITING_MODE=1 \
  NEXUS_E2E_BINARY=src-tauri/target/debug/Nexus \
  node e2e/smoke.mjs
```

`NATIVE_DRIVER=/path/to/WebKitWebDriver` overrides driver lookup (useful when
webkit2gtk-driver was extracted rather than installed). On failure the script
saves what the operator would have seen to `e2e/artifacts/failure.png`.

CI runs this in the `e2e-linux` job (ci.yml). The flows live in one file on
purpose — add a flow only when it guards a real incident class, and keep the
assertions on things an operator can see.

[tauri-driver]: https://v2.tauri.app/develop/tests/webdriver/
