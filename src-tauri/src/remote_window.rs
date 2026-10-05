//! The Remote stations window: the hosted Remote page, in a Nexus window of its own, on the PC an
//! operator works from. It is the browser tab, without the browser.
//!
//! **What it is.** One window, [`LABEL`], showing [`REMOTE_ORIGIN`] — the live Remote service the
//! station half of Remote already talks to — with the same sign-in, pairing and stream a browser
//! gets. The page keeps its cookies and its browser key in this app's web profile, as it would in a
//! browser's; Nexus reads none of it, and the profile is a browser of its own to the station, so
//! it is approved at the radio once like any other. What the window adds over a tab:
//! - **Full screen with Esc still a Stop.** F11 puts the WINDOW in the operating system's full
//!   screen, through Tauri. That is not the page's Fullscreen API, so nothing takes Esc to leave
//!   it: Esc stays an ordinary key, and over the picture it is the page's Stop TX. F11 itself is
//!   kept here and never reaches the page or the shack; Esc is never touched.
//! - **No browser shortcuts in the way.** WebView2's own accelerators (F5 and Ctrl+R reload, Ctrl+P
//!   print, Ctrl+F find, Alt+Left back…) are switched off, so those keys reach the page and, through
//!   the stream, the shack. Tauri 2.11 does not pass wry's switch for this through, so
//!   `quiet_browser_keys` sets `AreBrowserAcceleratorKeysEnabled` on the webview itself.
//! - **Its own place on the desktop.** Size and position are remembered, and clamped on load to
//!   the monitors attached then (`window_state`, the main window's policy). Full screen is not
//!   remembered: a window closed full screen reopens at its last windowed box.
//!
//! **What it is not: a bridge.** The page reaches no Nexus command. Tauri 2.11 refuses every
//! command — the app's own and every plugin's — to a page that is not the app's own unless a
//! capability names a remote URL (`webview/mod.rs`, `on_message`), and none does; the tests below
//! hold that against the access list this very build compiles in, and fail if any capability ever
//! names one. No command, event or channel exists for the page, the app stores no token, and the
//! page's own CSP and HSTS apply untouched. Tauri still injects its IPC script, and its plugins
//! theirs, into every page, so the page can ASK; every ask is refused. One Tauri request is exempt
//! from that check until Tauri 3 (`plugin:__TAURI_CHANNEL__|fetch`, which hands over a large
//! `tauri::ipc::Channel` message by a counter id from one app-wide queue). Nexus sends nothing
//! through a Channel, and its own windows call over the IPC protocol, so that queue stays empty;
//! a first `Channel` in this app reopens the question.
//!
//! **Where it may go** ([`route`]). The Remote origin and the sign-in issuer the service publishes
//! open here; so do the steps of a sign-in that left the Remote page for the issuer, until it comes
//! back. Measured on 2026-10-04: the issuer's Google sign-in goes to accounts.google.com and comes
//! back through Auth0's shared login host, neither of them the Remote origin or the issuer, so a
//! window held to those two alone could never finish a Google sign-in. Any other web link opens in
//! the system browser — a new window always does, never a second window of this app — and every
//! other kind of address (file:, javascript:, a custom scheme) goes nowhere.
//!
//! **Windows only, for now.** It is WebView2, the same Chromium WebRTC and WebCodecs the page is
//! tested on in Chrome and Edge. On Linux, WebKitGTK ships without WebRTC in most distributions'
//! builds, so the stream cannot start; on macOS, WKWebView has WebRTC but no `AudioDecoder` before
//! Safari 26, so the station's audio could not play. Neither is offered: the command refuses, and
//! Settings says to use a browser there.
//!
//! **Its sibling for Remote over this network: the Stations on this network window** ([`LAN_LABEL`],
//! the operator's ruling of 2026-10-04, "Window only in v1"). The same window for a station paired
//! over the shack's own network, with no internet: the stream page bundled in this Nexus, served
//! from a loopback origin of this computer's own (`lan_client::Origin`), which Tauri counts as a
//! remote page, so it too reaches no command. That origin runs while the window is open and ends
//! with it. The window shows its own page and nothing else ([`route_lan`]): any other web page goes
//! to the system browser, and this computer's own loopback and app hosts go nowhere. It asks the
//! Remote service for nothing, so it opens with no internet. Same F11 and browser keys as above.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{Manager, Url, WebviewUrl, WebviewWindowBuilder};

use crate::lan_client::{computer_name, Origin, Reach, Stations};
use crate::remote_service::vault::SystemVault;
use crate::remote_service::REMOTE_ORIGIN;
use crate::window_state;

/// The window's label. In no capability's `windows` list, so it gets nothing even as a local page.
pub const LABEL: &str = "remote-stations";

