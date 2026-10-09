//! The configuration of one Icom network session: the radio's address and model, which
//! advertised radio to use, and the network user it logs in as.
//!
//! Nexus's own, not a port: upstream's configuration tokens are not taken. What these types
//! enforce are facts of the protocol, refused when the value is made rather than truncated on the
//! wire later: a user name or password of at most 16 printable ASCII characters (the obfuscated
//! field holds 16, and the cipher has an entry only for printable characters), and a control port
//! that is never 0. There is no setting for the liveness window (fixed at 5000 ms per socket,
//! never "0 = never"), for transmit (the connection request always carries transmit-enable 0) or
//! for audio (not started at this stage).
//!
//! **The password is a [`Secret`].** It cannot be printed (`Debug` shows `<redacted>`, and there
//! is no `Display`), serialised (no `Serialize`) or cloned (no `Clone`). It is moved into the
//! session, and from there into the login builder, which encodes it and drops it. The user name
//! ([`User`]) is not secret, but it is not logged either: its `Debug` shows only its length.

use std::fmt;
use std::net::Ipv4Addr;
use std::num::NonZeroU16;

use super::caps::{Choice, Model};
use super::wire::{PASSCODE_MAX, PORT_CONTROL};

/// Which credential a [`ConfError`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    User,
    Password,
}

/// A credential the protocol cannot carry. Names the field, never the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfError {
    /// More than [`PASSCODE_MAX`] characters.
    TooLong(Field),
    /// A character outside printable ASCII, which the cipher cannot encode.
    NotPrintable(Field),
}

impl fmt::Display for ConfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (field, problem) = match self {
            ConfError::TooLong(field) => (field, "is longer than 16 characters"),
            ConfError::NotPrintable(field) => (field, "has a character the radio cannot take"),
        };
        let field = match field {
            Field::User => "the network user name",
            Field::Password => "the network password",
        };
        write!(f, "{field} {problem}")
    }
}

impl std::error::Error for ConfError {}

fn credential(text: &str, field: Field) -> Result<(), ConfError> {
    if text.len() > PASSCODE_MAX {
        return Err(ConfError::TooLong(field));
    }
    if !text.bytes().all(|c| (b' '..=b'~').contains(&c)) {
        return Err(ConfError::NotPrintable(field));
    }
    Ok(())
}

/// The radio's network user name (a network user configured in the radio's own menu).
#[derive(Clone, PartialEq, Eq)]
pub struct User(String);

impl User {
    pub fn new(name: &str) -> Result<User, ConfError> {
        credential(name, Field::User)?;
        Ok(User(name.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "User({} characters)", self.0.len())
    }
}

/// The network user's password. Never printed, serialised or cloned; see the module notes.
pub struct Secret(String);

impl Secret {
    /// Takes the password by value, so no copy of it is made here.
    pub fn new(password: String) -> Result<Secret, ConfError> {
        credential(&password, Field::Password)?;
        Ok(Secret(password))
    }

    /// The text, for the login builder alone.
    pub(super) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// The network user a session logs in as.
#[derive(Debug)]
pub struct Login {
    pub user: User,
    pub password: Secret,
}

/// One session's configuration.
#[derive(Debug)]
pub struct Config {
    /// The radio's address. The protocol's connection ids come from IPv4 addresses.
    pub host: Ipv4Addr,
    /// The radio's control port.
    pub control_port: NonZeroU16,
    /// The radio model: the name its server is expected to report, and the default choice.
    pub model: Model,
    /// Which advertised radio to use; `None` picks the one named for the model, or the only one
    /// advertised (a warning when its name differs, never a refusal).
    pub choice: Option<Choice>,
    pub login: Login,
}

impl Config {
    /// The defaults: the radio's standard control port, the model's own radio.
    pub fn new(host: Ipv4Addr, model: Model, login: Login) -> Config {
        Config {
            host,
            control_port: NonZeroU16::new(PORT_CONTROL).expect("a nonzero port"),
            model,
            choice: None,
            login,
        }
    }
}

#[cfg(test)]
mod tests {
    //! Nexus's own rules for the credential types. The upstream configuration cases these stand
    //! beside are translated in the session's tests, where what the radio receives can be seen.
    use super::*;

    /// Resolves to one item when `T` lacks the trait in `A`'s second impl, and is ambiguous (a
    /// compile error) when `T` has it: a guard that fails by not building.
    trait AmbiguousIfImpl<A> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
    struct IfClone;
    impl<T: Clone> AmbiguousIfImpl<IfClone> for T {}
    struct IfDisplay;
    impl<T: ?Sized + fmt::Display> AmbiguousIfImpl<IfDisplay> for T {}
    struct IfSerialize;
    impl<T: ?Sized + serde::Serialize> AmbiguousIfImpl<IfSerialize> for T {}

    #[test]
    fn the_password_cannot_be_cloned_displayed_or_serialised() {
        // Each line stops compiling if `Secret` gains the trait.
        <Secret as AmbiguousIfImpl<_>>::check();
        <Login as AmbiguousIfImpl<_>>::check();
        <Config as AmbiguousIfImpl<_>>::check();
    }

    #[test]
    fn the_password_never_formats() {
        let login = Login {
            user: User::new("test-user").unwrap(),
            password: Secret::new("not-a-password".into()).unwrap(),
        };
        let config = Config::new(Ipv4Addr::new(192, 0, 2, 1), Model::Ic7760, login);
        let text = format!("{config:?} {:#?}", config.login);
        assert!(text.contains("<redacted>"), "{text}");
        assert!(!text.contains("not-a-password"), "{text}");
        // the user name is not logged either
        assert!(!text.contains("test-user"), "{text}");
        assert!(text.contains("User(9 characters)"), "{text}");
    }

    #[test]
    fn a_credential_the_wire_cannot_carry_is_refused_when_it_is_made() {
        assert!(User::new("0123456789abcdef").is_ok());
        assert_eq!(
            User::new("0123456789abcdefg"),
            Err(ConfError::TooLong(Field::User))
        );
        assert!(Secret::new("0123456789abcdef".into()).is_ok());
        assert!(matches!(
            Secret::new("0123456789abcdefg".into()),
            Err(ConfError::TooLong(Field::Password))
        ));
        for bad in ["tab\tin", "caf\u{e9}", "nul\0"] {
            assert!(matches!(
                Secret::new(bad.into()),
                Err(ConfError::NotPrintable(Field::Password))
            ));
            assert_eq!(User::new(bad), Err(ConfError::NotPrintable(Field::User)));
        }
        // the refusal names the field, never the value
        let e = Secret::new("0123456789abcdef-secret".into()).unwrap_err();
        assert_eq!(
            e.to_string(),
            "the network password is longer than 16 characters"
        );
    }

    #[test]
    fn the_defaults() {
        let login = Login {
            user: User::new("test-user").unwrap(),
            password: Secret::new("not-a-password".into()).unwrap(),
        };
        let config = Config::new(Ipv4Addr::new(192, 0, 2, 1), Model::Ic9700, login);
        assert_eq!(config.control_port.get(), 50001);
        assert_eq!(config.choice, None);
    }
}
