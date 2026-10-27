//! An ID token from the account's issuer (OpenID Connect Core 1.0 §2), checked on this computer
//! rather than taken on the service's word: a device or a station that signs in puts a hash of its
//! own keys in the sign-in's `nonce`, and the issuer signs a token carrying it back. Whoever checks
//! that token here knows the issuer said "this subject signed in, for these keys", and the service
//! in between could store and forward it but never mint one.
//!
//! [`verify`] checks, in this order, and the first failure is the answer:
//! - **The algorithm** is RS256 and nothing else. `none`, HS256 (a public key used as an HMAC
//!   secret) and every other `alg` are refused before any key is looked at, and so is a `crit`
//!   header: no extension is understood here (RFC 7515 §4.1.11).
//! - **The key** is the issuer's key set entry with the header's `kid`, an RSA key for signing.
//!   An unknown `kid` is refused; the caller fetches the set again and asks once more.
//! - **The signature** over the first two parts exactly as sent (RSASSA-PKCS1-v1_5, SHA-256, a
//!   2048 to 8192-bit modulus: ring's own floor). Nothing in the payload is read before it holds.
//! - **The issuer** is exactly the one the caller trusts, **the audience** names the caller's client
//!   (when it names several, `azp` must be that client: §3.1.3.7 rules 3 to 5), **the life**: `exp`
//!   not passed, with [`LEEWAY_S`] for two clocks, and **the nonce** is exactly the one the sign-in
//!   sent.
//!
//! The key set and the clock are the caller's: nothing here fetches, stores or keeps a token.
use ring::signature::{RsaPublicKeyComponents, RSA_PKCS1_2048_8192_SHA256};
use serde_json::Value;

/// How far this computer's clock and the issuer's may disagree about `exp`.
pub const LEEWAY_S: u64 = 60;

/// What the caller trusts and expects of a token: the issuer (its `iss`, exactly), the client the
/// sign-in was for, and the nonce it sent.
pub struct Expected<'a> {
    pub issuer: &'a str,
    pub client_id: &'a str,
    pub nonce: &'a str,
}

/// What a token that held says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claims {
    /// The issuer's subject: who signed in.
    pub subject: String,
    /// The address, and whether the issuer verified it.
    pub email: Option<String>,
    pub email_verified: bool,
    /// When it was issued and when it lapses, seconds since 1970.
    pub issued_at: u64,
    pub expires_at: u64,
    /// How the subject signed in (`amr`), for one who asks whether a second factor was used.
    pub methods: Vec<String>,
}

/// A token's claims when every check holds, else the first one that did not: `tokenMalformed`,
/// `tokenAlgorithm`, `tokenKey`, `tokenSignature`, `tokenIssuer`, `tokenAudience`,
/// `tokenExpired`, `tokenNonce`. `keys` is the issuer's key set (`{"keys":[…]}`) as fetched;
/// `now_s` is seconds since 1970.
pub fn verify(
    token: &str,
    keys: &Value,
    expected: &Expected,
    now_s: u64,
) -> Result<Claims, &'static str> {
    let claims = signed(token, keys, expected.issuer, expected.client_id)?;
    let expires_at = claims["exp"].as_u64().ok_or("tokenExpired")?;
    if now_s >= expires_at.saturating_add(LEEWAY_S) {
        return Err("tokenExpired");
    }
    if claims["nonce"] != expected.nonce {
        return Err("tokenNonce");
    }
    claims_of(&claims)
}

const MALFORMED: &str = "tokenMalformed";

