use std::collections::HashSet;

use crate::Error;

/// SHA-256 of the combined app-server protocol schema compiled into this
/// crate's DTO layer.
pub const COMPILED_APP_SERVER_SCHEMA_SHA256: &str =
    "95f68321313fc4d64c8781737abf60657d6d100e2f516a036253ca936f4d73a2";

pub const BUNDLED_CODEX_VERSION: &str = "0.144.5";
pub const BUNDLED_CODEX_TARGET: &str = "aarch64-apple-darwin";
pub const BUNDLED_CODEX_EXECUTABLE_SHA256: &str =
    "5e29ab10ca1171be158f7335dd6bd8ce1aaf9af1556939db36a5ee338be6f5f2";

/// Immutable identity of one audited Codex runtime and its frozen app-server
/// schema.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuntimeIdentity {
    released_version: String,
    executable_sha256: String,
    schema_sha256: String,
}

impl RuntimeIdentity {
    /// Construct an audited identity from lowercase or uppercase hex digests.
    ///
    /// Source builds reporting `0.0.0`, missing versions, malformed digests,
    /// and all-zero placeholder digests are rejected.
    pub fn new(
        released_version: impl Into<String>,
        executable_sha256: impl AsRef<str>,
        schema_sha256: impl AsRef<str>,
    ) -> Result<Self, Error> {
        let released_version = released_version.into();
        let released_version = released_version.trim();
        if !is_released_version(released_version) || released_version == "0.0.0" {
            return Err(Error::InvalidConfiguration(
                "runtime identity requires a released x.y.z, non-0.0.0 version".to_owned(),
            ));
        }
        let executable_sha256 = normalize_sha256(executable_sha256.as_ref()).ok_or_else(|| {
            Error::InvalidConfiguration(
                "runtime executable SHA-256 must be 64 non-zero hexadecimal characters".to_owned(),
            )
        })?;
        let schema_sha256 = normalize_sha256(schema_sha256.as_ref()).ok_or_else(|| {
            Error::InvalidConfiguration(
                "app-server schema SHA-256 must be 64 non-zero hexadecimal characters".to_owned(),
            )
        })?;
        if schema_sha256 != COMPILED_APP_SERVER_SCHEMA_SHA256 {
            return Err(Error::InvalidConfiguration(format!(
                "runtime schema SHA-256 {schema_sha256} does not match compiled protocol schema {COMPILED_APP_SERVER_SCHEMA_SHA256}"
            )));
        }

        Ok(Self {
            released_version: released_version.to_owned(),
            executable_sha256,
            schema_sha256,
        })
    }

    #[must_use]
    pub fn released_version(&self) -> &str {
        &self.released_version
    }

    #[must_use]
    pub fn executable_sha256(&self) -> &str {
        &self.executable_sha256
    }

    #[must_use]
    pub fn schema_sha256(&self) -> &str {
        &self.schema_sha256
    }
}

/// Exact allowlist mapping audited executable artifacts to their schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeCompatibility {
    identities: Vec<RuntimeIdentity>,
}

impl RuntimeCompatibility {
    /// Build a non-empty compatibility set. One executable digest may occur
    /// only once, so a matched artifact always selects one unambiguous schema.
    pub fn new<I>(identities: I) -> Result<Self, Error>
    where
        I: IntoIterator<Item = RuntimeIdentity>,
    {
        let identities: Vec<_> = identities.into_iter().collect();
        if identities.is_empty() {
            return Err(Error::InvalidConfiguration(
                "runtime compatibility set must not be empty".to_owned(),
            ));
        }
        let mut executable_hashes = HashSet::with_capacity(identities.len());
        for identity in &identities {
            if !executable_hashes.insert(identity.executable_sha256.clone()) {
                return Err(Error::InvalidConfiguration(
                    "runtime compatibility set contains a duplicate executable SHA-256".to_owned(),
                ));
            }
        }
        Ok(Self { identities })
    }

    #[must_use]
    pub fn identities(&self) -> &[RuntimeIdentity] {
        &self.identities
    }

    /// Return the exact runtime identity vendored for this compilation target.
    /// Unsupported targets fail explicitly instead of reusing another
    /// platform's same-version identity.
    pub fn for_current_target() -> Result<Self, Error> {
        #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
        {
            let identity = RuntimeIdentity::new(
                BUNDLED_CODEX_VERSION,
                BUNDLED_CODEX_EXECUTABLE_SHA256,
                COMPILED_APP_SERVER_SCHEMA_SHA256,
            )?;
            Self::new([identity])
        }

        #[cfg(not(all(target_arch = "aarch64", target_os = "macos")))]
        {
            Err(Error::UnsupportedRuntimeTarget {
                target: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
            })
        }
    }

