//! Scalar wire types with lossless string and JSON representations.

use std::{
    borrow::{Borrow, Cow},
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};

/// Defines an enum whose unknown wire strings are preserved exactly.
///
/// The generated enum is non-exhaustive and always contains an
/// `Unknown(Box<str>)` variant. Known values use ordinary Rust variants, while
/// [`from_raw`](#method.from_raw) is the explicit forward-compatibility escape
/// hatch for request construction.
///
/// ```ignore
/// open_string_enum! {
///     pub enum ResponseStatus {
///         Completed = "completed",
///         InProgress = "in_progress",
///     }
/// }
/// ```
#[macro_export]
macro_rules! open_string_enum {
    (
        $(#[$enum_meta:meta])*
        $visibility:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident = $wire_value:literal
            ),* $(,)?
        }
    ) => {
        $(#[$enum_meta])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[non_exhaustive]
        $visibility enum $name {
            $(
                $(#[$variant_meta])*
                $variant,
            )*
            /// A value added by the service after this crate was released.
            Unknown(::std::boxed::Box<str>),
        }

        impl $name {
            /// Parses a wire value while retaining unknown strings verbatim.
            #[must_use]
            pub fn from_raw(value: impl ::std::convert::Into<::std::boxed::Box<str>>) -> Self {
                let value = value.into();
                match value.as_ref() {
                    $($wire_value => Self::$variant,)*
                    _ => Self::Unknown(value),
                }
            }

            /// Returns the exact string used on the wire.
            #[must_use]
            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $wire_value,)*
                    Self::Unknown(value) => value,
                }
            }

            /// Returns whether this crate knows the wire value.
            #[must_use]
            pub const fn is_known(&self) -> bool {
                !matches!(self, Self::Unknown(_))
            }

            /// Returns the raw value only when it is unknown to this crate.
            #[must_use]
            pub fn unknown_value(&self) -> ::std::option::Option<&str> {
                match self {
                    Self::Unknown(value) => ::std::option::Option::Some(value),
                    _ => ::std::option::Option::None,
                }
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S>(
                &self,
                serializer: S,
            ) -> ::std::result::Result<S::Ok, S::Error>
            where
                S: ::serde::Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D>(
                deserializer: D,
            ) -> ::std::result::Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                <::std::boxed::Box<str> as ::serde::Deserialize<'de>>::deserialize(deserializer)
                    .map(Self::from_raw)
            }
        }

        impl ::std::convert::AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        // `Borrow<str>` is deliberately not implemented. The derived
        // `Hash` of a unit variant writes only the variant discriminant -
        // the wire string never participates - and the derived `Ord` sorts
        // by discriminant first, so the owned ordering can contradict the
        // borrowed string ordering. `Borrow`'s contract that borrowed and
        // owned values stay `Eq`/`Ord`/`Hash`-equivalent would not hold, and
        // `HashMap<$name, _>::get("wire-string")` would compile while the
        // derived hash probes a different bucket - a silent miss.
        // `AsRef<str>` above carries no equivalence requirement and is the
        // supported way to reach the wire string. `opaque_string_id!`
        // structs keep their `Borrow<str>` because a transparent newtype
        // hashes and orders exactly like the `str` it wraps.

        impl ::std::fmt::Display for $name {
            fn fmt(
                &self,
                formatter: &mut ::std::fmt::Formatter<'_>,
            ) -> ::std::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = ::std::convert::Infallible;

            fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
                ::std::result::Result::Ok(Self::from_raw(value))
            }
        }

        impl ::std::convert::From<::std::string::String> for $name {
            fn from(value: ::std::string::String) -> Self {
                Self::from_raw(value)
            }
        }

        impl ::std::convert::From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::from_raw(value)
            }
        }
    };
}

/// Defines an opaque, transparent string ID newtype.
///
/// No prefix or length validation is generated because OpenAI IDs are opaque
/// and their formats can change independently of this crate.
#[macro_export]
macro_rules! opaque_string_id {
    (
        $(#[$type_meta:meta])*
        $visibility:vis struct $name:ident;
    ) => {
        $(#[$type_meta])*
        #[derive(
            Clone,
            Debug,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        #[serde(transparent)]
        $visibility struct $name(::std::boxed::Box<str>);

        impl $name {
            /// Creates an ID without imposing assumptions on its format.
            #[must_use]
            pub fn new(value: impl ::std::convert::Into<::std::boxed::Box<str>>) -> Self {
                Self(value.into())
            }

            /// Borrows the opaque wire value.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consumes the ID and returns its wire value.
            #[must_use]
            pub fn into_boxed_str(self) -> ::std::boxed::Box<str> {
                self.0
            }
        }

        impl ::std::convert::AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl ::std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.as_str()
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(
                &self,
                formatter: &mut ::std::fmt::Formatter<'_>,
            ) -> ::std::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl ::std::convert::From<::std::string::String> for $name {
            fn from(value: ::std::string::String) -> Self {
                Self::new(value)
            }
        }

        impl ::std::convert::From<::std::boxed::Box<str>> for $name {
            fn from(value: ::std::boxed::Box<str>) -> Self {
                Self::new(value)
            }
        }

        impl ::std::convert::From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = ::std::convert::Infallible;

            fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
                ::std::result::Result::Ok(Self::new(value))
            }
        }
    };
}

