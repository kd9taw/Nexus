//! WebView2's browser keys off in every Nexus window.
//!
//! **Why.** On Windows, Nexus runs in WebView2, which acts on its own browser shortcuts: F5, Ctrl+F5
//! and Ctrl+R reload the window, Ctrl+P prints, Ctrl+F finds, Alt+Left goes back, and in a debug
//! build F12 opens the developer tools. In the main window one of those keys, pressed by mistake in
//! a contest, would reload the whole app in the middle of a contact. A page can hold a key it
//! handles itself (the voice keyer holds F1–F12 while it is on screen), but not every key in every
//! window.
//!
//! **What.** WebView2's own switch for exactly those shortcuts,
//! `ICoreWebView2Settings3::AreBrowserAcceleratorKeysEnabled`, is set to false in every webview the
//! app makes. The shortcuts then reach the page
//! as ordinary keys instead of acting on the window: every F-key reaches the page's own handlers
//! (CW's and Phone's among them), and a key no page handles does nothing. The editing keys are not
//! browser shortcuts and are not affected (Ctrl+C, Ctrl+V, Ctrl+X, Ctrl+A, Ctrl+Z).
//!
//! **Where.** One plugin, [`init`], registered in the app's builder. Tauri hands its
//! `on_webview_ready` hook every webview it makes: the windows `tauri.conf.json` declares (the main
//! window and the splash), every pop-out, the Stations on this network window,
//! and any window added later, however it is built (tauri 2.11.2: `attach_webview` in
//! `manager/webview.rs` runs every plugin's hook, and `Builder::build` initializes the plugins
//! before `App::run` makes the configured windows). So no window has to remember to ask for it, and
//! the census below fails if a window is ever made where the hook cannot reach it.
//!
//! **Why not a setting.** Tauri 2.11 has no configuration field or builder method for it, and does
//! not pass wry's own switch (`with_browser_accelerator_keys`) through, so neither a window in
//! `tauri.conf.json` nor a built one can ask for it at creation. The hook sets it on the webview's
//! own settings once WebView2 has made it, through the controller Tauri hands out.
//!
//! **Windows only.** The setting is WebView2's. Linux's and macOS's webviews have no such switch,
//! and there the hook does nothing.

use tauri::plugin::TauriPlugin;
use tauri::{Webview, Wry};

/// The plugin that turns WebView2's browser keys off in each webview as Tauri makes it.
pub fn init() -> TauriPlugin<Wry> {
    tauri::plugin::Builder::new("browser-keys")
        .on_webview_ready(|webview| browser_keys_off(&webview))
        .build()
}

/// `AreBrowserAcceleratorKeysEnabled` false in this webview's own WebView2 settings. The hook runs
/// on the main thread, inside Tauri's own handling of the new webview, where `with_webview` calls
/// the closure at once; the closure touches nothing but this webview's settings, so it takes no
/// lock Tauri holds there.
#[cfg(windows)]
fn browser_keys_off(webview: &Webview<Wry>) {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
    use windows_core::Interface;

    let _ = webview.with_webview(|platform| {
        // SAFETY: plain COM calls on the live controller of this webview, on the thread WebView2
        // created it on (`with_webview` runs there).
        unsafe {
            if let Ok(settings) = platform
                .controller()
                .CoreWebView2()
                .and_then(|core| core.Settings())
                .and_then(|settings| settings.cast::<ICoreWebView2Settings3>())
            {
                let _ = settings.SetAreBrowserAcceleratorKeysEnabled(false);
            }
        }
    });
}

/// Linux's and macOS's webviews have no such switch.
#[cfg(not(windows))]
fn browser_keys_off(_: &Webview<Wry>) {}

#[cfg(test)]
mod tests {
    use std::ops::Range;
    use std::path::{Path, PathBuf};

    /// One Rust file of this crate: where it is under `src/`, and its text.
    struct Source {
        path: String,
        text: String,
    }