    /// Alias for [`Self::for_current_target`].
    pub fn bundled() -> Result<Self, Error> {
        Self::for_current_target()
    }

    #[cfg(feature = "app-server")]
    pub(crate) fn resolve(&self, executable_sha256: &str) -> Option<&RuntimeIdentity> {
        self.identities
            .iter()
            .find(|identity| identity.executable_sha256 == executable_sha256)
    }
}

fn normalize_sha256(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.len() != 64
        || !trimmed.bytes().all(|byte| byte.is_ascii_hexdigit())
        || trimmed.bytes().all(|byte| byte == b'0')
    {
        return None;
    }
    Some(trimmed.to_ascii_lowercase())
}

fn is_released_version(version: &str) -> bool {
    let mut parts = version.split('.');
    let valid_part = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|byte| byte.is_ascii_digit())
            && (part == "0" || !part.starts_with('0'))
    };
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(major), Some(minor), Some(patch), None)
            if valid_part(major) && valid_part(minor) && valid_part(patch)
    )
}

#[cfg(test)]
mod tests {
    use super::{
        BUNDLED_CODEX_EXECUTABLE_SHA256, BUNDLED_CODEX_TARGET, BUNDLED_CODEX_VERSION,
        COMPILED_APP_SERVER_SCHEMA_SHA256, RuntimeCompatibility, RuntimeIdentity,
    };

    const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn rejects_unknown_or_placeholder_identity() {
        assert!(RuntimeIdentity::new("0.0.0", HASH_A, COMPILED_APP_SERVER_SCHEMA_SHA256).is_err());
        assert!(RuntimeIdentity::new("dev", HASH_A, COMPILED_APP_SERVER_SCHEMA_SHA256).is_err());
        assert!(RuntimeIdentity::new("1.2.3", "", COMPILED_APP_SERVER_SCHEMA_SHA256).is_err());
        assert!(
            RuntimeIdentity::new("1.2.3", "0".repeat(64), COMPILED_APP_SERVER_SCHEMA_SHA256)
                .is_err()
        );
        assert!(RuntimeCompatibility::new(Vec::new()).is_err());
    }

    #[test]
    fn rejects_schema_that_is_not_compiled_into_dtos() {
        assert!(RuntimeIdentity::new("1.2.3", HASH_A, HASH_B).is_err());
    }

    #[test]
    fn rejects_ambiguous_artifact_mapping() -> Result<(), crate::Error> {
        let first = RuntimeIdentity::new("1.2.3", HASH_A, COMPILED_APP_SERVER_SCHEMA_SHA256)?;
        let second = RuntimeIdentity::new("1.2.4", HASH_A, COMPILED_APP_SERVER_SCHEMA_SHA256)?;
        assert!(RuntimeCompatibility::new([first, second]).is_err());
        Ok(())
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    #[test]
    fn bundled_identity_matches_spec_manifest() -> Result<(), crate::Error> {
        let compatibility = RuntimeCompatibility::bundled()?;
        let identity = compatibility
            .identities()
            .first()
            .ok_or_else(|| crate::Error::InvalidConfiguration("missing bundled identity".into()))?;
        assert_eq!(identity.released_version(), BUNDLED_CODEX_VERSION);
        assert_eq!(
            identity.executable_sha256(),
            BUNDLED_CODEX_EXECUTABLE_SHA256
        );
        assert_eq!(identity.schema_sha256(), COMPILED_APP_SERVER_SCHEMA_SHA256);

        let manifest = include_str!("../../../spec/contracts/codex-compatibility.toml");
        let expected = [
            format!("version = \"{BUNDLED_CODEX_VERSION}\""),
            format!("target = \"{BUNDLED_CODEX_TARGET}\""),
            format!("executable_sha256 = \"{BUNDLED_CODEX_EXECUTABLE_SHA256}\""),
            format!("app_server_schema_sha256 = \"{COMPILED_APP_SERVER_SCHEMA_SHA256}\""),
        ];
        for field in expected {
            assert!(manifest.lines().any(|line| line.trim() == field));
        }
        Ok(())
    }
}