opaque_string_id! {
    /// Opaque identifier of a Responses API response.
    pub struct ResponseId;
}

opaque_string_id! {
    /// Opaque identifier of an uploaded file.
    pub struct FileId;
}

opaque_string_id! {
    /// Opaque identifier of a batch.
    pub struct BatchId;
}

opaque_string_id! {
    /// Opaque identifier of a multipart upload.
    pub struct UploadId;
}

opaque_string_id! {
    /// Opaque identifier of a vector store.
    pub struct VectorStoreId;
}

opaque_string_id! {
    /// Opaque identifier of a fine-tuning job.
    pub struct FineTuningJobId;
}

/// An open model identifier.
///
/// Constants are conveniences rather than a closed capability list. Custom,
/// fine-tuned, and future model names can always be constructed with
/// [`ModelId::new`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ModelId(Cow<'static, str>);

impl ModelId {
    /// GPT-5.6 Sol snapshot.
    pub const GPT_5_6_SOL: Self = Self(Cow::Borrowed("gpt-5.6-sol"));

    /// GPT-5.6 Terra snapshot.
    pub const GPT_5_6_TERRA: Self = Self(Cow::Borrowed("gpt-5.6-terra"));

    /// GPT-5.6 Luna snapshot.
    pub const GPT_5_6_LUNA: Self = Self(Cow::Borrowed("gpt-5.6-luna"));

    /// GPT-5.6 Cyber snapshot (Responses-only availability).
    pub const GPT_5_6_CYBER: Self = Self(Cow::Borrowed("gpt-5.6-cyber"));

    /// Creates an open model ID from a static or owned string.
    #[must_use]
    pub fn new(value: impl Into<Cow<'static, str>>) -> Self {
        Self(value.into())
    }

    /// Creates a model ID without allocation from a static string.
    #[must_use]
    pub const fn from_static(value: &'static str) -> Self {
        Self(Cow::Borrowed(value))
    }

    /// Borrows the exact model name used on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }

    /// Consumes the model ID and returns an owned string.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0.into_owned()
    }
}

impl fmt::Debug for ModelId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ModelId")
            .field(&self.as_str())
            .finish()
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl AsRef<str> for ModelId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for ModelId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl From<String> for ModelId {
    fn from(value: String) -> Self {
        Self(Cow::Owned(value))
    }
}

impl From<&'static str> for ModelId {
    fn from(value: &'static str) -> Self {
        Self(Cow::Borrowed(value))
    }
}

impl std::str::FromStr for ModelId {
    type Err = std::convert::Infallible;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self::from(value.to_owned()))
    }
}

impl Serialize for ModelId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ModelId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self::from)
    }
}

/// JSON encoded inside a JSON string field.
///
/// The raw string is retained during wire deserialization, so streaming or
/// otherwise incomplete function arguments can still be represented. Parsing
/// into `T` is explicit via [`JsonText::parse`]. Construction through
/// [`JsonText::from_serializable`] performs the inner JSON encoding
/// automatically.
pub struct JsonText<T = serde_json::Value> {
    raw: Box<str>,
    marker: PhantomData<fn() -> T>,
}

impl<T> JsonText<T> {
    /// Retains a raw JSON string without validating its inner contents.
    #[must_use]
    pub fn from_raw(raw: impl Into<Box<str>>) -> Self {
        Self {
            raw: raw.into(),
            marker: PhantomData,
        }
    }

    /// Serializes a typed value into the inner JSON text.
    pub fn from_serializable(value: &T) -> serde_json::Result<Self>
    where
        T: Serialize,
    {
        serde_json::to_string(value).map(Self::from_raw)
    }

    /// Borrows the exact string carried by the outer JSON value.
    #[must_use]
    pub fn as_raw(&self) -> &str {
        &self.raw
    }