/// The Stations on this network window's label, in no capability's `windows` list either.
pub const LAN_LABEL: &str = "lan-stations";

/// The languages the Stations on this network window's page can be opened in: the catalogs it
/// ships with.
const LOCALES: [&str; 5] = ["en", "de", "es", "fr", "ja"];

/// Opening size, in logical px: room for the shack's window and the page's own controls.
const DEFAULT_INNER: (f64, f64) = (1280.0, 800.0);

/// The smallest the window can be dragged to. The page lays itself out down to a phone's width, so
/// this only keeps the window a window.
pub(crate) const MIN_INNER: (f64, f64) = (640.0, 480.0);

/// The Remote service did not answer with a sign-in issuer. Fixed codes, like the station
/// transport's: an HTTP error's own text can carry a URL.
const UNREACHABLE: &str = "remoteUnreachable";

/// The service answered, but the window itself could not be made.
const NOT_OPENED: &str = "remoteWindowFailed";

/// The answer on Linux and macOS, where the window is not offered (see the module header).
const UNAVAILABLE: &str = "remoteWindowUnavailable";

/// The service's public settings are a few hundred bytes; anything past this is not them.
const CONFIG_LIMIT: usize = 16384;

/// What the window does with an address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    /// Show it here.
    InWindow,
    /// Hand it to the operator's default browser, and stay where we are.
    SystemBrowser,
    /// Do nothing with it.
    Refuse,
}

/// Where a top-level navigation to `target` goes, from the page the window shows now (`from`,
/// `None` before the first page loads) with `issuer` the sign-in issuer the service publishes.
///
/// - Not http or https: refused. Only web pages are shown here or handed to a browser.
/// - The Remote origin, or the issuer: here.
/// - Anything else over https, while the window is away from the Remote page: here. The window
///   only ever leaves the Remote page for the issuer (the two rules above), so this admits the
///   steps of a sign-in or sign-out, and ends when it lands back on the Remote page. A host that
///   is the app's own (`localhost`, `*.localhost`) never opens here.
/// - Everything else, a link from the Remote page included: the system browser.
fn route(target: &Url, from: Option<&Url>, issuer: &Url) -> Route {
    if !matches!(target.scheme(), "http" | "https") {
        return Route::Refuse;
    }
    let remote = remote_url();
    if target.origin() == remote.origin() || target.origin() == issuer.origin() {
        return Route::InWindow;
    }
    let signing_in = from.is_some_and(|page| page.origin() != remote.origin());
    let local = target
        .host_str()
        .is_some_and(|h| h == "localhost" || h.ends_with(".localhost"));
    if signing_in && target.scheme() == "https" && !local {
        return Route::InWindow;
    }
    Route::SystemBrowser
}

/// Where a request for a NEW window goes (`target="_blank"`, `window.open`): never a window of
/// this app. A web address goes to the system browser, anything else nowhere.
fn route_new_window(target: &Url) -> Route {
    if matches!(target.scheme(), "http" | "https") {
        Route::SystemBrowser
    } else {
        Route::Refuse
    }
}

/// Where a navigation in the Stations on this network window goes, with `page` the address it
/// opened at: its own page's origin here; any other web page to the system browser, except this
/// computer's own hosts (loopback, `localhost`, `*.localhost`), which go nowhere; and everything
/// that is not a web page nowhere.
fn route_lan(target: &Url, page: &Url) -> Route {
    if !matches!(target.scheme(), "http" | "https") {
        return Route::Refuse;
    }
    if target.origin() == page.origin() {
        return Route::InWindow;
    }
    let own = target.host_str().is_none_or(|host| {
        host == "localhost"
            || host.ends_with(".localhost")
            || host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback() || ip.is_unspecified())
    });
    if own {
        Route::Refuse
    } else {
        Route::SystemBrowser
    }
}

/// [`REMOTE_ORIGIN`] as a URL.
fn remote_url() -> Url {
    Url::parse(REMOTE_ORIGIN).expect("REMOTE_ORIGIN is a URL")
}

/// The issuer the service publishes, as the page itself accepts it (`client.ts`): https, with no
/// credentials, query or fragment. `None` for anything else, and the window then does not open.
fn issuer_origin(issuer: &str) -> Option<Url> {
    let url = Url::parse(issuer).ok()?;
    let plain = url.scheme() == "https"
        && url.host_str().is_some_and(|h| !h.is_empty())
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none();
    plain.then_some(url)
}

