//! Settings ▸ Connections ▸ Test for LoTW, eQSL and ClubLog: each signs in to the service and
//! READS, the way the QRZ Logbook Test sends ACTION=STATUS. A Test never uploads and never
//! changes anything on the service; the request each sends is built in `tempo_core`
//! (`lotw`/`eqsl`/`clublog::build_login_test_url`) and is a read by that service's own
//! documentation.
//!
//! The credentials are read with the same calls, in the same order and with the same refusals,
//! as the path that already spends them: LoTW's and eQSL's confirmation downloads, ClubLog's
//! realtime upload. Nothing new is stored. A secret travels only in the request to its own
//! service; what comes back is a Nexus sentence or the service's own words, with any secret the
//! service echoes masked ([`crate::service_words_without_secrets`]).
//!
//! What each Test does NOT prove: LoTW's checks the website login the confirmation download uses,
//! not the TQSL certificate an upload is signed with; none can show a contact would be accepted.

use tauri::State;
use tempo_core::login_check::{capped, LoginCheck};

use crate::{
    alnum_folded, clublog_keychain, conn_logged, effective_clublog_key, engine_lock, eqsl_keychain,
    folded_secret_forms, lotw_keychain, service_words_without_secrets, Echoed, SharedEngine,
    CLUBLOG_SUSPENDED,
};

/// A Test's fetch: the read-only URL, and the service's answer or a redacted transport error.
type Fetch<'a, T> = &'a dyn Fn(&str) -> Result<T, String>;

/// What a Test answers the operator when the service's answer was neither a yes nor a no.
fn cannot_tell(service: &str, words: Option<String>) -> String {
    let base = format!(
        "{service} answered, but not in a way Nexus recognises, so it cannot tell whether the \
         login works. Try again shortly."
    );
    match words {
        Some(words) => format!("{base} {service} said: {words}"),
        None => base,
    }
}

/// The shortest run of a secret's letters and digits that, shown anywhere in a service's words,
/// withholds them. Five or fewer in a row can be a coincidence.
const FRAGMENT: usize = 6;

