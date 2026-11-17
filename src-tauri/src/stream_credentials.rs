//! ⛔ While a stream watches the station's screen, no stored key or password is saved, forgotten,
//! let go or asked about from any window of this app.
//!
//! The stream reaches the station only as input into the station's own page, so every command a
//! streamed press causes arrives here over the same IPC as a press at the shack, and nothing on the
//! call says which it was. The page marks a streamed press while its event is handled and refuses
//! the credential commands it starts, but that mark ends with the event: a command sent after an
//! await carries none. [`Watched`] does not end that way. It is live from a stream's session on
//! until the session is gone, so while it is live every credential command is refused here,
//! whoever pressed it. The operator at the shack saves keys again once the stream ends.
//!
//! WHICH COMMANDS: every row of the class table (`tempo_client::classes`) that is set at the shack
//! only ([`Class::RefusedHere`]) for a credential's sake, by its reason, read at run time, so a new
//! credential command is refused the day its row is written. One row is let through:
//! [`STREAM_MAY_ASK`]. And one read that is no credential is refused too, because it can show
//! one: [`STREAM_MAY_NOT_READ`].
//!
//! The refusal is `nexusClient:atShackOnly` ([`AT_SHACK_ONLY`]), which the page words as saved keys
//! being locked while the screen is streamed.

use tauri::ipc::Invoke;
use tauri::{Manager, Runtime};

use crate::command_classes::Class;
use crate::remote_service::stream::Watched;

/// The refusal's code: the page words it ([`crate::client_mode::word`]).
pub(crate) const AT_SHACK_ONLY: &str = "atShackOnly";

/// The credential row a watched screen may still ask: which saved keys wait, by radio name and
/// address, never a key. The stream already shows that notice in its picture, and refusing the ask
/// would stop the notice at the shack for as long as the app is open.
pub(crate) const STREAM_MAY_ASK: [&str; 1] = ["keys_to_confirm"];

/// What a watched screen may not read though it is no credential command: the Connections log,
/// whose lines carry the services' own words, and those have carried keys. The streamed picture
/// would show them to whoever watches.
pub(crate) const STREAM_MAY_NOT_READ: [&str; 1] = ["get_connection_log"];

/// Whether `cmd` is refused while a stream watches: [`STREAM_MAY_NOT_READ`], and a class-table row
/// set at the shack only whose reason is a credential, but not [`STREAM_MAY_ASK`].
pub(crate) fn refused_while_watched(cmd: &str) -> bool {
    STREAM_MAY_NOT_READ.contains(&cmd)
        || !STREAM_MAY_ASK.contains(&cmd)
            && tempo_client::classes::CLASSES
                .iter()
                .any(|(row, class, why)| {
                    *row == cmd
                        && *class == Class::RefusedHere
                        && why.to_ascii_lowercase().contains("credential")
                })
}