/// Ask the Remote service for its sign-in issuer (`GET /api/remote/config`, public, no
/// credentials). Asked at every open, so the service can change its sign-in provider without a
/// Nexus release.
async fn sign_in_issuer() -> Result<Url, String> {
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| UNREACHABLE)?;
    let response = client
        .get(format!("{REMOTE_ORIGIN}/api/remote/config"))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_| UNREACHABLE)?;
    let body = response.bytes().await.map_err(|_| UNREACHABLE)?;
    if body.len() > CONFIG_LIMIT {
        return Err(UNREACHABLE.into());
    }
    let config: serde_json::Value = serde_json::from_slice(&body).map_err(|_| UNREACHABLE)?;
    config
        .get("issuer")
        .and_then(serde_json::Value::as_str)
        .and_then(issuer_origin)
        .ok_or_else(|| UNREACHABLE.into())
}

/// Hand a web address to the operator's default browser. Off this thread: it is called from
/// inside WebView2's own navigation event, which must not wait on the shell.
fn open_in_browser(app: &tauri::AppHandle, url: &Url) {
    use tauri_plugin_opener::OpenerExt;
    let (app, url) = (app.clone(), url.to_string());
    std::thread::spawn(move || {
        let _ = app.opener().open_url(url, None::<&str>);
    });
}

/// A click the page would open in a new window, opened through the page's own `window.open`, so
/// that [`route_new_window`] hands it to the system browser. Without this the click goes nowhere:
/// tauri-plugin-opener listens on `window` for exactly these clicks in every webview, cancels them
/// and asks the app to open the link, and that ask is refused here because this page may call
/// nothing. A listener on `document` hears the click first, and the plugin leaves a cancelled click
/// alone. Same clicks as the plugin's: the main button on a link with `target="_blank"`, or with
/// Ctrl or Shift held, and no Alt or Meta.
const NEW_WINDOW_LINKS: &str = r#"(function () {
  document.addEventListener('click', function (e) {
    if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.altKey) return;
    var a = e.composedPath().find(function (n) {
      return n instanceof Node && n.nodeName.toUpperCase() === 'A';
    });
    if (!a || !a.href || (a.target !== '_blank' && !e.ctrlKey && !e.shiftKey)) return;
    e.preventDefault();
    window.open(a.href, '_blank', 'noopener');
  });
})();"#;

/// Open the Remote stations window, or bring it forward when it is already open.
///
/// Called by Settings ▸ Station ▸ Remote access on this PC's own window; the Remote page cannot
/// call it (or anything else). Async, as every window-making command must be on Windows.
#[tauri::command]
pub async fn open_remote_stations_window(app: tauri::AppHandle) -> Result<(), String> {
    if !cfg!(windows) {
        return Err(UNAVAILABLE.into());
    }
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }
    let issuer = sign_in_issuer().await?;
    // The page the window shows now, for `route`: set as each page starts to load.
    let shown: Arc<Mutex<Option<Url>>> = Arc::default();
    let showing = shown.clone();
    let (to_browser, new_to_browser) = (app.clone(), app.clone());
    let (w, h, place) = match window_state::restore_remote(&app, MIN_INNER) {
        Some(r) => (r.w, r.h, Some((r.position, r.maximized))),
        None => (DEFAULT_INNER.0, DEFAULT_INNER.1, None),
    };
    let mut builder = WebviewWindowBuilder::new(&app, LABEL, WebviewUrl::External(remote_url()))
        .title("Nexus — Remote stations")
        .inner_size(w, h)
        .min_inner_size(MIN_INNER.0, MIN_INNER.1)
        // As in a browser, a drag inside the page is the page's (the main window's reason too).
        .disable_drag_drop_handler()
        .initialization_script(NEW_WINDOW_LINKS)
        .on_page_load(move |_, payload| {
            if payload.event() == PageLoadEvent::Started {
                *showing.lock().unwrap_or_else(|e| e.into_inner()) = Some(payload.url().clone());
            }
        })
        .on_navigation(move |target| {
            let from = shown.lock().unwrap_or_else(|e| e.into_inner()).clone();
            match route(target, from.as_ref(), &issuer) {
                Route::InWindow => true,
                Route::SystemBrowser => {
                    open_in_browser(&to_browser, target);
                    false
                }
                Route::Refuse => false,
            }
        })
        .on_new_window(move |target, _| {
            if route_new_window(&target) == Route::SystemBrowser {
                open_in_browser(&new_to_browser, &target);
            }
            NewWindowResponse::Deny
        })
        // The outer window, title bar included, fitted to the work area it opens on.
        .prevent_overflow();
    builder = match place {
        Some((Some((x, y)), _)) => builder.position(x, y),
        // A first open, or a saved place on a monitor that is gone.
        _ => builder.center(),
    };
    if matches!(place, Some((_, true))) {
        builder = builder.maximized(true);
    }
    let window = builder.build().map_err(|_| NOT_OPENED)?;
    window_state::arm_remote_capture(&window);
    #[cfg(windows)]
    quiet_browser_keys(&window);
    Ok(())
}