/// A service's whole answer `words` as a Test shows them: the secrets masked first
/// ([`service_words_without_secrets`]), and only then cut to length — a cut made first can leave
/// the start of a secret that masking, looking for the whole secret, no longer finds. Words whose
/// cut still ends in the first four or more letters of a secret (an echo the service itself cut
/// short) are replaced whole by `withheld`, and so are words that show any run of
/// [`FRAGMENT`] or more letters and digits of a secret anywhere, in any case or percent-encoded
/// (a start, middle or end the service echoed on its own). A secret shorter than [`FRAGMENT`]
/// has no such run; it relies on the whole-secret mask and the guard at the cut.
fn shown_words(words: &str, echoed: &[Echoed<'_>], withheld: &str) -> String {
    let shown = capped(&service_words_without_secrets(words, echoed, withheld));
    let folded = alnum_folded(shown.trim_end_matches('…'));
    let ends_in_a_secret = echoed.iter().any(|e| {
        folded_secret_forms(e.secret)
            .iter()
            .any(|k| (4..=k.len()).any(|n| folded.ends_with(&k[..n])))
    });
    let shows_a_fragment = echoed.iter().any(|e| {
        folded_secret_forms(e.secret).iter().any(|k| {
            k.len() >= FRAGMENT
                && (0..=k.len() - FRAGMENT).any(|i| folded.contains(&k[i..i + FRAGMENT]))
        })
    });
    if ends_in_a_secret || shows_a_fragment {
        withheld.to_string()
    } else {
        shown
    }
}

// ----- LoTW -------------------------------------------------------------------

#[tauri::command]
pub(crate) async fn lotw_test_connection(state: State<'_, SharedEngine>) -> Result<String, String> {
    let engine = state.inner().clone();
    let res = tauri::async_runtime::spawn_blocking(move || {
        // Read as `download_lotw_report_impl` reads them.
        let username = engine_lock(&engine)
            .settings()
            .lotw_username
            .trim()
            .to_string();
        if username.is_empty() {
            return Err("Set your LoTW username in Settings first.".to_string());
        }
        let password = lotw_keychain()?
            .get_password()
            .map_err(|_| "No LoTW password stored — set it in Settings.".to_string())?;
        lotw_test(&username, &password, &|url| {
            propagation::live::lotw::fetch_report(url)
        })
    })
    .await
    .map_err(|e| format!("LoTW test task failed: {e}"))?;
    conn_logged("LoTW", |s| format!("connection test OK — {s}"), res)
}

/// The LoTW Test with the account and LoTW (`fetch`) supplied.
fn lotw_test(username: &str, password: &str, fetch: Fetch<'_, String>) -> Result<String, String> {
    let body = fetch(&tempo_core::lotw::build_login_test_url(username, password))?;
    match tempo_core::lotw::classify_login_test(&body) {
        LoginCheck::Accepted => Ok(format!(
            "LoTW accepted the website login for {username}. Uploads are signed separately, by \
             TQSL with your certificate."
        )),
        LoginCheck::Refused(words) => Err(format!("LoTW said: {words}")),
        LoginCheck::Unrecognised(words) => Err(cannot_tell("LoTW", words)),
    }
}

// ----- eQSL -------------------------------------------------------------------

#[tauri::command]
pub(crate) async fn eqsl_test_connection(state: State<'_, SharedEngine>) -> Result<String, String> {
    let engine = state.inner().clone();
    let res = tauri::async_runtime::spawn_blocking(move || {
        // Read as `download_eqsl_report_impl` and `eqsl_push_qso_impl` read them.
        let (username, qth_nickname) = {
            let eng = engine_lock(&engine);
            let s = eng.settings();
            (
                s.eqsl_username.trim().to_string(),
                s.eqsl_qth_nickname.trim().to_string(),
            )
        };
        if username.is_empty() {
            return Err("Set your eQSL username in Settings first.".to_string());
        }
        let password = eqsl_keychain()?
            .get_password()
            .map_err(|_| "No eQSL password stored — set it in Settings.".to_string())?;
        eqsl_test(&username, &password, &qth_nickname, &|url| {
            propagation::live::eqsl::fetch_login_page(url)
        })
    })
    .await
    .map_err(|e| format!("eQSL test task failed: {e}"))?;
    conn_logged("eQSL", |s| format!("connection test OK — {s}"), res)
}

/// What the operator is told instead of eQSL's words when they still show the password.
const EQSL_WORDS_WITHHELD: &str = "eQSL refused the login, and its reply echoed the password \
     back, so its wording is withheld. Check the eQSL settings.";

/// The eQSL Test with the account and eQSL (`fetch`, step 1 of the InBox only) supplied.
fn eqsl_test(
    username: &str,
    password: &str,
    qth_nickname: &str,
    fetch: Fetch<'_, String>,
) -> Result<String, String> {
    let page = fetch(&tempo_core::eqsl::build_login_test_url(
        username,
        password,
        qth_nickname,
    ))?;
    let unechoed = |words: &str| {
        shown_words(
            words,
            &[Echoed {
                secret: password,
                marker: "«password»",
            }],
            EQSL_WORDS_WITHHELD,
        )
    };
    match tempo_core::eqsl::classify_login_test(&page) {
        LoginCheck::Accepted => Ok(format!("eQSL accepted the login for {username}.")),
        LoginCheck::Refused(words) => Err(format!("eQSL said: {}", unechoed(&words))),
        LoginCheck::Unrecognised(words) => Err(cannot_tell("eQSL", words.map(|w| unechoed(&w)))),
    }
}

// ----- ClubLog ----------------------------------------------------------------

#[tauri::command]
pub(crate) async fn clublog_test_connection(
    state: State<'_, SharedEngine>,
) -> Result<String, String> {
    let engine = state.inner().clone();
    let res = tauri::async_runtime::spawn_blocking(move || {
        // ClubLog asks a program to stop sending credentials it has refused until the operator
        // replaces them, and firewalls an address that keeps sending them. The upload's latch
        // holds the Test too; saving the password again re-arms both.
        if CLUBLOG_SUSPENDED.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(CLUBLOG_TEST_HELD.to_string());
        }
        // Read as `clublog_push_qso_impl` reads them.
        let (email, callsign_setting, api_setting, mycall) = {
            let eng = engine_lock(&engine);
            let s = eng.settings();
            (
                s.clublog_email.trim().to_string(),
                s.clublog_callsign.trim().to_string(),
                s.clublog_api_key.trim().to_string(),
                s.mycall.trim().to_string(),
            )
        };
        let api_key = effective_clublog_key(&api_setting);
        if api_key.is_empty() {
            return Err("This build has no ClubLog application key. Official installers bundle one; building from source, get a free key at clublog.org/requestapikey.php and add it in Settings ▸ Confirmations.".to_string());
        }
        if email.is_empty() {
            return Err("Set your ClubLog email in Settings first.".to_string());
        }
        let callsign = if callsign_setting.is_empty() {
            mycall
        } else {
            callsign_setting
        };
        let password = clublog_keychain()?
            .get_password()
            .map_err(|_| "No ClubLog app-password stored — set it in Settings.".to_string())?;
        let test = ClubLogTest {
            email: &email,
            password: &password,
            callsign: &callsign,
            api_key: &api_key,
            built_in: &effective_clublog_key(""),
        };
        let res = clublog_test(&test, &|url| propagation::live::clublog::get_status(url));
        if matches!(&res, Err(ClubLogTestFailure::Refused(_))) {
            crate::conn_log(
                "ClubLog",
                "error",
                "the Test was refused — uploads and Tests SUSPENDED until credentials change in \
                 Settings",
            );
            CLUBLOG_SUSPENDED.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        res.map_err(ClubLogTestFailure::into_words)
    })
    .await
    .map_err(|e| format!("ClubLog test task failed: {e}"))?;
    conn_logged("ClubLog", |s| format!("connection test OK — {s}"), res)
}

/// What a ClubLog Test answers while ClubLog's refusal holds the credentials back.
const CLUBLOG_TEST_HELD: &str = "ClubLog refused these credentials earlier this session, so \
     Nexus is not sending them again, as ClubLog asks. Enter the Application Password again in \
     Settings (or fix the email or callsign), then Test.";

/// What the operator is told instead of ClubLog's words when they still show a stored secret.
const CLUBLOG_TEST_WORDS_WITHHELD: &str = "ClubLog refused the login, and its reply echoed a \
     stored password or key back, so its wording is withheld. Check the ClubLog settings.";

/// The four values a ClubLog Test signs in with — the realtime upload's — and the key built in to
/// the installer, which ClubLog's words are masked for even when Settings overrides it.
struct ClubLogTest<'a> {
    email: &'a str,
    password: &'a str,
    callsign: &'a str,
    api_key: &'a str,
    built_in: &'a str,
}

