use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::signature::{RSA_PKCS1_2048_8192_SHA256, RsaPublicKeyComponents};
use serde::Deserialize;

use super::{DirectError, secure_equal};

const MAX_JWT_BYTES: usize = 64 * 1024;

/// Opaque, validated ChatGPT workspace/account identifier.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ChatGptAccountId(String);

impl ChatGptAccountId {
    pub(crate) fn parse(value: String) -> Result<Self, DirectError> {
        if value.is_empty()
            || value.len() > 256
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(DirectError::Jwt(
                "invalid ChatGPT account identifier claim".to_owned(),
            ));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn fixture(value: &str) -> Result<Self, DirectError> {
        Self::parse(value.to_owned())
    }
}

impl std::fmt::Debug for ChatGptAccountId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ChatGptAccountId(<redacted>)")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct JsonWebKeySet {
    keys: Vec<JsonWebKey>,
}

#[derive(Debug, Clone, Deserialize)]
struct JsonWebKey {
    kty: String,
    kid: String,
    #[serde(rename = "use")]
    usage: Option<String>,
    alg: Option<String>,
    n: String,
    e: String,
}

#[derive(Debug, Deserialize)]
struct JwtHeader {
    alg: String,
    kid: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Deserialize)]
struct NamespacedAuthClaims {
    chatgpt_account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    iss: String,
    aud: Audience,
    exp: u64,
    #[serde(default)]
    nbf: Option<u64>,
    #[serde(default)]
    iat: Option<u64>,
    #[serde(default)]
    azp: Option<String>,
    nonce: String,
    #[serde(default)]
    chatgpt_account_id: Option<String>,
    #[serde(default, rename = "https://api.openai.com/auth")]
    namespaced_auth: Option<NamespacedAuthClaims>,
}

pub(crate) struct OidcVerifier {
    issuer: String,
    audience: String,
    jwks: JsonWebKeySet,
}

impl OidcVerifier {
    pub(crate) fn new(
        issuer: impl Into<String>,
        audience: impl Into<String>,
        jwks: JsonWebKeySet,
    ) -> Result<Self, DirectError> {
        let issuer = issuer.into();
        let audience = audience.into();
        if issuer.is_empty() || audience.is_empty() || jwks.keys.is_empty() {
            return Err(DirectError::Configuration(
                "OIDC issuer, audience, and JWKS must be non-empty".to_owned(),
            ));
        }
        Ok(Self {
            issuer,
            audience,
            jwks,
        })
    }

    pub(crate) fn verify(
        &self,
        token: &str,
        expected_nonce: &str,
        now_epoch_seconds: u64,
    ) -> Result<ChatGptAccountId, DirectError> {
        if token.len() > MAX_JWT_BYTES || expected_nonce.is_empty() {
            return Err(DirectError::Jwt(
                "token or expected nonce is invalid".to_owned(),
            ));
        }
        let mut segments = token.split('.');
        let header_segment = segments.next().unwrap_or_default();
        let payload_segment = segments.next().unwrap_or_default();
        let signature_segment = segments.next().unwrap_or_default();
        if header_segment.is_empty()
            || payload_segment.is_empty()
            || signature_segment.is_empty()
            || segments.next().is_some()
        {
            return Err(DirectError::Jwt("malformed compact JWT".to_owned()));
        }

        let header_bytes = decode_segment(header_segment)?;
        let header: JwtHeader = serde_json::from_slice(&header_bytes)
            .map_err(|_| DirectError::Jwt("invalid JWT header".to_owned()))?;
        if header.alg != "RS256" || header.kid.is_empty() {
            return Err(DirectError::Jwt(
                "only keyed RS256 ID tokens are accepted".to_owned(),
            ));
        }
        let matching: Vec<_> = self
            .jwks
            .keys
            .iter()
            .filter(|key| key.kid == header.kid)
            .collect();
        if matching.len() != 1 {
            return Err(DirectError::Jwt(
                "JWT kid did not select exactly one JWK".to_owned(),
            ));
        }
        let key = matching[0];
        if key.kty != "RSA"
            || key.usage.as_deref().is_some_and(|usage| usage != "sig")
            || key.alg.as_deref().is_some_and(|alg| alg != "RS256")
        {
            return Err(DirectError::Jwt(
                "selected JWK is not an RS256 signing key".to_owned(),
            ));
        }
        let modulus = decode_segment(&key.n)?;
        let exponent = decode_segment(&key.e)?;
        let signature = decode_segment(signature_segment)?;
        let signing_input = format!("{header_segment}.{payload_segment}");
        RsaPublicKeyComponents {
            n: modulus.as_slice(),
            e: exponent.as_slice(),
        }
        .verify(
            &RSA_PKCS1_2048_8192_SHA256,
            signing_input.as_bytes(),
            &signature,
        )
        .map_err(|_| DirectError::Jwt("JWT signature verification failed".to_owned()))?;

        // Claims are parsed only after cryptographic verification.
        let payload = decode_segment(payload_segment)?;
        let claims: IdTokenClaims = serde_json::from_slice(&payload)
            .map_err(|_| DirectError::Jwt("invalid verified JWT claims".to_owned()))?;
        if claims.iss != self.issuer {
            return Err(DirectError::Jwt("issuer mismatch".to_owned()));
        }
        validate_audience(&claims, &self.audience)?;
        if claims.exp <= now_epoch_seconds {
            return Err(DirectError::Jwt("ID token expired".to_owned()));
        }
        if claims
            .nbf
            .is_some_and(|not_before| not_before > now_epoch_seconds)
            || claims
                .iat
                .is_some_and(|issued_at| issued_at > now_epoch_seconds.saturating_add(60))
        {
            return Err(DirectError::Jwt("ID token is not yet valid".to_owned()));
        }
        if !secure_equal(claims.nonce.as_bytes(), expected_nonce.as_bytes()) {
            return Err(DirectError::Jwt("nonce mismatch".to_owned()));
        }

        let namespaced = claims
            .namespaced_auth
            .and_then(|claims| claims.chatgpt_account_id);
        let account_id = match (claims.chatgpt_account_id, namespaced) {
            (Some(first), Some(second)) if first != second => {
                return Err(DirectError::Jwt(
                    "conflicting ChatGPT account identifier claims".to_owned(),
                ));
            }
            (Some(value), _) | (None, Some(value)) => value,
            (None, None) => {
                return Err(DirectError::Jwt(
                    "verified ID token omitted ChatGPT account identifier".to_owned(),
                ));
            }
        };
        ChatGptAccountId::parse(account_id)
    }
}