    /// Borrows the exact string carried by the outer JSON value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Consumes the wrapper and returns its unparsed string.
    #[must_use]
    pub fn into_raw(self) -> Box<str> {
        self.raw
    }

    /// Parses the retained JSON text as its declared type.
    pub fn parse(&self) -> serde_json::Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_str(&self.raw)
    }

    /// Parses the retained JSON text as a different type.
    pub fn deserialize_as<U>(&self) -> serde_json::Result<U>
    where
        U: DeserializeOwned,
    {
        serde_json::from_str(&self.raw)
    }

    /// Changes only the compile-time parse target, retaining the raw text.
    #[must_use]
    pub fn cast<U>(self) -> JsonText<U> {
        JsonText::from_raw(self.raw)
    }
}

impl<T> Clone for JsonText<T> {
    fn clone(&self) -> Self {
        Self::from_raw(self.raw.clone())
    }
}

impl<T> fmt::Debug for JsonText<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("JsonText").field(&self.raw).finish()
    }
}

impl<T> PartialEq for JsonText<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<T> Eq for JsonText<T> {}

impl<T> PartialOrd for JsonText<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for JsonText<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.raw.cmp(&other.raw)
    }
}

impl<T> Hash for JsonText<T> {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        self.raw.hash(state);
    }
}

impl<T> From<String> for JsonText<T> {
    fn from(value: String) -> Self {
        Self::from_raw(value)
    }
}

impl<T> From<&str> for JsonText<T> {
    fn from(value: &str) -> Self {
        Self::from_raw(value)
    }
}

impl<T> From<Box<str>> for JsonText<T> {
    fn from(value: Box<str>) -> Self {
        Self::from_raw(value)
    }
}

impl<T> AsRef<str> for JsonText<T> {
    fn as_ref(&self) -> &str {
        self.as_raw()
    }
}

impl<T> Serialize for JsonText<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.raw)
    }
}