    /// Every Rust file under this crate's `src/`, read as the test runs. The census reads the
    /// crate, never a file it names, so a window made in a file that does not exist yet is in it.
    fn sources() -> Vec<Source> {
        fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("a source folder") {
                let path = entry.expect("a folder entry").path();
                if path.is_dir() {
                    walk(&path, found);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    found.push(path);
                }
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut found = Vec::new();
        walk(&root, &mut found);
        found.sort();
        found
            .into_iter()
            .map(|path| Source {
                text: std::fs::read_to_string(&path)
                    .expect("a source file")
                    .replace("\r\n", "\n"),
                path: path
                    .strip_prefix(&root)
                    .expect("a file under src")
                    .to_string_lossy()
                    .replace('\\', "/"),
            })
            .collect()
    }

    /// How the rest of the crate names an item of the file at `path` (`remote_window::`,
    /// `remote_service::lan::`); nothing for the crate's root.
    fn module_prefix(path: &str) -> String {
        let module = path.trim_end_matches(".rs").trim_end_matches("/mod");
        if module == "lib" || module == "main" {
            String::new()
        } else {
            format!("{}::", module.replace('/', "::"))
        }
    }

    /// A function at the top level of a file: its name, and the span of its text.
    struct Function {
        name: String,
        span: Range<usize>,
    }

    /// The top-level functions of `text`, as rustfmt lays them out: from a line that starts with
    /// `fn` (after `pub`, `async` and the like) to the first line after it that is only `}`.
    fn functions(text: &str) -> Vec<Function> {
        let mut found = Vec::new();
        let mut at = 0;
        for line in text.split_inclusive('\n') {
            let mut head = line;
            while let Some(rest) = [
                "pub(crate) ",
                "pub(super) ",
                "pub ",
                "const ",
                "async ",
                "unsafe ",
            ]
            .iter()
            .find_map(|word| head.strip_prefix(word))
            {
                head = rest;
            }
            if let Some(signature) = head.strip_prefix("fn ") {
                let name = signature
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                let end = if line.trim_end().ends_with('}') {
                    at + line.len()
                } else {
                    text[at..]
                        .find("\n}\n")
                        .map_or(text.len(), |end| at + end + 3)
                };
                found.push(Function {
                    name,
                    span: at..end,
                });
            }
            at += line.len();
        }
        found
    }

    /// Where `needle` is written in `text` as code: every place but those on a comment line.
    fn code_at(text: &str, needle: &str) -> Vec<usize> {
        text.match_indices(needle)
            .map(|(at, _)| at)
            .filter(|&at| {
                let line = text[..at].rfind('\n').map_or(0, |n| n + 1);
                !text[line..at].trim_start().starts_with("//")
            })
            .collect()
    }

    /// The top-level function `at` is in, if any.
    fn inside(functions: &[Function], at: usize) -> Option<&Function> {
        functions.iter().find(|f| f.span.contains(&at))
    }

    /// Whether `body` calls the function `name` (as code, and by that name, not a longer one).
    fn calls(body: &str, name: &str) -> bool {
        code_at(body, &format!("{name}("))
            .into_iter()
            .any(|at| !body[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_'))
    }

    /// What the crate's source does about WebView2's browser keys.
    struct Switch {
        /// The functions that turn them off themselves.
        setters: Vec<String>,
        /// The call, in the app's builder, that registers a hook Tauri runs for every webview it
        /// makes, a hook that hands the webview to a setter. `None` when the builder has none.
        every_webview: Option<String>,
    }

    fn switch(sources: &[Source]) -> Switch {
        // Each needle is put together here, so this file's own text never matches it.
        let setting = ["SetAreBrowserAcceleratorKeysEnabled", "(false)"].concat();
        let ready = [".on_webview", "_ready("].concat();
        let start = ["tauri::Builder", "::default()"].concat();
        let mut setters = Vec::new();
        let mut apps = Vec::new();
        for source in sources {
            let functions = functions(&source.text);
            for at in code_at(&source.text, &setting) {
                setters.extend(inside(&functions, at).map(|f| f.name.clone()));
            }
            for at in code_at(&source.text, &start) {
                apps.extend(
                    inside(&functions, at).map(|f| source.text[f.span.clone()].to_string()),
                );
            }
        }
        apps.dedup();
        // Two apps are built: the station's own (`build_app`) and client mode's (`client_mode::run`).
        // The switch counts only when it is registered in every one of them.
        assert_eq!(apps.len(), 2, "the app is built in exactly two functions");
        let mut hooks = Vec::new();
        for source in sources {
            let functions = functions(&source.text);
            for at in code_at(&source.text, &ready) {
                if let Some(f) = inside(&functions, at) {
                    let body = &source.text[f.span.clone()];
                    if setters.iter().any(|setter| calls(body, setter)) {
                        hooks.push(format!("{}{}(", module_prefix(&source.path), f.name));
                    }
                }
            }
        }
        let every_webview = hooks
            .into_iter()
            .find(|hook| apps.iter().all(|app| app.contains(hook.as_str())));
        Switch {
            setters,
            every_webview,
        }
    }

    /// One window the census found, and whether WebView2's browser keys are off in it.
    #[derive(Debug)]
    struct Window {
        what: String,
        keys_off: bool,
    }

    /// The context this build compiles in: its configuration, and its access list.
    fn context() -> tauri::Context<tauri::Wry> {
        tauri::generate_context!()
    }

    /// Every way a window comes into being in this app, each judged by what runs when it is made:
    /// - the windows the configuration declares, which Tauri makes as the app starts: off when the
    ///   app's builder hands every webview to the switch;
    /// - every webview this crate builds (`WebviewWindow` and `Webview` builders), in whatever file:
    ///   off when the app's builder hands every webview to the switch, or when the function that
    ///   builds it calls a setter itself;
    /// - a page's own new window, which WebView2 makes itself when a handler answers
    ///   `NewWindowResponse::Allow`: never off, because Tauri never sees it, so no hook reaches it;
    /// - a window a page makes through Tauri's own commands, wherever a capability grants one:
    ///   off when the app's builder hands every webview to the switch.
    fn census(sources: &[Source]) -> Vec<Window> {
        let switch = switch(sources);
        let every = switch.every_webview.is_some();
        let mut windows: Vec<Window> = context()
            .config()
            .app
            .windows
            .iter()
            .filter(|w| w.create)
            .map(|w| Window {
                what: format!("{} (tauri.conf.json)", w.label),
                keys_off: every,
            })
            .collect();
        let allow = ["NewWindowResponse", "::Allow"].concat();
        for source in sources {
            let functions = functions(&source.text);
            let line = |at: usize| source.text[..at].matches('\n').count() + 1;
            for at in code_at(&source.text, "Builder::") {
                let before = &source.text[..at];
                let kind = &before[before
                    .char_indices()
                    .rev()
                    .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
                    .last()
                    .map_or(at, |(i, _)| i)..];
                let made_by = &source.text[at + "Builder::".len()..];
                if !matches!(kind, "WebviewWindow" | "Webview")
                    || !(made_by.starts_with("new(") || made_by.starts_with("from_config("))
                {
                    continue;
                }
                let maker = inside(&functions, at);
                let own = maker.is_some_and(|f| {
                    let body = &source.text[f.span.clone()];
                    switch.setters.iter().any(|setter| calls(body, setter))
                });
                windows.push(Window {
                    what: format!(
                        "{} ({}:{})",
                        maker.map_or("?", |f| f.name.as_str()),
                        source.path,
                        line(at)
                    ),
                    keys_off: every || own,
                });
            }
            for at in code_at(&source.text, &allow) {
                windows.push(Window {
                    what: format!("a page's own new window ({}:{})", source.path, line(at)),
                    keys_off: false,
                });
            }
        }
        windows.extend(made_by_a_page().into_iter().map(|what| Window {
            what,
            keys_off: every,
        }));
        windows
    }

    /// The windows a page could make through Tauri's own commands: one for each window a
    /// capability grants a window-making command. Asked of Tauri's own access decision
    /// (`resolve_access`) over the access list this build compiles in. None today: `core:default`
    /// grants no window-making command.
    fn made_by_a_page() -> Vec<String> {
        let manifests: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("OUT_DIR"),
            "/acl-manifests.json"
        )))
        .expect("the build's ACL manifests");
        let capabilities: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/capabilities.json")))
                .expect("the build's capabilities");
        let making = [
            ("webview", "create_webview_window"),
            ("webview", "create_webview"),
            ("window", "create"),
        ];
        for (plugin, command) in making {
            let known = manifests[format!("core:{plugin}")]["permissions"]
                .as_object()
                .into_iter()
                .flatten()
                .any(|(_, permission)| {
                    permission["commands"]["allow"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|c| c == command)
                });
            assert!(
                known,
                "the build knows no `{plugin}|{command}`, so asking about it would ask nothing"
            );
        }
        let mut context = context();
        let authority = context.runtime_authority_mut();
        let own = tauri::ipc::Origin::Local;
        assert!(
            authority
                .resolve_access("plugin:event|listen", "main", "main", &own)
                .is_some(),
            "the main window's own page must be able to listen, or this asks nothing"
        );
        let mut made = Vec::new();
        for capability in capabilities.values() {
            let patterns = capability["windows"].as_array().into_iter().flatten();
            for label in patterns
                .filter_map(|w| w.as_str())
                .map(|w| w.replace('*', "census"))
            {
                for (plugin, command) in making {
                    let name = format!("plugin:{plugin}|{command}");
                    if authority
                        .resolve_access(&name, &label, &label, &own)
                        .is_some()
                    {
                        made.push(format!("a window a page in {label} makes ({name})"));
                    }
                }
            }
        }
        made
    }

