//! The Stations on this network window: Remote over this network, on the PC an operator works
//! from — the window the Connect to my station… entry opens (the operator's ruling of 2026-10-04,
//! "Window only in v1"). It is the browser tab, without the browser.
//!
//! **What it is.** One window, [`LAN_LABEL`], for a station paired over the shack's own network
//! or reached through the account: the stream page bundled in this Nexus, served from a loopback
//! origin of this computer's own (`lan_client::Origin`). That origin runs while the window is open
//! and ends with it. What the window adds over a tab:
//! - **Full screen with Esc still a Stop.** F11 puts the WINDOW in the operating system's full
//!   screen, through Tauri. That is not the page's Fullscreen API, so nothing takes Esc to leave
//!   it: Esc stays an ordinary key, and over the picture it is the page's Stop TX. F11 itself is
//!   kept here and never reaches the page or the shack; Esc is never touched.
//! - **No browser shortcuts in the way.** WebView2's own accelerators (F5 and Ctrl+R reload, Ctrl+P
//!   print, Ctrl+F find, Alt+Left back…) are off, as in every Nexus window (`browser_keys`), so
//!   those keys reach the page and, through the stream, the shack.
//!
//! **What it is not: a bridge.** The page reaches no Nexus command. Tauri counts the loopback
//! origin as a remote page, and Tauri 2.11 refuses every command — the app's own and every
//! plugin's — to a page that is not the app's own unless a capability names a remote URL
//! (`webview/mod.rs`, `on_message`), and none does; the tests below hold that against the access
//! list this very build compiles in, and fail if any capability ever names one. Tauri still
//! injects its IPC script, and its plugins theirs, into every page, so the page can ASK; every ask
//! is refused. One Tauri request is exempt from that check until Tauri 3
//! (`plugin:__TAURI_CHANNEL__|fetch`, which hands over a large `tauri::ipc::Channel` message by a
//! counter id from one app-wide queue). Nexus sends nothing through a Channel, and its own windows
//! call over the IPC protocol, so that queue stays empty; a first `Channel` in this app reopens
//! the question.
//!
//! **Where it may go** ([`route_lan`]). The window shows its own page and nothing else: any other
//! web page goes to the system browser, and this computer's own loopback and app hosts go nowhere.
//!
//! **Windows only, for now.** It is WebView2, the same Chromium WebRTC and WebCodecs the page is
//! tested on in Chrome and Edge. On Linux, WebKitGTK ships without WebRTC in most distributions'
//! builds, so the stream cannot start; on macOS, WKWebView has WebRTC but no `AudioDecoder` before
//! Safari 26, so the station's audio could not play. Neither is offered: the command refuses, and
//! Settings says to use a browser there.

use std::sync::{Arc, Mutex};

use tauri::webview::NewWindowResponse;
use tauri::{Manager, Url, WebviewUrl, WebviewWindowBuilder};

use crate::lan_client::{computer_name, typed_path, Origin, Reach, Stations};
use crate::remote_service::vault::SystemVault;
use crate::remote_service::REMOTE_ORIGIN;

/// The window's label. In no capability's `windows` list, so it gets nothing even as a local page.
pub const LAN_LABEL: &str = "lan-stations";

/// The languages the Stations on this network window's page can be opened in: the catalogs it
/// ships with.
const LOCALES: [&str; 5] = ["en", "de", "es", "fr", "ja"];

/// Opening size, in logical px: room for the shack's window and the page's own controls.
const DEFAULT_INNER: (f64, f64) = (1280.0, 800.0);

/// The smallest the window can be dragged to. The page lays itself out down to a phone's width, so
/// this only keeps the window a window.
const MIN_INNER: (f64, f64) = (640.0, 480.0);

/// The window itself could not be made.
const NOT_OPENED: &str = "remoteWindowFailed";

/// The answer on Linux and macOS, where the window is not offered (see the module header).
const UNAVAILABLE: &str = "remoteWindowUnavailable";

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

/// Hand a web address to the operator's default browser. Off this thread: it is called from
/// inside WebView2's own navigation event, which must not wait on the shell.
fn open_in_browser(app: &tauri::AppHandle, url: &Url) {
    use tauri_plugin_opener::OpenerExt;
    let (app, url) = (app.clone(), url.to_string());
    std::thread::spawn(move || {
        let _ = app.opener().open_url(url, None::<&str>);
    });
}

/// The loopback origin the Stations on this network window's page comes from, while the window is
/// open.
#[derive(Default)]
pub struct LanOrigin(Mutex<Option<Origin>>);