/// The app's handler `commands`, with every [`refused_while_watched`] command refused while the
/// app's [`Watched`] is live. An app that manages no `Watched` (client mode's) is never watched.
pub(crate) fn guarded<R: Runtime>(
    commands: Box<dyn Fn(Invoke<R>) -> bool + Send + Sync>,
) -> Box<dyn Fn(Invoke<R>) -> bool + Send + Sync> {
    Box::new(move |invoke: Invoke<R>| {
        let watched = invoke
            .message
            .webview_ref()
            .try_state::<Watched>()
            .is_some_and(|w| w.live());
        if watched && refused_while_watched(invoke.message.command()) {
            invoke
                .resolver
                .reject(crate::client_mode::word(AT_SHACK_ONLY));
            return true;
        }
        commands(invoke)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
    use tauri::webview::InvokeRequest;
    use tauri::WebviewWindow;

    /// The class rows set at the shack only for a credential's sake, read as the page's test reads
    /// them (`ui/src/api.credentialsAtShack.test.ts`).
    fn credential_rows() -> Vec<&'static str> {
        tempo_client::classes::CLASSES
            .iter()
            .filter(|(_, class, why)| {
                *class == Class::RefusedHere && why.to_ascii_lowercase().contains("credential")
            })
            .map(|(row, ..)| *row)
            .collect()
    }

    /// A mock app whose every command answers `"ran"`, behind [`guarded`], with `watched`.
    fn app(
        watched: &Watched,
    ) -> (
        tauri::App<tauri::test::MockRuntime>,
        WebviewWindow<tauri::test::MockRuntime>,
    ) {
        let app = mock_builder()
            .manage(watched.clone())
            .invoke_handler(guarded(Box::new(
                |invoke: Invoke<tauri::test::MockRuntime>| {
                    invoke.resolver.resolve("ran");
                    true
                },
            )))
            .build(mock_context(noop_assets()))
            .expect("a mock app");
        let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("the main window");
        (app, main)
    }

    fn call(window: &WebviewWindow<tauri::test::MockRuntime>, cmd: &str) -> Result<Value, Value> {
        get_ipc_response(
            window,
            InvokeRequest {
                cmd: cmd.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(json!({})),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .map(|body| body.deserialize::<Value>().unwrap())
    }

    #[test]
    fn the_table_has_the_credential_rows_this_reads() {
        let rows = credential_rows();
        for cmd in [
            "answer_saved_key",
            "upload_backlog",
            "set_cloudlog_key",
            "clear_icom_lan_password",
            "set_repeaterbook_token",
            "keys_to_confirm",
        ] {
            assert!(rows.contains(&cmd), "{cmd} is a credential row: {rows:?}");
        }
        for cmd in STREAM_MAY_ASK {
            assert!(rows.contains(&cmd), "{cmd} is a credential row");
        }
        for cmd in STREAM_MAY_NOT_READ {
            assert!(
                !rows.contains(&cmd),
                "{cmd} is no credential row: it is refused by name"
            );
            assert_eq!(
                crate::command_classes::class_of(cmd),
                Some(Class::RefusedHere),
                "{cmd} is a row the table keeps at the shack"
            );
        }
    }

    /// ⛔ While a stream watches, every credential row is refused, in the page's words, and never
    /// reaches the command. CONTROL: with no stream, and once the stream is gone, each runs.
    #[test]
    fn every_credential_command_is_refused_while_a_stream_watches() {
        let watched = Watched::default();
        let (_app, main) = app(&watched);
        let refused = json!(format!("nexusClient:{AT_SHACK_ONLY}"));
        let rows = credential_rows();
        for cmd in &rows {
            assert_eq!(
                call(&main, cmd),
                Ok(json!("ran")),
                "CONTROL, no stream: {cmd}"
            );
        }
        let stream = watched.watching();
        for cmd in &rows {
            let answer = call(&main, cmd);
            if STREAM_MAY_ASK.contains(cmd) {
                assert_eq!(answer, Ok(json!("ran")), "{cmd} is let through");
            } else {
                assert_eq!(answer, Err(refused.clone()), "{cmd} while a stream watches");
            }
        }
        // What is no credential runs as before.
        for cmd in [
            "set_settings",
            "update_radio_profile",
            "get_settings",
            "clublog_push_qso",
        ] {
            assert_eq!(call(&main, cmd), Ok(json!("ran")), "{cmd} is no credential");
        }
        drop(stream);
        for cmd in &rows {
            assert_eq!(
                call(&main, cmd),
                Ok(json!("ran")),
                "CONTROL, stream gone: {cmd}"
            );
        }
    }

    /// ⛔ While a stream watches, the Connections log is not read: its lines have carried keys,
    /// and the streamed picture would show them. CONTROL: it is read before and after.
    #[test]
    fn the_connections_log_is_not_read_while_a_stream_watches() {
        let watched = Watched::default();
        let (_app, main) = app(&watched);
        assert_eq!(
            call(&main, "get_connection_log"),
            Ok(json!("ran")),
            "CONTROL: no stream"
        );
        let stream = watched.watching();
        assert_eq!(
            call(&main, "get_connection_log"),
            Err(json!(format!("nexusClient:{AT_SHACK_ONLY}")))
        );
        drop(stream);
        assert_eq!(
            call(&main, "get_connection_log"),
            Ok(json!("ran")),
            "CONTROL: stream gone"
        );
    }

    /// The app's own handler goes through [`guarded`], once, around the whole list, so no
    /// registered command can be left out of it.
    #[test]
    fn the_apps_handler_is_guarded() {
        let src = include_str!("lib.rs");
        let body = &src[src
            .find("\npub(crate) fn app_commands()")
            .expect("app_commands")..];
        let body = &body[..body.find("\n}\n").expect("its end")];
        assert!(
            body.contains("stream_credentials::guarded(commands)"),
            "app_commands returns its commands through stream_credentials::guarded"
        );
    }
}