fn decode_segment(segment: &str) -> Result<Vec<u8>, DirectError> {
    URL_SAFE_NO_PAD
        .decode(segment)
        .map_err(|_| DirectError::Jwt("invalid base64url in JWT or JWK".to_owned()))
}

fn validate_audience(claims: &IdTokenClaims, expected: &str) -> Result<(), DirectError> {
    match &claims.aud {
        Audience::One(value) if value == expected => Ok(()),
        Audience::Many(values) if values.iter().any(|value| value == expected) => {
            if values.len() > 1 && claims.azp.as_deref() != Some(expected) {
                return Err(DirectError::Jwt(
                    "multi-audience token omitted matching azp".to_owned(),
                ));
            }
            Ok(())
        }
        _ => Err(DirectError::Jwt("audience mismatch".to_owned())),
    }
}

/// Shared RSA signing fixture for the JWT unit tests and the OAuth e2e
/// tests in `auth.rs` (8-22): one key pair, its published JWKS, and a signer.
#[cfg(test)]
pub(crate) mod test_support {
    use base64::Engine;
    use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
    use ring::rand::SystemRandom;
    use ring::rsa::PublicKeyComponents;
    use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
    use serde_json::json;

    use super::JsonWebKeySet;

    const PRIVATE_KEY: &str = "MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDIp4UApaJQ247TbIW43Pg8S+GVMRT6qsdhbg6iSSL6a3qwH4VYLIFcw73rXtRnYrxTasyqi3JwWwDO8xay7FCPuWlyQbnjQjhBnMz3M57riwYhR69PWTL2E9m8CucL9tVtRDLoPhN2dYdTG/qd1WUxdBJEvnXovJImufpEtLihATWNfou3XQxySk8R7Od3diY/rv55YS6x1xZG536JgoZr4UAOr8NYDTE5tBqqc4AYc3LyLjW9VbKISWFlyIHtFU1YESRcUtVswJ1JFtTypQvPWuCiY39M+mv52q/BE9uoODtt19pt2Nsi2FEKjTEVmDMIkJoaAzJReqVeiW4VQkmzAgMBAAECggEAI6TukZDa5rY6BwDOOGq4hi2Moy4W5fiUdpBQdS+80PNq1gKjc2hkipATGs67uKnnfoIIXXtsFt1zpU+1ho9IOF/dhXh7hw1qZO1v07IN1xXZPuw3DkdwMBqSoT7mkE+G1mQ5DtyIJJD4OyFLQeJ4mXJfFGspEvD8nXiIJtBbw+3cMzbUJRYwTWfTxIHfkq7uuXUs1zn3hGm1Ku3WIQo/e3+y1eiecSTqJqrGGWLtZjB6689c59RI0leT6jM4tizOIQ3BkUXAetn/HRFbKZRcNFhh0e7+G6QIVTFX/wXHbLZsJWkPzHxNX2USoWqgpnmgiGZSGTbAt/CJ492NeX0K8QKBgQD4W6jcKVAjlu6SKrhVlhO8RdjYs4IC+Mi4/1eyhvCtgtPhrHxWb/5zHPrlYZrt3E5rdhvcshNkcOM9cS1MxwPCnJshs+eWnjXwkl+tWy3ceroc21xAhu9XHrPqNLuyX04YHV/B0Rg23aC+/C8aQmikq30yeLxFpTiz0jQdSDgnFwKBgQDO1BfuiMQBoDRDYfUx3NfwJXcw5AX81U625OU5aOZc5WBC3I/F4W5S5r3D3CbsiunD+JGxxEuR+xFjSinxQkT9hQ/Vjp5PX53wJ1WmGQmM/VyBlSN6htfCR/Y8ra9nuUiV1qphlTrckdy1wY2VreK/RG3QZcFRlrlv+mFWGXaTxQKBgQC/yYCHq3uQUDChPU4mAYPyAvomtdBzXQ0cF0rwuVXIl9vpTNqjoU6cNEfntM0AW/1O7OEtN3LUQHyq6Ogzfwf/VBJUH2p6nGhJA6/Q3jV3Kmrod9kwl0LiQvpqpRhA8WoMIzrcIA0T6WgFtBbnr1rBtxAyVpwFKEa2TmAiMK/0NwKBgDW7gCQmP9W0Sx+eWVcE6symzxpSgwO2XubA/JQ3nnFP3fxA1NExybmb3Hz/utUFGcohz6gBOSjJszC6Wb8l2kqKwRxYGuTAEIYNkgC+zG5mfBvmJPt2AKOmkmAdN06ZIjRbOpRzcoFPG6nUiPXz4M6T9nuHk/ugTri6sYLuxpGJAoGBALI0mlazlyncjdZYq8GNnN8HaQu6uMahky1cgJjnN5LSq8jC03gEhHwyPlFSmjKVXD0En2YyQC5dEZAtFde76EJMAqtU3ZbEDADY/0H1ajcguEPUXBtey/xQ2y5tWgsXtaF0PeIfamGlgC2pAnH72m5MbRKuM5IiUql/qXNlOreq";