/// Why a ClubLog Test failed. A refusal is ClubLog's 403, which latches the uploads off.
#[derive(Debug, PartialEq)]
enum ClubLogTestFailure {
    Refused(String),
    Other(String),
}

impl ClubLogTestFailure {
    fn into_words(self) -> String {
        match self {
            Self::Refused(words) | Self::Other(words) => words,
        }
    }
}

/// The ClubLog Test with the credentials and ClubLog (`fetch`) supplied.
fn clublog_test(
    test: &ClubLogTest<'_>,
    fetch: Fetch<'_, (u16, String)>,
) -> Result<String, ClubLogTestFailure> {
    let url = tempo_core::clublog::build_login_test_url(
        test.email,
        test.password,
        test.callsign,
        test.api_key,
    );
    let (status, body) = fetch(&url).map_err(ClubLogTestFailure::Other)?;
    let unechoed = |words: &str| {
        shown_words(
            words,
            &[
                Echoed {
                    secret: test.password,
                    marker: "«password»",
                },
                Echoed {
                    secret: test.api_key,
                    marker: "«api key»",
                },
                Echoed {
                    secret: test.built_in,
                    marker: "«api key»",
                },
            ],
            CLUBLOG_TEST_WORDS_WITHHELD,
        )
    };
    match tempo_core::clublog::classify_login_test(status, &body) {
        LoginCheck::Accepted => Ok(format!(
            "ClubLog accepted {} for the logbook {}.",
            test.email, test.callsign
        )),
        LoginCheck::Refused(words) => Err(ClubLogTestFailure::Refused(format!(
            "ClubLog said: {}",
            unechoed(&words)
        ))),
        LoginCheck::Unrecognised(words) => Err(ClubLogTestFailure::Other(cannot_tell(
            "ClubLog",
            Some(match words {
                Some(words) => format!("HTTP {status}: {}", unechoed(&words)),
                None => format!("HTTP {status}"),
            }),
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const PASSWORD: &str = "Zx81qPv3Lm0K";
    const API_KEY: &str = "Jw4Nc8Tb1Hy6";
    const BUILT_IN: &str = "Bt5Qx2Wd7Kp9";

    /// Every URL a Test sent, in order.
    #[derive(Default)]
    struct Sent(RefCell<Vec<String>>);

    impl Sent {
        fn record(&self, url: &str) {
            self.0.borrow_mut().push(url.to_string());
        }
        fn urls(&self) -> Vec<String> {
            self.0.borrow().clone()
        }
    }

    /// The upload endpoints — none of them may ever be reached from a Test. (LoTW's upload is
    /// TQSL, a program run on the contacts, not a URL: the LoTW Test takes no contacts and runs
    /// nothing, and its one request is pinned below.)
    const UPLOAD_ENDPOINTS: [&str; 3] = [
        tempo_core::eqsl::EQSL_IMPORT_URL,
        tempo_core::clublog::CLUBLOG_REALTIME_URL,
        "https://clublog.org/putlogs.php",
    ];

    /// The URLs in `sent` that reach an upload endpoint or carry a record.
    fn uploads(sent: &[String]) -> Vec<String> {
        sent.iter()
            .filter(|url| {
                UPLOAD_ENDPOINTS
                    .iter()
                    .any(|endpoint| url.starts_with(endpoint))
                    || url.to_ascii_lowercase().contains("adif")
            })
            .cloned()
            .collect()
    }

    /// The negative control's positive control: the upload check catches each service's real
    /// upload request, so its silence on the Tests below means something.
    #[test]
    fn the_upload_check_catches_a_real_upload() {
        let eqsl = format!(
            "{}?{}",
            tempo_core::eqsl::EQSL_IMPORT_URL,
            tempo_core::eqsl::build_upload_body("W9TST", PASSWORD, "<call:4>W1AW<eor>", None, "1")
        );
        let clublog = format!(
            "{}?{}",
            tempo_core::clublog::CLUBLOG_REALTIME_URL,
            tempo_core::clublog::build_realtime_body(&tempo_core::clublog::ClubLogQuery {
                email: "a@b.invalid".into(),
                password: PASSWORD.into(),
                callsign: "W9TST".into(),
                api_key: API_KEY.into(),
                adif: "<call:4>W1AW<eor>".into(),
            })
        );
        assert_eq!(
            uploads(&[eqsl.clone(), clublog.clone()]),
            vec![eqsl, clublog]
        );
    }

    // ----- LoTW ---------------------------------------------------------------

    #[test]
    fn a_lotw_test_sends_one_read_and_answers_ok() {
        let sent = Sent::default();
        let answer = lotw_test("w9tst", PASSWORD, &|url| {
            sent.record(url);
            Ok("ARRL Logbook of the World Status Report\n<eoh>\n".into())
        });
        assert_eq!(
            answer,
            Ok(
                "LoTW accepted the website login for w9tst. Uploads are signed separately, by \
                TQSL with your certificate."
                    .into()
            )
        );
        let urls = sent.urls();
        assert_eq!(
            urls,
            vec![tempo_core::lotw::build_login_test_url("w9tst", PASSWORD)]
        );
        assert_eq!(uploads(&urls), Vec::<String>::new());
    }

    #[test]
    fn a_lotw_test_answers_lotws_refusal_and_a_network_failure() {
        assert_eq!(
            lotw_test("w9tst", PASSWORD, &|_| Ok(
                "<html><p>Username/password incorrect</p></html>".into()
            )),
            Err("LoTW said: Username/password incorrect".into())
        );
        assert_eq!(
            lotw_test("w9tst", PASSWORD, &|_| Err(
                "LoTW: could not connect — check your internet connection".into()
            )),
            Err("LoTW: could not connect — check your internet connection".into())
        );
        let unknown = lotw_test(
            "w9tst",
            PASSWORD,
            &|_| Ok("<html>maintenance</html>".into()),
        );
        assert!(
            unknown.as_ref().is_err_and(|e| e.contains("cannot tell")),
            "{unknown:?}"
        );
    }

    // ----- eQSL ---------------------------------------------------------------

    #[test]
    fn an_eqsl_test_sends_one_read_and_answers_ok() {
        let sent = Sent::default();
        let answer = eqsl_test("W9TST", PASSWORD, "Home", &|url| {
            sent.record(url);
            Ok("<html>You have no log entries</html>".into())
        });
        assert_eq!(answer, Ok("eQSL accepted the login for W9TST.".into()));
        let urls = sent.urls();
        assert_eq!(
            urls,
            vec![tempo_core::eqsl::build_login_test_url(
                "W9TST", PASSWORD, "Home"
            )]
        );
        assert_eq!(uploads(&urls), Vec::<String>::new());
    }

    #[test]
    fn an_eqsl_test_answers_eqsls_refusal_without_the_password() {
        assert_eq!(
            eqsl_test("W9TST", PASSWORD, "", &|_| Ok(
                "<H3>Error: No such Callsign found</H3>".into()
            )),
            Err("eQSL said: Error: No such Callsign found".into())
        );
        let echoed = eqsl_test("W9TST", PASSWORD, "", &|_| {
            Ok(format!("<H3>Error: bad password {PASSWORD}</H3>"))
        });
        assert_eq!(
            echoed,
            Err("eQSL said: Error: bad password «password»".into())
        );
        let reformatted = eqsl_test("W9TST", PASSWORD, "", &|_| {
            Ok(format!("<H3>Error: {}</H3>", PASSWORD.to_lowercase()))
        });
        assert_eq!(
            reformatted,
            Err(format!("eQSL said: {EQSL_WORDS_WITHHELD}"))
        );
        assert_eq!(
            eqsl_test("W9TST", PASSWORD, "", &|_| Err(
                "eQSL: request timed out — eQSL can be slow, try again shortly".into()
            )),
            Err("eQSL: request timed out — eQSL can be slow, try again shortly".into())
        );
    }

    /// How many characters of a service's words a Test shows, read off the cap itself.
    fn cap() -> usize {
        tempo_core::login_check::capped(&"a".repeat(10_000))
            .trim_end_matches('…')
            .chars()
            .count()
    }

    /// `head` padded with dots so the next character written after it is the `at`-th.
    fn padded_to(head: &str, at: usize) -> String {
        format!("{head}{}", ".".repeat(at - head.chars().count()))
    }

    /// Whether `words` show the first four or more characters of `secret`, in any case.
    fn shows_a_prefix_of(words: &str, secret: &str) -> bool {
        let words = words.to_lowercase();
        let secret = secret.to_lowercase();
        (4..=secret.len()).any(|n| words.contains(&secret[..n]))
    }

    /// eQSL's words are masked in full before they are cut: a password that starts just before
    /// the cut never leaves its first characters behind. Controls: a short refusal is shown as
    /// eQSL wrote it, a password mid-sentence is masked with the words around it kept, and a long
    /// refusal with no secret in it is cut and still shown.
    #[test]
    fn an_eqsl_test_masks_the_password_before_it_cuts_the_words() {
        let head = padded_to("Error: bad login ", cap() - 4);
        let words = eqsl_test("W9TST", PASSWORD, "", &|_| {
            Ok(format!("<H3>{head}{PASSWORD} for this account</H3>"))
        })
        .expect_err("an Error: line is a refusal");
        assert!(!shows_a_prefix_of(&words, PASSWORD), "{words}");
        // A partial echo of the password, ending where the words are cut, is withheld.
        let words = eqsl_test("W9TST", PASSWORD, "", &|_| {
            Ok(format!("<H3>{head}{} and more</H3>", &PASSWORD[..6]))
        })
        .expect_err("an Error: line is a refusal");
        assert!(!shows_a_prefix_of(&words, PASSWORD), "{words}");
        // Controls.
        assert_eq!(
            eqsl_test("W9TST", PASSWORD, "", &|_| Ok(
                "<H3>Error: Bad password</H3>".into()
            )),
            Err("eQSL said: Error: Bad password".into())
        );
        assert_eq!(
            eqsl_test("W9TST", PASSWORD, "", &|_| Ok(format!(
                "<H3>Error: password {PASSWORD} is wrong</H3>"
            ))),
            Err("eQSL said: Error: password «password» is wrong".into())
        );
        let long = eqsl_test("W9TST", PASSWORD, "", &|_| {
            Ok(format!(
                "<H3>{}</H3>",
                padded_to("Error: busy ", cap() + 50)
            ))
        })
        .expect_err("an Error: line is a refusal");
        assert!(
            long.starts_with("eQSL said: Error: busy ..") && long.ends_with('…'),
            "{long}"
        );
    }

    /// A fragment of the password of six or more characters, anywhere in eQSL's words and in any
    /// case or encoding, withholds them: the start or end of the password echoed mid-sentence (a
    /// reply eQSL itself cut short). Controls: a refusal with no fragment is shown as eQSL wrote
    /// it, and a five-character overlap is a coincidence, not an echo.
    #[test]
    fn an_eqsl_test_withholds_words_that_show_a_fragment_of_the_password() {
        let withheld = Err(format!("eQSL said: {EQSL_WORDS_WITHHELD}"));
        for echo in [
            format!("Error: pwd {} truncated, try again", &PASSWORD[..6]),
            format!("Error: pwd ...{} try again", &PASSWORD[4..]),
            format!("Error: pwd {} truncated", PASSWORD[2..9].to_lowercase()),
        ] {
            let answer = eqsl_test("W9TST", PASSWORD, "", &|_| Ok(format!("<H3>{echo}</H3>")));
            assert_eq!(answer, withheld, "{echo}");
        }
        // Percent-encoded: `-` and `&` reach the words as `%2D` and `%26`.
        let symbols = "Pq-7&Rs9tU";
        let answer = eqsl_test("W9TST", symbols, "", &|_| {
            Ok("<H3>Error: pwd Pq%2D7%26R truncated</H3>".into())
        });
        assert_eq!(answer, withheld);
        // Controls.
        assert_eq!(
            eqsl_test("W9TST", PASSWORD, "", &|_| Ok(
                "<H3>Error: Bad password, try again</H3>".into()
            )),
            Err("eQSL said: Error: Bad password, try again".into())
        );
        assert_eq!(
            eqsl_test("W9TST", PASSWORD, "", &|_| Ok(format!(
                "<H3>Error: code {} is not valid</H3>",
                &PASSWORD[..5]
            ))),
            Err(format!(
                "eQSL said: Error: code {} is not valid",
                &PASSWORD[..5]
            ))
        );
    }

    // ----- ClubLog ------------------------------------------------------------

    fn clublog() -> ClubLogTest<'static> {
        ClubLogTest {
            email: "w9tst@example.invalid",
            password: PASSWORD,
            callsign: "W9TST",
            api_key: API_KEY,
            built_in: BUILT_IN,
        }
    }

    #[test]
    fn a_clublog_test_sends_one_read_and_answers_ok() {
        let sent = Sent::default();
        let answer = clublog_test(&clublog(), &|url| {
            sent.record(url);
            Ok((200, "[]".into()))
        });
        assert_eq!(
            answer,
            Ok("ClubLog accepted w9tst@example.invalid for the logbook W9TST.".into())
        );
        let urls = sent.urls();
        assert_eq!(
            urls,
            vec![tempo_core::clublog::build_login_test_url(
                "w9tst@example.invalid",
                PASSWORD,
                "W9TST",
                API_KEY
            )]
        );
        assert_eq!(uploads(&urls), Vec::<String>::new());
    }

    /// A 403 is a refusal (the command latches uploads and Tests off on it); its words come
    /// through without the password or either key, however ClubLog echoes them.
    #[test]
    fn a_clublog_test_answers_a_refusal_without_the_secrets() {
        assert_eq!(
            clublog_test(&clublog(), &|_| Ok((403, "Invalid login".into()))),
            Err(ClubLogTestFailure::Refused(
                "ClubLog said: Invalid login".into()
            ))
        );
        for echo in [
            format!("password={PASSWORD}&api={API_KEY}"),
            format!("api={BUILT_IN}"),
            format!("pw {}", PASSWORD.to_uppercase()),
        ] {
            let answer = clublog_test(&clublog(), &|_| Ok((403, echo.clone())));
            let words = answer.expect_err("a 403 is a refusal").into_words();
            for secret in [PASSWORD, API_KEY, BUILT_IN] {
                assert!(
                    !words.to_lowercase().contains(&secret.to_lowercase()),
                    "{echo} -> {words}"
                );
            }
        }
        // Controls: a server error and a network failure are not refusals, so they latch nothing.
        assert!(matches!(
            clublog_test(&clublog(), &|_| Ok((500, String::new()))),
            Err(ClubLogTestFailure::Other(_))
        ));
        assert_eq!(
            clublog_test(&clublog(), &|_| Err("ClubLog: could not connect".into())),
            Err(ClubLogTestFailure::Other(
                "ClubLog: could not connect".into()
            ))
        );
    }

    /// ClubLog's words are masked in full before they are cut: neither the password nor a key
    /// that starts just before the cut leaves its first characters behind, in a refusal or in an
    /// answer Nexus cannot read. Controls as for eQSL.
    #[test]
    fn a_clublog_test_masks_the_secrets_before_it_cuts_the_words() {
        let head = padded_to("Invalid login ", cap() - 4);
        for secret in [PASSWORD, API_KEY, BUILT_IN] {
            for status in [403, 200] {
                let body = format!("{head}{secret} for this account");
                let words = clublog_test(&clublog(), &|_| Ok((status, body.clone())))
                    .expect_err("neither is a good login")
                    .into_words();
                assert!(!shows_a_prefix_of(&words, secret), "{status}: {words}");
            }
            // A partial echo, ending where the words are cut, is withheld.
            let body = format!("{head}{} and more", &secret[..6]);
            let words = clublog_test(&clublog(), &|_| Ok((403, body.clone())))
                .expect_err("a 403 is a refusal")
                .into_words();
            assert!(!shows_a_prefix_of(&words, secret), "{words}");
        }
        // Controls.
        assert_eq!(
            clublog_test(&clublog(), &|_| Ok((403, format!("api {API_KEY} refused")))),
            Err(ClubLogTestFailure::Refused(
                "ClubLog said: api «api key» refused".into()
            ))
        );
        let long = clublog_test(&clublog(), &|_| {
            Ok((403, padded_to("Invalid login ", cap() + 50)))
        })
        .expect_err("a 403 is a refusal")
        .into_words();
        assert!(
            long.starts_with("ClubLog said: Invalid login ..") && long.ends_with('…'),
            "{long}"
        );
    }

    /// A fragment of six or more characters of the password or either key, anywhere in ClubLog's
    /// words, withholds them, in a refusal and in an answer Nexus cannot read. Controls: a refusal
    /// with no fragment is shown as ClubLog wrote it, and a five-character overlap is not an echo.
    #[test]
    fn a_clublog_test_withholds_words_that_show_a_fragment_of_a_secret() {
        for secret in [PASSWORD, API_KEY, BUILT_IN] {
            for echo in [
                format!("Invalid login {} truncated, try again", &secret[..6]),
                format!("Invalid login ...{} try again", &secret[4..]),
            ] {
                for status in [403, 200] {
                    let words = clublog_test(&clublog(), &|_| Ok((status, echo.clone())))
                        .expect_err("neither is a good login")
                        .into_words();
                    assert!(
                        words.contains(CLUBLOG_TEST_WORDS_WITHHELD) && !words.contains("Invalid"),
                        "{status}: {echo} -> {words}"
                    );
                }
            }
        }
        // Controls.
        assert_eq!(
            clublog_test(&clublog(), &|_| Ok((
                403,
                "Invalid login, try again".into()
            ))),
            Err(ClubLogTestFailure::Refused(
                "ClubLog said: Invalid login, try again".into()
            ))
        );
        assert_eq!(
            clublog_test(&clublog(), &|_| Ok((
                403,
                format!("Invalid login {} refused", &API_KEY[..5])
            ))),
            Err(ClubLogTestFailure::Refused(format!(
                "ClubLog said: Invalid login {} refused",
                &API_KEY[..5]
            )))
        );
    }
}