impl<'de, T> Deserialize<'de> for JsonText<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Box::<str>::deserialize(deserializer).map(Self::from_raw)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::hash::Hasher;

    use proptest::prelude::*;
    use serde::{Deserialize, Serialize, de::DeserializeOwned};
    use serde_json::json;
    use static_assertions::assert_impl_all;

    use super::{BatchId, FileId, JsonText, ModelId, ResponseId};

    crate::open_string_enum! {
        pub enum TestStatus {
            Completed = "completed",
            InProgress = "in_progress",
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Arguments {
        city: String,
        units: String,
    }

    assert_impl_all!(TestStatus: Serialize, DeserializeOwned, Send, Sync, AsRef<str>);
    assert_impl_all!(ResponseId: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(FileId: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(BatchId: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ModelId: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(JsonText<Arguments>: Serialize, DeserializeOwned, Send, Sync);

    #[test]
    fn open_enum_preserves_unknown_wire_values() {
        let status: TestStatus =
            serde_json::from_str(r#""future_status.v2""#).expect("decode unknown status");

        assert!(!status.is_known());
        assert_eq!(status.unknown_value(), Some("future_status.v2"));
        assert_eq!(
            serde_json::to_string(&status).expect("encode unknown status"),
            r#""future_status.v2""#
        );
    }

    #[test]
    fn open_enum_uses_known_variants() {
        let status: TestStatus =
            serde_json::from_str(r#""in_progress""#).expect("decode known status");

        assert_eq!(status, TestStatus::InProgress);
        assert_eq!(status.as_str(), "in_progress");
        assert!(status.is_known());
    }

    /// Type-level check that open enums expose their wire string through
    /// `AsRef<str>` only - never through `Borrow<str>` - so maps keyed by an
    /// open enum cannot be probed with a bare `&str` at compile time.
    fn assert_wire_string_access_is_as_ref_only<T: AsRef<str>>() {}

    #[test]
    fn open_enum_borrow_is_as_ref_only_and_keys_stay_whole_values() {
        assert_wire_string_access_is_as_ref_only::<TestStatus>();

        // Known wire strings normalize onto their variant, so an enum-keyed
        // map already unifies `from_raw("completed")` with `Completed`.
        let mut counts = HashMap::new();
        counts.insert(TestStatus::Completed, 1);
        counts.insert(TestStatus::from_raw("completed"), 2);
        assert_eq!(counts.len(), 1);
        assert_eq!(counts.get(&TestStatus::Completed), Some(&2));
        assert_eq!(TestStatus::Completed.as_ref(), "completed");
        assert_eq!(TestStatus::from_raw("completed").as_ref(), "completed");

        // Why `Borrow<str>` is not implemented: the derived `Hash` of a unit
        // variant never touches the wire string, and the derived `Ord` sorts
        // by discriminant first, so owned and borrowed order disagree. With
        // `Borrow<str>`, `counts.get("completed")` would compile while the
        // probe hashes to a different bucket - a silent miss. Without it,
        // lookups stay keyed by the whole enum value.
        let owned = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(&TestStatus::Completed, &mut hasher);
            hasher.finish()
        };
        let borrowed = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(&"completed", &mut hasher);
            hasher.finish()
        };
        assert_ne!(owned, borrowed);
        assert!(TestStatus::InProgress < TestStatus::Unknown("a".into()));
        assert!("in_progress" > "a");
    }

    #[test]
    fn ids_are_transparent_and_do_not_validate_prefixes() {
        let id = ResponseId::new("entirely-new-id-format");
        assert_eq!(
            serde_json::to_string(&id).expect("encode ID"),
            r#""entirely-new-id-format""#
        );
        assert_eq!(
            serde_json::from_str::<ResponseId>(r#""future:opaque/42""#)
                .expect("decode opaque ID")
                .as_str(),
            "future:opaque/42"
        );
    }

    #[test]
    fn model_id_is_open_and_round_trips() {
        let model = ModelId::new(String::from("ft:gpt-future:org:custom"));
        let encoded = serde_json::to_string(&model).expect("encode model ID");
        let decoded = serde_json::from_str::<ModelId>(&encoded).expect("decode model ID");

        assert_eq!(decoded, model);
        assert_eq!(ModelId::GPT_5_6_SOL.as_str(), "gpt-5.6-sol");
        assert_eq!(ModelId::GPT_5_6_TERRA.as_str(), "gpt-5.6-terra");
        assert_eq!(ModelId::GPT_5_6_LUNA.as_str(), "gpt-5.6-luna");
        assert_eq!(ModelId::GPT_5_6_CYBER.as_str(), "gpt-5.6-cyber");
    }

    #[test]
    fn json_text_encodes_inner_value_without_manual_json() {
        let arguments = Arguments {
            city: String::from("上海"),
            units: String::from("metric"),
        };
        let text = JsonText::from_serializable(&arguments).expect("encode inner JSON");
        let outer = serde_json::to_string(&text).expect("encode outer JSON string");

        assert_eq!(
            serde_json::from_str::<String>(&outer).expect("decode outer string"),
            text.as_raw()
        );
        assert_eq!(text.parse().expect("parse typed arguments"), arguments);
    }

    #[test]
    fn json_text_wire_decode_is_lazy() {
        let text = serde_json::from_str::<JsonText<Arguments>>(r#""{\"city\":""#)
            .expect("incomplete inner JSON remains representable");

        assert_eq!(text.as_raw(), r#"{"city":"#);
        assert!(text.parse().is_err());
    }

    #[test]
    fn json_text_rejects_non_string_outer_values() {
        for invalid in ["null", "17", "true", r#"{"city":"Paris"}"#] {
            let error = serde_json::from_str::<JsonText<Arguments>>(invalid)
                .expect_err("outer JSON value must be a string");
            assert!(error.to_string().contains("string"));
        }
    }

    #[test]
    fn json_text_can_parse_an_alternate_target() {
        let text = JsonText::<Arguments>::from_raw(r#"{"answer":42}"#);
        let value = text
            .deserialize_as::<serde_json::Value>()
            .expect("parse alternate type");

        assert_eq!(value, json!({"answer": 42}));
    }

    proptest! {
        #[test]
        fn open_enum_strings_semantically_round_trip(value in ".{0,128}") {
            let status = TestStatus::from_raw(value.clone());
            let encoded = serde_json::to_vec(&status).expect("encode status");
            let decoded = serde_json::from_slice::<TestStatus>(&encoded).expect("decode status");
            prop_assert_eq!(decoded.as_str(), value);
        }

        #[test]
        fn typed_json_text_round_trips(values in proptest::collection::btree_map("[a-z]{1,12}", any::<i64>(), 0..24)) {
            let text = JsonText::from_serializable(&values).expect("encode typed JSON text");
            let outer = serde_json::to_vec(&text).expect("encode outer JSON");
            let decoded = serde_json::from_slice::<JsonText<BTreeMap<String, i64>>>(&outer)
                .expect("decode outer JSON");
            let parsed = decoded.parse().expect("parse inner JSON");
            prop_assert_eq!(parsed, values);
        }
    }
}