    /// ⛔ WEBVIEW2'S BROWSER KEYS ARE OFF IN EVERY WINDOW NEXUS MAKES. A census of every way a
    /// window comes into being here ([`census`]), read from the crate's source, the configuration
    /// and the access list this build compiles in. CONTROLS: the census finds the main window,
    /// from the configuration, and the Stations on this network window, from a builder outside
    /// `lib.rs`; and it sees the keys off in the Stations on this network window.
    ///
    /// What it reads is what the code is written to do. Whether WebView2 then acts on the setting
    /// (F5 does not reload, the F-keys and the editing keys still work) only a Windows run shows.
    #[test]
    fn every_window_nexus_makes_has_the_browser_keys_off() {
        let windows = census(&sources());
        let found = |what: &str| windows.iter().find(|w| w.what.starts_with(what));
        assert!(
            found("main (").is_some() && found("open_lan_stations_window (").is_some(),
            "the census finds the main window and the Stations on this network window: {windows:#?}"
        );
        assert!(
            found("open_lan_stations_window (").is_some_and(|w| w.keys_off),
            "the census sees the keys off in the Stations on this network window: {windows:#?}"
        );
        let (off, on): (Vec<&Window>, Vec<&Window>) = windows.iter().partition(|w| w.keys_off);
        assert!(
            on.is_empty(),
            "WebView2's browser keys are on in {on:#?}\n(off in {off:#?})"
        );
    }