/// The loopback origin the Stations on this network window's page comes from, while the window is
/// open.
#[derive(Default)]
pub struct LanOrigin(Mutex<Option<Origin>>);

/// What the window's page can reach: the app's own embedded files (the same the main window
/// loads, so nothing is read from disk and nothing can differ from the shipped page), this
/// computer's paired stations in the OS credential store, and the stations Windows' own DNS-SD
/// finds by name.
fn reach(app: &tauri::AppHandle) -> Reach {
    let files = app.clone();
    Reach {
        assets: Arc::new(move |path: &str| {
            let resolver = files.asset_resolver();
            let asset = resolver
                .get(format!("/{path}"))
                .or_else(|| resolver.get(path.to_string()))?;
            Some((asset.bytes, asset.mime_type))
        }),
        stations: Arc::new(Stations::new(Arc::new(SystemVault))),
        find: Arc::new(tempo_stream::lan::dnssd::find),
        name: computer_name(),
    }
}

/// Open the Stations on this network window, or bring it forward when it is already open, with
/// its page in `locale` (one of [`LOCALES`]; otherwise the page's own choice).
///
/// Called by Settings ▸ Station ▸ Remote access on this PC's own window; the window's page cannot
/// call it (or anything else).
#[tauri::command]
pub async fn open_lan_stations_window(
    app: tauri::AppHandle,
    origin: tauri::State<'_, LanOrigin>,
    locale: Option<String>,
) -> Result<(), String> {
    if !cfg!(windows) {
        return Err(UNAVAILABLE.into());
    }
    if let Some(window) = app.get_webview_window(LAN_LABEL) {
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }
    let page = {
        let mut running = origin.0.lock().map_err(|_| NOT_OPENED)?;
        if running.is_none() {
            *running = Some(Origin::start(reach(&app)).map_err(|_| NOT_OPENED)?);
        }
        running.as_ref().map(Origin::page).ok_or(NOT_OPENED)?
    };
    let mut page = Url::parse(&page).map_err(|_| NOT_OPENED)?;
    if let Some(locale) = locale.filter(|l| LOCALES.contains(&l.as_str())) {
        page.query_pairs_mut().append_pair("lang", &locale);
    }
    let (here, here_too) = (page.clone(), page.clone());
    let (to_browser, new_to_browser) = (app.clone(), app.clone());
    let window = WebviewWindowBuilder::new(&app, LAN_LABEL, WebviewUrl::External(page))
        .title("Nexus — Stations on this network")
        .inner_size(DEFAULT_INNER.0, DEFAULT_INNER.1)
        .min_inner_size(MIN_INNER.0, MIN_INNER.1)
        .disable_drag_drop_handler()
        .on_navigation(move |target| match route_lan(target, &here) {
            Route::InWindow => true,
            Route::SystemBrowser => {
                open_in_browser(&to_browser, target);
                false
            }
            Route::Refuse => false,
        })
        .on_new_window(move |target, _| {
            if route_lan(&target, &here_too) == Route::SystemBrowser {
                open_in_browser(&new_to_browser, &target);
            }
            NewWindowResponse::Deny
        })
        .prevent_overflow()
        .center()
        .build();
    let window = match window {
        Ok(window) => window,
        Err(_) => {
            if let Ok(mut running) = origin.0.lock() {
                running.take();
            }
            return Err(NOT_OPENED.into());
        }
    };
    // The origin lives as long as the window: closing it ends the page's session, and any road
    // to a station with it, which ends that computer's stream and lease at the station.
    let closing = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            if let Some(origin) = closing.try_state::<LanOrigin>() {
                if let Ok(mut running) = origin.0.lock() {
                    running.take();
                }
            }
        }
    });
    #[cfg(windows)]
    quiet_browser_keys(&window);
    Ok(())
}