/// A token's payload once its algorithm, key, signature, issuer and audience hold, in that order:
/// the checks every use of a token shares. What else holds (its life, its nonce) is the caller's.
pub(crate) fn signed(
    token: &str,
    keys: &Value,
    issuer: &str,
    client: &str,
) -> Result<Value, &'static str> {
    let parts: Vec<&str> = token.split('.').collect();
    let [header, payload, signature] = parts[..] else {
        return Err(MALFORMED);
    };
    let header: Value = unbase64url(header)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .ok_or(MALFORMED)?;
    let payload = unbase64url(payload).ok_or(MALFORMED)?;
    let signature = unbase64url(signature).ok_or(MALFORMED)?;

    if header["alg"] != "RS256" || header.get("crit").is_some() {
        return Err("tokenAlgorithm");
    }
    let kid = header["kid"].as_str().ok_or("tokenKey")?;
    let key = keys["keys"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|key| {
            key["kid"] == kid
                && key["kty"] == "RSA"
                && key.get("use").is_none_or(|v| v == "sig")
                && key.get("alg").is_none_or(|v| v == "RS256")
        })
        .ok_or("tokenKey")?;
    let component = |name: &str| key[name].as_str().and_then(unbase64url).ok_or("tokenKey");
    let public = RsaPublicKeyComponents {
        n: component("n")?,
        e: component("e")?,
    };
    let signed = &token[..token.rfind('.').unwrap_or(0)];
    public
        .verify(&RSA_PKCS1_2048_8192_SHA256, signed.as_bytes(), &signature)
        .map_err(|_| "tokenSignature")?;

    let claims: Value = serde_json::from_slice(&payload).map_err(|_| MALFORMED)?;
    issued_for(&claims, issuer, client)?;
    Ok(claims)
}

/// Whether a payload names exactly `issuer`, and is for `client` (when its audience names several,
/// `azp` must be that client: §3.1.3.7 rules 3 to 5): `tokenIssuer` or `tokenAudience` when not.
pub(crate) fn issued_for(claims: &Value, issuer: &str, client: &str) -> Result<(), &'static str> {
    if claims["iss"] != issuer {
        return Err("tokenIssuer");
    }
    let audience = match &claims["aud"] {
        Value::String(aud) => aud == client,
        Value::Array(auds) => {
            auds.iter().any(|aud| aud == client) && (auds.len() == 1 || claims["azp"] == client)
        }
        _ => false,
    };
    if !audience || claims.get("azp").is_some_and(|azp| azp != client) {
        return Err("tokenAudience");
    }
    Ok(())
}

/// What a payload that held says. `tokenMalformed` without a subject or a signing time.
pub(crate) fn claims_of(claims: &Value) -> Result<Claims, &'static str> {
    Ok(Claims {
        subject: claims["sub"]
            .as_str()
            .filter(|sub| !sub.is_empty())
            .ok_or(MALFORMED)?
            .to_string(),
        email: claims["email"].as_str().map(str::to_string),
        email_verified: claims["email_verified"] == true,
        issued_at: claims["iat"].as_u64().ok_or(MALFORMED)?,
        expires_at: claims["exp"].as_u64().ok_or(MALFORMED)?,
        methods: claims["amr"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m.as_str().map(str::to_string))
            .collect(),
    })
}