    /// ⛔ THE CENSUS CAN STILL SAY NO. The same census, with the switch's registration taken out of
    /// the app's builder, finds the keys on in the main window and in the pop-outs: so the census
    /// above is green because the switch is registered, not because it cannot see a window
    /// without it.
    #[test]
    fn without_the_switch_in_the_apps_builder_the_census_finds_the_keys_on() {
        let mut sources = sources();
        let call = switch(&sources)
            .every_webview
            .expect("the app's builder registers the switch");
        for source in &mut sources {
            source.text = source.text.replace(&call, "unregistered(");
        }
        let windows = census(&sources);
        for window in ["main (", "open_panel_window ("] {
            assert!(
                windows
                    .iter()
                    .any(|w| w.what.starts_with(window) && !w.keys_off),
                "the census finds the keys on in {window}…): {windows:#?}"
            );
        }
    }

    /// ⛔ AND IN CLIENT MODE'S APP. The same census, with the switch taken out of client mode's
    /// builder alone, finds the keys on in the main window: a computer driving a station makes its
    /// windows through that builder, so the switch must be registered there too.
    #[test]
    fn without_the_switch_in_client_modes_builder_the_census_finds_the_keys_on() {
        let mut sources = sources();
        let call = switch(&sources)
            .every_webview
            .expect("every app's builder registers the switch");
        let client = sources
            .iter_mut()
            .find(|s| s.path.ends_with("client_mode.rs"))
            .expect("the census reads client_mode.rs");
        assert!(
            client.text.contains(&call),
            "client mode's builder registers the switch"
        );
        client.text = client.text.replace(&call, "unregistered(");
        let windows = census(&sources);
        assert!(
            windows
                .iter()
                .any(|w| w.what.starts_with("main (") && !w.keys_off),
            "the census finds the keys on in main (…): {windows:#?}"
        );
    }
}