/// WebView2's browser accelerators off, and F11 for the window's own full screen.
///
/// `AreBrowserAcceleratorKeysEnabled` false hands F5, Ctrl+R, Ctrl+P, Ctrl+F, F12, Alt+Left and the
/// rest to the page instead of acting on them; editing keys (Ctrl+C/V/X/A/Z) are not affected.
/// The `AcceleratorKeyPressed` handler sees F11 before the page does: it keeps F11, press and
/// release, and toggles the window's full screen on the press (not on auto-repeat). The toggle runs
/// on a thread of its own because this handler is inside WebView2's event, where resizing the
/// window under it is not safe. No other key is touched; Esc in particular always reaches the page.
#[cfg(windows)]
fn quiet_browser_keys(window: &tauri::WebviewWindow) {
    use webview2_com::AcceleratorKeyPressedEventHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Settings3, COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
        COREWEBVIEW2_KEY_EVENT_KIND_KEY_UP, COREWEBVIEW2_PHYSICAL_KEY_STATUS,
    };
    use windows_core::Interface;

    /// `VK_F11` (winuser.h).
    const VK_F11: u32 = 0x7A;

    let target = window.clone();
    let _ = window.with_webview(move |webview| {
        let controller = webview.controller();
        // SAFETY: plain COM calls on the live controller of this window's own webview, on the
        // thread WebView2 created it on (`with_webview` runs there).
        unsafe {
            if let Ok(settings) = controller
                .CoreWebView2()
                .and_then(|core| core.Settings())
                .and_then(|settings| settings.cast::<ICoreWebView2Settings3>())
            {
                let _ = settings.SetAreBrowserAcceleratorKeysEnabled(false);
            }
            let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else {
                    return Ok(());
                };
                let mut key = 0u32;
                args.VirtualKey(&mut key)?;
                let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
                args.KeyEventKind(&mut kind)?;
                if key != VK_F11
                    || !(kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                        || kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_UP)
                {
                    return Ok(());
                }
                args.SetHandled(true)?;
                let mut status = COREWEBVIEW2_PHYSICAL_KEY_STATUS::default();
                args.PhysicalKeyStatus(&mut status)?;
                if kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN && !status.WasKeyDown.as_bool() {
                    let window = target.clone();
                    std::thread::spawn(move || {
                        let full = window.is_fullscreen().unwrap_or(false);
                        let _ = window.set_fullscreen(!full);
                    });
                }
                Ok(())
            }));
            let mut token = 0i64;
            let _ = controller.add_AcceleratorKeyPressed(&handler, &mut token);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The issuer the live service published on 2026-10-04, shape only: the tests need an
    /// issuer, not the tenant.
    const ISSUER: &str = "https://tenant.us.auth0.com/";

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn issuer() -> Url {
        issuer_origin(ISSUER).unwrap()
    }

    /// From the Remote page itself — the window's resting state.
    fn on_remote() -> Option<Url> {
        Some(url("https://remote-staging.hamradiotools.io/"))
    }

    #[test]
    fn the_remote_page_and_the_issuer_open_in_the_window() {
        for target in [
            "https://remote-staging.hamradiotools.io/",
            "https://remote-staging.hamradiotools.io/?code=abc&state=def",
            "https://remote-staging.hamradiotools.io/index.html#x",
            "https://tenant.us.auth0.com/authorize?client_id=x",
            "https://tenant.us.auth0.com/u/login/identifier?state=x",
            "https://tenant.us.auth0.com/v2/logout?returnTo=x",
        ] {
            assert_eq!(
                route(&url(target), on_remote().as_ref(), &issuer()),
                Route::InWindow,
                "{target}"
            );
        }
        // The very first load, before any page: the Remote page itself.
        assert_eq!(
            route(&remote_url(), None, &issuer()),
            Route::InWindow,
            "the window's own first page"
        );
    }

    #[test]
    fn a_link_from_the_remote_page_opens_in_the_system_browser() {
        for target in [
            "https://hamradiotools.io/remote/",
            "https://www.qrz.com/db/KD9TAW",
            "https://accounts.google.com/",
            // The Remote host, but not its origin: another scheme or port is another site.
            "http://remote-staging.hamradiotools.io/",
            "https://remote-staging.hamradiotools.io:8443/",
            // Look-alikes.
            "https://remote-staging.hamradiotools.io.example.net/",
            "https://example.net/remote-staging.hamradiotools.io/",
            "https://remote-staging.hamradiotools.io@example.net/",
            "https://tenant.us.auth0.com.example.net/",
        ] {
            assert_eq!(
                route(&url(target), on_remote().as_ref(), &issuer()),
                Route::SystemBrowser,
                "{target}"
            );
            // Before the first page loads, exactly as strict.
            assert_eq!(
                route(&url(target), None, &issuer()),
                Route::SystemBrowser,
                "{target}"
            );
        }
    }

    /// The hops measured on 2026-10-04 for a Google sign-in: the issuer sends the window to
    /// Google, Google returns through Auth0's shared login host (the tenant's Google connection
    /// uses Auth0's own keys), and that sends it back to the issuer and on to the Remote page.
    #[test]
    fn a_google_sign_in_runs_its_whole_course_in_the_window() {
        let hops = [
            (
                "https://remote-staging.hamradiotools.io/",
                "https://tenant.us.auth0.com/authorize?x",
            ),
            (
                "https://tenant.us.auth0.com/u/login/identifier",
                "https://accounts.google.com/o/oauth2/auth?x",
            ),
            (
                "https://accounts.google.com/v3/signin/identifier",
                "https://accounts.google.com/signin/oauth/consent",
            ),
            (
                "https://accounts.google.com/signin/oauth/consent",
                "https://login.us.auth0.com/login/callback?x",
            ),
            (
                "https://login.us.auth0.com/login/callback",
                "https://tenant.us.auth0.com/login/callback?x",
            ),
            (
                "https://tenant.us.auth0.com/login/callback",
                "https://remote-staging.hamradiotools.io/?code=x",
            ),
        ];
        for (from, to) in hops {
            assert_eq!(
                route(&url(to), Some(&url(from)), &issuer()),
                Route::InWindow,
                "{from} -> {to}"
            );
        }
    }

    #[test]
    fn a_sign_in_step_is_https_and_never_the_apps_own_host() {
        let on_google = url("https://accounts.google.com/v3/signin/identifier");
        for target in [
            "http://accounts.google.com/",
            "http://example.net/",
            "https://localhost/",
            "https://tauri.localhost/",
            "http://tauri.localhost/index.html",
            "https://ipc.localhost/",
        ] {
            assert_eq!(
                route(&url(target), Some(&on_google), &issuer()),
                Route::SystemBrowser,
                "{target}"
            );
        }
    }

    #[test]
    fn nothing_but_a_web_page_goes_anywhere() {
        for target in [
            "javascript:alert(1)",
            "data:text/html,<p>x</p>",
            "file:///C:/Windows/win.ini",
            "about:blank",
            "blob:https://remote-staging.hamradiotools.io/1234",
            "mailto:someone@example.net",
            "ms-settings:privacy",
            "tauri://localhost/",
        ] {
            let target = url(target);
            for from in [on_remote(), None, Some(url("https://accounts.google.com/"))] {
                assert_eq!(
                    route(&target, from.as_ref(), &issuer()),
                    Route::Refuse,
                    "{target}"
                );
            }
            assert_eq!(route_new_window(&target), Route::Refuse, "{target}");
        }
    }

    #[test]
    fn a_new_window_is_never_one_of_ours() {
        for target in [
            "https://remote-staging.hamradiotools.io/",
            "https://tenant.us.auth0.com/",
            "https://www.qrz.com/db/KD9TAW",
            "http://example.net/",
        ] {
            assert_eq!(
                route_new_window(&url(target)),
                Route::SystemBrowser,
                "{target}"
            );
        }
    }

    #[test]
    fn the_issuer_is_taken_only_in_the_shape_the_page_takes_it() {
        assert_eq!(
            issuer_origin(ISSUER).map(|u| u.host_str().map(str::to_owned)),
            Some(Some("tenant.us.auth0.com".into()))
        );
        for bad in [
            "http://tenant.us.auth0.com/",
            "https://user:pass@tenant.us.auth0.com/",
            "https://user@tenant.us.auth0.com/",
            "https://tenant.us.auth0.com/?next=https://example.net/",
            "https://tenant.us.auth0.com/#x",
            "tenant.us.auth0.com",
            "",
            "javascript:alert(1)",
        ] {
            assert_eq!(issuer_origin(bad), None, "{bad:?}");
        }
    }

    // ---- the page reaches no command ---------------------------------------------------------

    /// The context this build compiles in — its configuration, and the access list Tauri checks
    /// every IPC request against (`runtime_authority`).
    fn context() -> tauri::Context<tauri::Wry> {
        tauri::generate_context!()
    }

    /// Every command any permission could grant, named as the IPC names it, from the plugin
    /// manifests the build resolved the access list from (Tauri's own `resolved.rs` naming).
    fn every_command() -> Vec<String> {
        let manifests: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("OUT_DIR"),
            "/acl-manifests.json"
        )))
        .expect("the build's ACL manifests");
        let mut commands = Vec::new();
        for (key, manifest) in manifests.as_object().expect("a manifest map") {
            let prefix = if key == "__app-acl__" {
                String::new()
            } else {
                format!("plugin:{}|", key.strip_prefix("core:").unwrap_or(key))
            };
            for permission in manifest["permissions"].as_object().into_iter().flatten() {
                let allow = permission.1["commands"]["allow"].as_array();
                for command in allow.into_iter().flatten().filter_map(|c| c.as_str()) {
                    commands.push(format!("{prefix}{command}"));
                }
            }
        }
        commands.sort();
        commands.dedup();
        commands
    }

    /// The pages this window can show: the Remote page, an issuer, Google's sign-in and Auth0's
    /// shared login host (see `route`).
    fn remote_pages() -> [Url; 4] {
        [
            url("https://remote-staging.hamradiotools.io/"),
            url("https://tenant.us.auth0.com/u/login"),
            url("https://accounts.google.com/v3/signin/identifier"),
            url("https://login.us.auth0.com/login/callback"),
        ]
    }

    /// ⛔ The Remote page — and every page the window can reach — calls no command: Tauri's own
    /// decision (`RuntimeAuthority::resolve_access`, what `on_message` asks before running
    /// anything), over the access list this build compiles in, for every command a permission
    /// exists for. The app's own commands are never in that list unless the app declares an ACL
    /// manifest (the `__app-acl__` key, walked above if it ever appears); without one, Tauri
    /// refuses them to remote content outright.
    #[test]
    fn no_page_in_the_remote_window_can_call_a_command() {
        let mut context = context();
        let authority = context.runtime_authority_mut();
        let commands = every_command();
        // The control: the list is real, and the same question asked for the app's own window
        // and its own page is answered yes — so a "no" below is an answer, not a broken query.
        assert!(
            commands.len() > 100,
            "only {} commands read",
            commands.len()
        );
        assert!(commands.iter().any(|c| c == "plugin:event|listen"));
        assert!(
            authority
                .resolve_access(
                    "plugin:event|listen",
                    "main",
                    "main",
                    &tauri::ipc::Origin::Local
                )
                .is_some(),
            "the main window's own page must be able to listen, or this test asks nothing"
        );
        let mut reached = Vec::new();
        for page in remote_pages() {
            for label in [LABEL, "main", "panel-connect"] {
                for command in &commands {
                    let origin = tauri::ipc::Origin::Remote { url: page.clone() };
                    if authority
                        .resolve_access(command, label, label, &origin)
                        .is_some()
                    {
                        reached.push(format!("{command} from {page} in {label}"));
                    }
                }
            }
        }
        assert!(
            reached.is_empty(),
            "remote content reaches commands: {reached:#?}"
        );
    }

    // ---- the Stations on this network window ----------------------------------------------

    /// The page the Stations on this network window opens at, as the loopback origin makes it.
    fn lan_page() -> Url {
        url(&format!(
            "http://127.0.0.1:{}/{}/lan.html?lang=de",
            crate::lan_client::PREFERRED_PORT,
            "0a".repeat(32)
        ))
    }

    #[test]
    fn the_lan_window_shows_its_own_page_and_nothing_else() {
        let page = lan_page();
        for here in [
            page.as_str(),
            // The origin's own: what is not the page there is refused by its gates, not here.
            "http://127.0.0.1:42076/",
            "http://127.0.0.1:42076/another/path?x=1#y",
        ] {
            assert_eq!(route_lan(&url(here), &page), Route::InWindow, "{here}");
        }
        for elsewhere in [
            "https://www.qrz.com/db/KD9TAW",
            "https://remote-staging.hamradiotools.io/",
            "http://example.net/",
            "https://192.168.1.20/",
        ] {
            assert_eq!(
                route_lan(&url(elsewhere), &page),
                Route::SystemBrowser,
                "{elsewhere}"
            );
        }
        for own in [
            // Another port, another loopback address, or the app's own hosts: never a browser's.
            "http://127.0.0.1:42075/",
            "https://127.0.0.1:42076/",
            "http://127.0.0.2:42076/",
            "http://localhost:42076/",
            "http://tauri.localhost/index.html",
            "https://ipc.localhost/",
            "http://[::1]:42076/",
            "http://0.0.0.0:42076/",
            // Not a web page at all.
            "javascript:alert(1)",
            "data:text/html,<p>x</p>",
            "file:///C:/Windows/win.ini",
            "blob:http://127.0.0.1:42076/1234",
            "tauri://localhost/",
            "about:blank",
        ] {
            assert_eq!(route_lan(&url(own), &page), Route::Refuse, "{own}");
        }
    }

    /// Tauri's own reading of "is this the app's own page?" (`Webview::is_local_url`, in tauri
    /// 2.11.2's `webview/mod.rs`), which decides whether a page may call the app's own commands at
    /// all: a page that is not the app's own may call none unless a capability names its address,
    /// and none does (`no_capability_names_a_remote_url`). Tauri keeps it private, so this asks
    /// its three questions of this build's own configuration: Tauri's own scheme; an address under
    /// the dev URL, or under `tauri.localhost` where the dist files are served; and a custom
    /// scheme's `<name>.localhost` host (any one of them counts here, registered or not).
    fn local_to_tauri(page: &Url) -> bool {
        let config = context().config().clone();
        let mut own = vec![
            url("tauri://localhost/"),
            url("http://tauri.localhost/"),
            url("https://tauri.localhost/"),
        ];
        own.extend(config.build.dev_url.clone());
        own.iter().any(|base| base.make_relative(page).is_some())
            || page.host_str().is_some_and(|h| h.ends_with(".localhost"))
    }

    /// ⛔ The Stations on this network window's page is not the app's own to Tauri, at the port
    /// the origin asks for first or at any other: so it reaches no app command (the window's own
    /// rule), however many the app adds. CONTROL: the same reading takes the app's own pages, its
    /// dev URL and a custom scheme's host as its own.
    #[test]
    fn the_lan_page_is_not_the_apps_own_to_tauri() {
        for own in [
            "http://tauri.localhost/index.html",
            "https://tauri.localhost/lan.html",
            "tauri://localhost/lan.html",
            "http://asset.localhost/x",
        ] {
            assert!(local_to_tauri(&url(own)), "the control: {own}");
        }
        let dev = context()
            .config()
            .build
            .dev_url
            .clone()
            .expect("the build names a dev URL");
        assert!(
            local_to_tauri(&dev.join("lan.html").unwrap()),
            "the control: {dev}"
        );
        for port in [crate::lan_client::PREFERRED_PORT, 1024, 49152, 65535] {
            let page = url(&format!(
                "http://127.0.0.1:{port}/{}/lan.html",
                "0a".repeat(32)
            ));
            assert!(!local_to_tauri(&page), "{page}");
        }
    }

    /// ⛔ The page in the Stations on this network window calls no command the app's own page can
    /// call: each one any permission grants that the main window's own page may call (Tauri's own
    /// decision over this build's access list, as for the Remote stations window) is refused to
    /// the LAN page, in this window or any other. The app's own commands, the radio's among them,
    /// are not compared: with no app ACL manifest the list holds none of them, so the same
    /// question refuses them to the app's own page too and its "no" proves nothing. Their refusal
    /// to this page rests on Tauri counting it remote (`the_lan_page_is_not_the_apps_own_to_tauri`),
    /// and the tripwire below fails if the app ever declares a manifest. CONTROL, command by
    /// command: the main window's own page may call each one compared, `plugin:event|listen`
    /// among them.
    #[test]
    fn no_page_in_the_lan_window_can_call_a_command() {
        let mut context = context();
        let authority = context.runtime_authority_mut();
        let commands = every_command();
        assert!(
            commands.len() > 100,
            "only {} commands read",
            commands.len()
        );
        let own = tauri::ipc::Origin::Local;
        let callable: Vec<&String> = commands
            .iter()
            .filter(|command| {
                authority
                    .resolve_access(command, "main", "main", &own)
                    .is_some()
            })
            .collect();
        assert!(
            callable
                .iter()
                .any(|command| *command == "plugin:event|listen"),
            "the main window's own page must be able to listen, or this test asks nothing"
        );
        for radio in ["set_ptt", "halt_tx", "set_tune", "stop_voice"] {
            assert!(
                authority
                    .resolve_access(radio, "main", "main", &own)
                    .is_none(),
                "the app now declares an ACL manifest: compare {radio} and its kind here too"
            );
        }
        let origin = tauri::ipc::Origin::Remote { url: lan_page() };
        let reached: Vec<String> = callable
            .iter()
            .filter(|command| {
                [LAN_LABEL, "main", "panel-connect"].iter().any(|label| {
                    authority
                        .resolve_access(command, label, label, &origin)
                        .is_some()
                })
            })
            .map(|command| command.to_string())
            .collect();
        assert!(
            reached.is_empty(),
            "the LAN page reaches commands: {reached:#?}"
        );
    }

    /// ⛔ No capability names a remote URL at all — not the Remote origin, not the issuer, not
    /// any other. Read from what the build itself compiles in: the capability files exactly as
    /// tauri-build parsed them (`OUT_DIR/capabilities.json`) and any capability written inline
    /// in the merged configuration. Nexus has no remote content that should call anything; a
    /// capability with a `remote` block is a security decision, and this test is where it is
    /// taken, not a capability file.
    #[test]
    fn no_capability_names_a_remote_url() {
        let files: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/capabilities.json")))
                .expect("the build's capabilities");
        assert!(
            files.contains_key("default"),
            "the capability files were not read"
        );
        for (id, capability) in &files {
            assert!(
                capability.get("remote").is_none(),
                "capability {id:?} names remote URLs: {}",
                capability["remote"]
            );
        }
        for entry in &context().config().app.security.capabilities {
            if let tauri::utils::config::CapabilityEntry::Inlined(capability) = entry {
                assert!(
                    capability.remote.is_none(),
                    "inline capability {:?} names remote URLs",
                    capability.identifier
                );
            }
        }
    }
}