/// base64url with no padding (RFC 4648 §5), strictly: any other byte, or a length no encoding
/// has, is `None`.
pub(crate) fn unbase64url(text: &str) -> Option<Vec<u8>> {
    if text.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for b in text.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        acc = acc << 6 | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::base64url;

    // Minted once by a throwaway RSA key that never entered the repository: only its public half is
    // in the fixture's key set, so these tokens prove nothing to anyone.
    const CASES: &str = include_str!("../tests/fixtures/id-token-cases.json");

    struct Case {
        all: Value,
    }

    impl Case {
        fn load() -> Self {
            Self {
                all: serde_json::from_str(CASES).unwrap(),
            }
        }
        fn token(&self, name: &str) -> &str {
            self.all["tokens"][name].as_str().unwrap()
        }
        fn now(&self) -> u64 {
            self.all["now"].as_u64().unwrap()
        }
        fn exp(&self) -> u64 {
            self.all["exp"].as_u64().unwrap()
        }
        fn expected(&self) -> Expected<'_> {
            Expected {
                issuer: self.all["issuer"].as_str().unwrap(),
                client_id: self.all["clientId"].as_str().unwrap(),
                nonce: self.all["nonce"].as_str().unwrap(),
            }
        }
        fn check(&self, name: &str, now: u64) -> Result<Claims, &'static str> {
            verify(self.token(name), &self.all["jwks"], &self.expected(), now)
        }
    }

    #[test]
    fn a_good_token_gives_its_claims() {
        let case = Case::load();
        assert_eq!(
            case.check("good", case.now()),
            Ok(Claims {
                subject: "auth0|test-subject".into(),
                email: Some("operator@example.test".into()),
                email_verified: true,
                issued_at: case.now() - 5,
                expires_at: case.exp(),
                methods: vec![],
            })
        );
    }

    #[test]
    fn a_signature_by_another_key_or_over_another_payload_is_refused() {
        let case = Case::load();
        assert_eq!(case.check("forged", case.now()), Err("tokenSignature"));
        // The good token's signature over a payload naming someone else.
        let good: Vec<&str> = case.token("good").split('.').collect();
        let mut payload: Value = serde_json::from_slice(&unbase64url(good[1]).unwrap()).unwrap();
        payload["sub"] = "auth0|someone-else".into();
        let swapped = format!(
            "{}.{}.{}",
            good[0],
            base64url(payload.to_string().as_bytes()),
            good[2]
        );
        assert_eq!(
            verify(&swapped, &case.all["jwks"], &case.expected(), case.now()),
            Err("tokenSignature")
        );
    }

    #[test]
    fn a_key_the_set_does_not_have_is_refused() {
        let case = Case::load();
        assert_eq!(case.check("unknownKid", case.now()), Err("tokenKey"));
        // The right kid in a set whose entry is not an RS256 signing RSA key.
        for (field, value) in [("use", "enc"), ("kty", "oct"), ("alg", "RS512")] {
            let mut keys = case.all["jwks"].clone();
            keys["keys"][0][field] = value.into();
            assert_eq!(
                verify(case.token("good"), &keys, &case.expected(), case.now()),
                Err("tokenKey"),
                "{field}"
            );
        }
    }

    #[test]
    fn a_token_past_its_life_and_the_leeway_is_refused() {
        let case = Case::load();
        let exp = case.exp();
        assert!(case.check("good", exp + LEEWAY_S - 1).is_ok());
        assert_eq!(case.check("good", exp + LEEWAY_S), Err("tokenExpired"));
        assert_eq!(case.check("good", exp + 86_400), Err("tokenExpired"));
    }

    #[test]
    fn a_token_for_another_sign_in_is_refused() {
        let case = Case::load();
        assert_eq!(case.check("wrongNonce", case.now()), Err("tokenNonce"));
        assert_eq!(case.check("noNonce", case.now()), Err("tokenNonce"));
    }

    #[test]
    fn a_token_for_another_client_is_refused() {
        let case = Case::load();
        assert_eq!(case.check("wrongAud", case.now()), Err("tokenAudience"));
        assert!(case.check("audListAzp", case.now()).is_ok());
        for name in [
            "audListOtherAzp",
            "audListNoAzp",
            "audListWithout",
            "azpOther",
        ] {
            assert_eq!(case.check(name, case.now()), Err("tokenAudience"), "{name}");
        }
    }

    #[test]
    fn a_token_from_another_issuer_is_refused() {
        let case = Case::load();
        assert_eq!(case.check("wrongIss", case.now()), Err("tokenIssuer"));
    }

    #[test]
    fn only_rs256_is_taken() {
        let case = Case::load();
        assert_eq!(case.check("algNone", case.now()), Err("tokenAlgorithm"));
        assert_eq!(case.check("algHs256", case.now()), Err("tokenAlgorithm"));
        assert_eq!(case.check("crit", case.now()), Err("tokenAlgorithm"));
    }

    #[test]
    fn a_token_that_is_not_three_base64url_parts_is_malformed() {
        let case = Case::load();
        let good = case.token("good");
        for bad in [
            "",
            "a.b",
            &format!("{good}.x"),
            &good.replacen('.', ".=", 1),
        ] {
            assert_eq!(
                verify(bad, &case.all["jwks"], &case.expected(), case.now()),
                Err("tokenMalformed"),
                "{bad}"
            );
        }
    }

    #[test]
    fn base64url_decodes_what_it_encodes() {
        for n in 0..=7u8 {
            let bytes: Vec<u8> = (0..n).map(|i| i.wrapping_mul(97) ^ 0xA5).collect();
            assert_eq!(unbase64url(&base64url(&bytes)), Some(bytes));
        }
        assert_eq!(unbase64url("abcde"), None);
        assert_eq!(unbase64url("ab+/"), None);
    }
}