/// What the window's page can reach: the app's own embedded files (the same the main window
/// loads, so nothing is read from disk and nothing can differ from the shipped page), this
/// computer's paired stations in the OS credential store, and the stations Windows' own DNS-SD
/// finds by name; and signing in with the station's account, through the system browser.
fn reach(app: &tauri::AppHandle) -> Reach {
    let files = app.clone();
    let stations = Arc::new(
        Stations::new(Arc::new(SystemVault))
            .typed_in(typed_path())
            .signed_in(crate::lan_client::signed_path()),
    );
    Reach {
        assets: Arc::new(move |path: &str| {
            let resolver = files.asset_resolver();
            let asset = resolver
                .get(format!("/{path}"))
                .or_else(|| resolver.get(path.to_string()))?;
            Some((asset.bytes, asset.mime_type))
        }),
        stations: Arc::clone(&stations),
        find: Arc::new(tempo_stream::lan::dnssd::find),
        lookup: Arc::new(tempo_stream::lan::lookup),
        name: computer_name(),
        operate: {
            let app = app.clone();
            Arc::new(move |station: String, typed| {
                crate::client_mode::start_again(
                    &app,
                    crate::client_mode::operate_args(station, typed),
                )
            })
        },
        sign_in: {
            let app = app.clone();
            Arc::new(move |station: Option<String>, choose, ask| {
                let (app, stations) = (app.clone(), Arc::clone(&stations));
                Box::pin(async move {
                    use tauri_plugin_opener::OpenerExt;
                    let env = crate::lan_client::account::Env::new(
                        stations,
                        Arc::new(tempo_stream::lan::dnssd::find),
                        Arc::new(tempo_stream::lan::lookup),
                        computer_name(),
                    )?;
                    let open = |url: &str| {
                        app.opener()
                            .open_url(url, None::<&str>)
                            .map_err(|e| e.to_string())
                    };
                    crate::lan_client::account::sign_in(&env, station, open, choose, ask).await
                })
            })
        },
        forget_internet: Arc::new(|road| {
            Box::pin(async move {
                crate::lan_client::internet::Service::new(REMOTE_ORIGIN)?
                    .forget(&road)
                    .await
            })
        }),
        now_s: || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        },
    }
}

/// Open the Stations on this network window, or bring it forward when it is already open, with
/// its page in `locale` (one of [`LOCALES`]; otherwise the page's own choice), and offering Pair
/// again for the station `pair_again` names (a station id; anything else is left out): the one
/// this computer last drove, which refused it in a way only a new pairing ends
/// (`client_mode::pair_again`). The page fills in that station's address from its own list.
///
/// Called by Settings ▸ Station ▸ Remote access on this PC's own window, and by the launch after
/// such a refusal; the window's page cannot call it (or anything else).
#[tauri::command]
pub async fn open_lan_stations_window(
    app: tauri::AppHandle,
    origin: tauri::State<'_, LanOrigin>,
    locale: Option<String>,
    pair_again: Option<String>,
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
    if let Some(station) = pair_again.filter(|id| tempo_stream::protocol::identifier(id)) {
        page.query_pairs_mut().append_pair("pairAgain", &station);
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
    f11_full_screen(&window);
    Ok(())
}

/// F11 for the window's own full screen. (WebView2's browser accelerators are off in every Nexus
/// window: `browser_keys`.)
///
/// The `AcceleratorKeyPressed` handler sees F11 before the page does: it keeps F11, press and
/// release, and toggles the window's full screen on the press (not on auto-repeat). The toggle runs
/// on a thread of its own because this handler is inside WebView2's event, where resizing the
/// window under it is not safe. No other key is touched; Esc in particular always reaches the page.
#[cfg(windows)]
fn f11_full_screen(window: &tauri::WebviewWindow) {
    use webview2_com::AcceleratorKeyPressedEventHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
        COREWEBVIEW2_KEY_EVENT_KIND_KEY_UP, COREWEBVIEW2_PHYSICAL_KEY_STATUS,
    };

    /// `VK_F11` (winuser.h).
    const VK_F11: u32 = 0x7A;

    let target = window.clone();
    let _ = window.with_webview(move |webview| {
        let controller = webview.controller();
        // SAFETY: plain COM calls on the live controller of this window's own webview, on the
        // thread WebView2 created it on (`with_webview` runs there).
        unsafe {
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

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
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

    /// Hosted pages: the Remote page, a sign-in issuer, Google's sign-in and Auth0's shared login
    /// host.
    fn remote_pages() -> [Url; 4] {
        [
            url("https://remote-staging.hamradiotools.io/"),
            url("https://tenant.us.auth0.com/u/login"),
            url("https://accounts.google.com/v3/signin/identifier"),
            url("https://login.us.auth0.com/login/callback"),
        ]
    }

    /// ⛔ A hosted page — the Remote page and the sign-in hosts — calls no command in any window of
    /// this app, the Stations on this network window included: Tauri's own
    /// decision (`RuntimeAuthority::resolve_access`, what `on_message` asks before running
    /// anything), over the access list this build compiles in, for every command a permission
    /// exists for. The app's own commands are never in that list unless the app declares an ACL
    /// manifest (the `__app-acl__` key, walked above if it ever appears); without one, Tauri
    /// refuses them to remote content outright.
    #[test]
    fn no_hosted_page_can_call_a_command() {
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
            for label in [LAN_LABEL, "main", "panel-connect"] {
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
    /// decision over this build's access list, as for a hosted page) is refused to
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
