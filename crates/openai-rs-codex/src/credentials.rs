use std::fmt;

use tokio::process::Command;

mod private {
    use tokio::process::Command;

    pub trait Sealed {
        fn apply_to_child(&self, command: &mut Command);
    }
}

/// Marker implemented only by credential modes accepted by the Codex
/// app-server backend.
///
/// This intentionally does not implement, extend, or convert into a Platform
/// API credential trait. The sealed boundary prevents downstream crates from
/// smuggling an arbitrary bearer credential into this transport.
pub trait CodexCredentialMarker: private::Sealed + Send + Sync + 'static {}

/// Authentication is managed entirely by the dedicated app-server profile.
///
/// Use the typed browser or device-code login methods after spawning the
/// client, or point the process at a dedicated profile that was explicitly
/// pre-authenticated with the Codex CLI.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct ManagedAppServerCredential;

impl fmt::Debug for ManagedAppServerCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ManagedAppServerCredential")
    }
}

impl private::Sealed for ManagedAppServerCredential {
    fn apply_to_child(&self, command: &mut Command) {
        command.env_remove("CODEX_ACCESS_TOKEN");
    }
}

impl CodexCredentialMarker for ManagedAppServerCredential {}

/// A Codex workspace access token that is injected only into the owned child.
///
/// It is never sent through `account/login/start`, never exposed by this
/// crate's protocol DTOs, and cannot be used as a Platform API credential.
/// Managed login methods are also absent from an access-token client:
///
/// ```compile_fail
/// use openai_rs_codex::{AppServerClient, CodexAccessTokenCredential};
///
/// async fn cannot_switch_login(
///     client: &AppServerClient<CodexAccessTokenCredential>,
/// ) {
///     client.account_login_device().await;
/// }
/// ```
#[cfg(feature = "access-token")]
pub struct CodexAccessTokenCredential {
    token: secrecy::SecretString,
}

#[cfg(feature = "access-token")]
impl CodexAccessTokenCredential {
    /// Wrap an access token without making it serializable or displayable.
    #[must_use]
    pub fn new(token: secrecy::SecretString) -> Self {
        Self { token }
    }
}

#[cfg(feature = "access-token")]
impl fmt::Debug for CodexAccessTokenCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexAccessTokenCredential")
            .field("token", &"<redacted>")
            .finish()
    }
}

#[cfg(feature = "access-token")]
impl private::Sealed for CodexAccessTokenCredential {
    fn apply_to_child(&self, command: &mut Command) {
        use secrecy::ExposeSecret;

        command.env("CODEX_ACCESS_TOKEN", self.token.expose_secret());
    }
}

#[cfg(feature = "access-token")]
impl CodexCredentialMarker for CodexAccessTokenCredential {}

#[cfg(feature = "app-server")]
pub(crate) fn apply_credential<C: CodexCredentialMarker>(credential: &C, command: &mut Command) {
    credential.apply_to_child(command);
}

#[cfg(all(test, feature = "access-token"))]
mod tests {
    use super::{CodexAccessTokenCredential, CodexCredentialMarker};

    static_assertions::assert_not_impl_any!(
        CodexAccessTokenCredential: serde::Serialize, std::fmt::Display, Clone
    );

    fn assert_codex_marker<T: CodexCredentialMarker>() {}

    #[test]
    fn access_token_is_redacted_and_codex_only() {
        assert_codex_marker::<CodexAccessTokenCredential>();
        let credential =
            CodexAccessTokenCredential::new(secrecy::SecretString::from("unit-secret".to_owned()));
        let debug = format!("{credential:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("unit-secret"));
    }
}