    /// The RSA pair plus the JWKS publishing its public half (kid `fixture`).
    ///
    /// `jwks_json` is the exact object `jwks` decodes from, so loopback
    /// servers can serve it without a Serialize impl on the DTO.
    pub(crate) struct RsaFixture {
        pub(crate) pair: RsaKeyPair,
        pub(crate) jwks: JsonWebKeySet,
        pub(crate) jwks_json: serde_json::Value,
    }

    pub(crate) fn rsa_fixture() -> Result<RsaFixture, Box<dyn std::error::Error + Send + Sync>> {
        let private = STANDARD.decode(PRIVATE_KEY)?;
        let pair = RsaKeyPair::from_pkcs8(&private)
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        let components = PublicKeyComponents::<Vec<u8>>::from(pair.public());
        let jwks_json = json!({"keys":[{
            "kty":"RSA", "kid":"fixture", "use":"sig", "alg":"RS256",
            "n": URL_SAFE_NO_PAD.encode(&components.n),
            "e": URL_SAFE_NO_PAD.encode(&components.e)
        }]});
        let jwks = serde_json::from_value(jwks_json.clone())?;
        Ok(RsaFixture {
            pair,
            jwks,
            jwks_json,
        })
    }

    fn sign(
        pair: &RsaKeyPair,
        header: String,
        claims: serde_json::Value,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims)?);
        let signing_input = format!("{header}.{payload}");
        let mut signature = vec![0_u8; pair.public().modulus_len()];
        pair.sign(
            &RSA_PKCS1_SHA256,
            &SystemRandom::new(),
            signing_input.as_bytes(),
            &mut signature,
        )
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        Ok(format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signature)
        ))
    }

    /// Sign `claims` with the pinned RS256 `fixture` kid header.
    pub(crate) fn token(
        pair: &RsaKeyPair,
        claims: serde_json::Value,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let header = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({
            "alg":"RS256", "kid":"fixture", "typ":"JWT"
        }))?);
        sign(pair, header, claims)
    }

    /// Sign `claims` under an attacker-chosen `alg` header value; only the
    /// header differs, so the signature bytes stay a valid RSA signature.
    pub(crate) fn token_with_alg(
        pair: &RsaKeyPair,
        alg: &str,
        claims: serde_json::Value,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let header = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({
            "alg": alg, "kid": "fixture", "typ": "JWT"
        }))?);
        sign(pair, header, claims)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::OidcVerifier;
    use super::test_support::{rsa_fixture, token, token_with_alg};

    #[test]
    fn verifies_signature_issuer_audience_expiry_and_nonce()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let fixture = rsa_fixture()?;
        let verifier = OidcVerifier::new("https://issuer.test", "client-test", fixture.jwks)?;
        let claims = json!({
            "iss":"https://issuer.test", "aud":"client-test", "exp":2000,
            "iat":900, "nonce":"nonce-test", "chatgpt_account_id":"acct-123"
        });
        let token = token(&fixture.pair, claims)?;
        assert_eq!(
            verifier.verify(&token, "nonce-test", 1000)?.as_str(),
            "acct-123"
        );
        assert!(verifier.verify(&token, "wrong", 1000).is_err());
        assert!(verifier.verify(&token, "nonce-test", 2000).is_err());

        let mut tampered = token.into_bytes();
        let last = tampered.len().saturating_sub(1);
        if let Some(byte) = tampered.get_mut(last) {
            *byte = if *byte == b'A' { b'B' } else { b'A' };
        }
        let tampered = String::from_utf8(tampered)?;
        assert!(verifier.verify(&tampered, "nonce-test", 1000).is_err());
        Ok(())
    }

    /// 8-22: a multi-audience `aud` array is accepted only when the matching
    /// `azp` is present; a single-element array needs no `azp`, while a
    /// missing or mismatched `azp` and an absent audience are rejected.
    #[test]
    fn multi_audience_tokens_require_a_matching_azp()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let fixture = rsa_fixture()?;
        let verifier = OidcVerifier::new("https://issuer.test", "client-test", fixture.jwks)?;
        let signed = |aud: serde_json::Value, azp: Option<&str>| {
            let mut claims = json!({
                "iss":"https://issuer.test", "aud": aud, "exp":2000,
                "iat":900, "nonce":"nonce-test", "chatgpt_account_id":"acct-123"
            });
            if let Some(azp) = azp {
                claims["azp"] = json!(azp);
            }
            token(&fixture.pair, claims)
        };

        let with_azp = signed(json!(["client-test", "other-client"]), Some("client-test"))?;
        assert_eq!(
            verifier.verify(&with_azp, "nonce-test", 1000)?.as_str(),
            "acct-123"
        );
        let single_no_azp = signed(json!(["client-test"]), None)?;
        assert!(verifier.verify(&single_no_azp, "nonce-test", 1000).is_ok());

        let missing_azp = signed(json!(["client-test", "other-client"]), None)?;
        assert!(matches!(
            verifier.verify(&missing_azp, "nonce-test", 1000),
            Err(super::DirectError::Jwt(ref message))
                if message.contains("multi-audience token omitted matching azp")
        ));
        let wrong_azp = signed(json!(["client-test", "other-client"]), Some("other-client"))?;
        assert!(verifier.verify(&wrong_azp, "nonce-test", 1000).is_err());

        let absent_audience = signed(json!(["other-client"]), None)?;
        assert!(
            verifier
                .verify(&absent_audience, "nonce-test", 1000)
                .is_err()
        );
        Ok(())
    }

    /// 8-22: the namespaced `https://api.openai.com/auth` claim path carries
    /// the account identifier, equal duplicates are accepted, and a
    /// conflicting or entirely absent pair is rejected.
    #[test]
    fn namespaced_and_conflicting_account_claims()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let fixture = rsa_fixture()?;
        let verifier = OidcVerifier::new("https://issuer.test", "client-test", fixture.jwks)?;
        let signed = |claims: serde_json::Value| token(&fixture.pair, claims);
        let base = json!({
            "iss":"https://issuer.test", "aud":"client-test", "exp":2000,
            "iat":900, "nonce":"nonce-test"
        });
        let mut claims = base.clone();
        claims["https://api.openai.com/auth"] = json!({"chatgpt_account_id": "acct-namespaced"});
        assert_eq!(
            verifier
                .verify(&signed(claims)?, "nonce-test", 1000)?
                .as_str(),
            "acct-namespaced"
        );

        let mut agreeing = base.clone();
        agreeing["chatgpt_account_id"] = json!("acct-agree");
        agreeing["https://api.openai.com/auth"] = json!({"chatgpt_account_id": "acct-agree"});
        assert_eq!(
            verifier
                .verify(&signed(agreeing)?, "nonce-test", 1000)?
                .as_str(),
            "acct-agree"
        );

        let mut conflicting = base.clone();
        conflicting["chatgpt_account_id"] = json!("acct-top");
        conflicting["https://api.openai.com/auth"] =
            json!({"chatgpt_account_id": "acct-namespaced"});
        assert!(matches!(
            verifier.verify(&signed(conflicting)?, "nonce-test", 1000),
            Err(super::DirectError::Jwt(ref message))
                if message.contains("conflicting ChatGPT account identifier claims")
        ));

        assert!(matches!(
            verifier.verify(&signed(base)?, "nonce-test", 1000),
            Err(super::DirectError::Jwt(ref message))
                if message.contains("omitted ChatGPT account identifier")
        ));
        Ok(())
    }

    /// 8-22: a future `nbf` (and an `iat` beyond the 60-second clock skew)
    /// marks the token not yet valid even though its `exp` is in the future.
    #[test]
    fn future_nbf_and_far_future_iat_are_not_yet_valid()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let fixture = rsa_fixture()?;
        let verifier = OidcVerifier::new("https://issuer.test", "client-test", fixture.jwks)?;
        let signed = |nbf: Option<u64>, iat: Option<u64>| {
            let mut claims = json!({
                "iss":"https://issuer.test", "aud":"client-test", "exp":2000,
                "iat":900, "nonce":"nonce-test", "chatgpt_account_id":"acct-123"
            });
            if let Some(nbf) = nbf {
                claims["nbf"] = json!(nbf);
            }
            if let Some(iat) = iat {
                claims["iat"] = json!(iat);
            }
            token(&fixture.pair, claims)
        };

        let future_nbf = signed(Some(1500), None)?;
        assert!(matches!(
            verifier.verify(&future_nbf, "nonce-test", 1000),
            Err(super::DirectError::Jwt(ref message))
                if message.contains("ID token is not yet valid")
        ));
        let current_nbf = signed(Some(1000), None)?;
        verifier
            .verify(&current_nbf, "nonce-test", 1000)
            .expect("nbf at or before now is valid");

        let future_iat = signed(None, Some(1200))?;
        assert!(matches!(
            verifier.verify(&future_iat, "nonce-test", 1000),
            Err(super::DirectError::Jwt(ref message))
                if message.contains("ID token is not yet valid")
        ));
        // Within the 60-second skew an `iat` slightly ahead is tolerated.
        let skewed_iat = signed(None, Some(1030))?;
        verifier
            .verify(&skewed_iat, "nonce-test", 1000)
            .expect("iat within the clock skew is valid");
        Ok(())
    }

    /// 8-22: only a keyed RS256 header is accepted — the classic `alg: none`
    /// and an HMAC confusion attempt are rejected before any key lookup or
    /// claim parsing, even though the signature bytes verify under RSA.
    #[test]
    fn non_rs256_headers_are_rejected() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let fixture = rsa_fixture()?;
        let verifier = OidcVerifier::new("https://issuer.test", "client-test", fixture.jwks)?;
        let claims = json!({
            "iss":"https://issuer.test", "aud":"client-test", "exp":2000,
            "iat":900, "nonce":"nonce-test", "chatgpt_account_id":"acct-123"
        });
        for alg in ["none", "HS256", "RS512"] {
            let forged = token_with_alg(&fixture.pair, alg, claims.clone())?;
            assert!(
                matches!(
                    verifier.verify(&forged, "nonce-test", 1000),
                    Err(super::DirectError::Jwt(ref message))
                        if message.contains("only keyed RS256 ID tokens are accepted")
                ),
                "the {alg} header must be rejected before verification"
            );
        }
        Ok(())
    }

    /// 8-22: a `kid` that selects more than one JWKS entry fails — the
    /// verifier never guesses which key signed the token.
    #[test]
    fn kid_must_select_exactly_one_jwks_entry()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let fixture = rsa_fixture()?;
        let entry = &fixture.jwks_json["keys"][0];
        let jwks: super::JsonWebKeySet = serde_json::from_value(json!({
            "keys": [entry.clone(), entry.clone()]
        }))?;
        let verifier = OidcVerifier::new("https://issuer.test", "client-test", jwks)?;
        let claims = json!({
            "iss":"https://issuer.test", "aud":"client-test", "exp":2000,
            "iat":900, "nonce":"nonce-test", "chatgpt_account_id":"acct-123"
        });
        let signed = token(&fixture.pair, claims)?;
        assert!(matches!(
            verifier.verify(&signed, "nonce-test", 1000),
            Err(super::DirectError::Jwt(ref message))
                if message.contains("JWT kid did not select exactly one JWK")
        ));
        Ok(())
    }
}
