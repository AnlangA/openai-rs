//! Serde primitives used by handwritten OpenAI wire types.
//!
//! OpenAPI distinguishes an absent property from a property whose value is
//! `null`. [`Omittable`] and [`Nullable`] deliberately model those two axes
//! separately so that `Omittable<Nullable<T>>` has all three wire states.
//! Contract projections in `spec/contracts/` audit these types; they do not
//! generate Rust.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::{Map, Value};
use thiserror::Error;

/// A property that can be absent from its containing JSON object.
///
/// Handwritten fields use this type together with
/// `#[serde(default, skip_serializing_if = "Omittable::is_omitted")]`.
/// Serializing [`Omittable::Omitted`] directly is an error because only the
/// containing object can omit a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Omittable<T> {
    /// The containing object must not emit the property.
    Omitted,
    /// The property is present and contains this value.
    Value(T),
}

impl<T> Omittable<T> {
    /// Returns `true` when the property must be omitted.
    #[must_use]
    pub const fn is_omitted(&self) -> bool {
        matches!(self, Self::Omitted)
    }

    /// Returns `true` when the property is present.
    #[must_use]
    pub const fn is_value(&self) -> bool {
        matches!(self, Self::Value(_))
    }

    /// Borrows the contained value without changing its presence state.
    #[must_use]
    pub const fn as_ref(&self) -> Omittable<&T> {
        match self {
            Self::Omitted => Omittable::Omitted,
            Self::Value(value) => Omittable::Value(value),
        }
    }

    /// Mutably borrows the contained value without changing its presence
    /// state.
    #[must_use]
    pub const fn as_mut(&mut self) -> Omittable<&mut T> {
        match self {
            Self::Omitted => Omittable::Omitted,
            Self::Value(value) => Omittable::Value(value),
        }
    }

    /// Converts the presence state to an [`Option`].
    #[must_use]
    pub fn into_option(self) -> Option<T> {
        match self {
            Self::Omitted => None,
            Self::Value(value) => Some(value),
        }
    }

    /// Maps a present value while preserving omission.
    #[must_use]
    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> Omittable<U> {
        match self {
            Self::Omitted => Omittable::Omitted,
            Self::Value(value) => Omittable::Value(map(value)),
        }
    }

    /// Converts `None` to omission and `Some` to a present value.
    #[must_use]
    pub fn from_option(value: Option<T>) -> Self {
        match value {
            Some(value) => Self::Value(value),
            None => Self::Omitted,
        }
    }
}

impl<T> Default for Omittable<T> {
    fn default() -> Self {
        Self::Omitted
    }
}

impl<T> From<T> for Omittable<T> {
    fn from(value: T) -> Self {
        Self::Value(value)
    }
}

impl<T> Serialize for Omittable<T>
where
    T: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Value(value) => value.serialize(serializer),
            Self::Omitted => Err(serde::ser::Error::custom(
                "Omittable::Omitted must be skipped by its containing field",
            )),
        }
    }
}

impl<'de, T> Deserialize<'de> for Omittable<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        T::deserialize(deserializer).map(Self::Value)
    }
}

/// A required property that may explicitly contain JSON `null`.
///
/// This type intentionally has no [`Default`] implementation: a missing
/// required-nullable property must remain a deserialization error.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Nullable<T> {
    /// The property is present with the JSON value `null`.
    Null,
    /// The property is present with a non-null value.
    Value(T),
}

impl<T> Nullable<T> {
    /// Returns `true` when the wire value is `null`.
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Returns `true` when the wire value is non-null.
    #[must_use]
    pub const fn is_value(&self) -> bool {
        matches!(self, Self::Value(_))
    }

    /// Borrows the contained value while preserving explicit null.
    #[must_use]
    pub const fn as_ref(&self) -> Nullable<&T> {
        match self {
            Self::Null => Nullable::Null,
            Self::Value(value) => Nullable::Value(value),
        }
    }

    /// Mutably borrows the contained value while preserving explicit null.
    #[must_use]
    pub const fn as_mut(&mut self) -> Nullable<&mut T> {
        match self {
            Self::Null => Nullable::Null,
            Self::Value(value) => Nullable::Value(value),
        }
    }

    /// Converts explicit null to `None` and a value to `Some`.
    #[must_use]
    pub fn into_option(self) -> Option<T> {
        match self {
            Self::Null => None,
            Self::Value(value) => Some(value),
        }
    }

    /// Maps a non-null value while preserving explicit null.
    #[must_use]
    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> Nullable<U> {
        match self {
            Self::Null => Nullable::Null,
            Self::Value(value) => Nullable::Value(map(value)),
        }
    }

    /// Converts `None` to explicit null and `Some` to a non-null value.
    #[must_use]
    pub fn from_option(value: Option<T>) -> Self {
        match value {
            Some(value) => Self::Value(value),
            None => Self::Null,
        }
    }
}

impl<T> From<T> for Nullable<T> {
    fn from(value: T) -> Self {
        Self::Value(value)
    }
}

impl<T> Serialize for Nullable<T>
where
    T: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Null => serializer.serialize_none(),
            Self::Value(value) => value.serialize(serializer),
        }
    }
}

impl<'de, T> Deserialize<'de> for Nullable<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        NullableRepr::<T>::deserialize(deserializer).map(|value| match value {
            NullableRepr::Null => Self::Null,
            NullableRepr::Value(value) => Self::Value(value),
        })
    }
}

// Unlike `Option<T>`, an untagged representation starts through
// `deserialize_any`. Serde's missing-field deserializer rejects that call,
// while a real JSON null is retained as the unit variant. This distinction is
// what makes required-nullable fields reject an absent key.
#[derive(Deserialize)]
#[serde(untagged)]
enum NullableRepr<T> {
    Null,
    Value(T),
}

/// Additional properties retained from a response object.
///
/// The map is intentionally read-only through the public API. Handwritten
/// serializers must call [`ExtraFields::ensure_no_reserved`] with their known
/// wire keys before flattening it into an object.
#[derive(Clone, Default, PartialEq)]
pub struct ExtraFields {
    fields: Map<String, Value>,
}

impl ExtraFields {
    /// Creates an empty set of additional properties.
    #[must_use]
    pub fn new() -> Self {
        Self { fields: Map::new() }
    }

    /// Builds additional properties after checking them against the known
    /// fields of their containing object.
    pub fn try_from_map<I, K>(
        fields: Map<String, Value>,
        reserved_keys: I,
    ) -> Result<Self, ExtraFieldsConflict>
    where
        I: IntoIterator<Item = K>,
        K: AsRef<str>,
    {
        let extra = Self { fields };
        extra.ensure_no_reserved(reserved_keys)?;
        Ok(extra)
    }

    /// Gets an additional property by wire key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.fields.get(key)
    }

    /// Returns whether an additional property is retained.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.fields.contains_key(key)
    }

    /// Iterates over retained keys and values without allowing mutation.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &Value)> {
        self.fields.iter().map(|(key, value)| (key.as_str(), value))
    }

    /// Iterates over retained wire keys.
    pub fn keys(&self) -> impl ExactSizeIterator<Item = &str> {
        self.fields.keys().map(String::as_str)
    }

    /// Iterates over retained values.
    pub fn values(&self) -> impl ExactSizeIterator<Item = &Value> {
        self.fields.values()
    }

    /// Returns the number of retained properties.
    #[must_use]
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Returns `true` when there are no additional properties.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Ensures that no extra property would collide with a known object key.
    pub fn ensure_no_reserved<I, K>(&self, reserved_keys: I) -> Result<(), ExtraFieldsConflict>
    where
        I: IntoIterator<Item = K>,
        K: AsRef<str>,
    {
        for key in reserved_keys {
            let key = key.as_ref();
            if self.fields.contains_key(key) {
                return Err(ExtraFieldsConflict {
                    key: key.to_owned().into_boxed_str(),
                });
            }
        }
        Ok(())
    }
}

impl fmt::Debug for ExtraFields {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtraFields")
            .field("len", &self.fields.len())
            .finish()
    }
}

impl Serialize for ExtraFields {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.fields.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExtraFields {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Map::<String, Value>::deserialize(deserializer).map(|fields| Self { fields })
    }
}

impl<'a> IntoIterator for &'a ExtraFields {
    type Item = (&'a str, &'a Value);
    type IntoIter = Box<dyn ExactSizeIterator<Item = Self::Item> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

/// An extra property has the same name as a known property of its object.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("extra field `{key}` collides with a known field")]
pub struct ExtraFieldsConflict {
    key: Box<str>,
}

impl ExtraFieldsConflict {
    /// Returns the colliding wire key.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }
}

/// Constant `type` field used by tagged request and response objects.
macro_rules! literal_tag {
    ($name:ident, $wire:literal) => {
        literal_tag!($name, Value, $wire);
    };
    ($name:ident, $variant:ident, $wire:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        enum $name {
            #[serde(rename = $wire)]
            $variant,
        }
    };
}

/// Forward-compatible tagged union that retains unknown objects verbatim.
///
/// A variant may list dated aliases after the primary discriminator:
/// `WebSearch(WebSearchTool) => "web_search" | "web_search_2025_08_26"`.
macro_rules! tagged_union {
    ($(#[$meta:meta])* pub enum $name:ident {
        $($variant:ident($ty:ty) => $wire:literal $(| $alias:literal)*),+ $(,)?
    }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq)]
        #[non_exhaustive]
        pub enum $name {
            $($variant($ty),)+
            /// A future variant retained as a complete semantic JSON object.
            Unknown($crate::UnknownTaggedObject),
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                match self {
                    $(Self::$variant(value) => value.serialize(serializer),)+
                    Self::Unknown(value) => value.serialize(serializer),
                }
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = Value::deserialize(deserializer)?;
                let discriminator =
                    $crate::kernel::object_discriminator(&value).map_err(D::Error::custom)?;
                match discriminator.as_str() {
                    $($wire $(| $alias)* => serde_json::from_value::<$ty>(value)
                        .map(Self::$variant)
                        .map_err(D::Error::custom),)+
                    _ => $crate::UnknownTaggedObject::from_value(value)
                        .map(Self::Unknown)
                        .map_err(D::Error::custom),
                }
            }
        }
    };
}

/// Tagged union that rejects known-but-invalid tags instead of keeping them.
macro_rules! tagged_union_reject_known {
    ($(#[$meta:meta])* pub enum $name:ident {
        $($variant:ident($ty:ty) => $tag:literal),+ $(,)?
    } reject [$($rejected:literal),+ $(,)?]) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq)]
        #[non_exhaustive]
        pub enum $name {
            $($variant($ty),)+
            /// A genuinely future source tag retained verbatim.
            Unknown($crate::UnknownTaggedObject),
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                match self {
                    $(Self::$variant(value) => value.serialize(serializer),)+
                    Self::Unknown(value) => value.serialize(serializer),
                }
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = Value::deserialize(deserializer)?;
                let tag = $crate::kernel::object_discriminator(&value).map_err(D::Error::custom)?;
                match tag.as_str() {
                    $($tag => serde_json::from_value(value)
                        .map(Self::$variant)
                        .map_err(D::Error::custom),)+
                    $($rejected => Err(D::Error::custom(format_args!(
                        "known source tag `{tag}` is not valid in {}",
                        stringify!($name),
                    ))),)+
                    _ => $crate::UnknownTaggedObject::from_value(value)
                        .map(Self::Unknown)
                        .map_err(D::Error::custom),
                }
            }
        }
    };
}

pub(crate) fn object_discriminator(value: &Value) -> Result<String, &'static str> {
    let object = value
        .as_object()
        .ok_or("tagged value must be a JSON object")?;
    object
        .get("type")
        .ok_or("tagged object is missing string field `type`")?
        .as_str()
        .map(str::to_owned)
        .ok_or("tagged object field `type` must be a string")
}

/// A future tagged object, including its discriminator and every raw field.
///
/// The map is immutable through the public API, so `discriminator` can never
/// drift from its retained `type` property.
#[derive(Clone, PartialEq)]
pub struct UnknownTaggedObject {
    discriminator: Box<str>,
    raw: Map<String, Value>,
}

impl UnknownTaggedObject {
    /// Validates and retains an unknown tagged JSON object.
    pub fn from_value(value: Value) -> Result<Self, UnknownTaggedObjectError> {
        let discriminator = object_discriminator(&value)
            .map_err(UnknownTaggedObjectError::Invalid)?
            .into_boxed_str();
        let Value::Object(raw) = value else {
            return Err(UnknownTaggedObjectError::Invalid(
                "tagged value must be a JSON object",
            ));
        };
        Ok(Self { discriminator, raw })
    }

    /// Returns the exact unknown discriminator.
    #[must_use]
    pub fn discriminator(&self) -> &str {
        &self.discriminator
    }

    /// Borrows all retained object fields, including `type`.
    #[must_use]
    pub const fn raw(&self) -> &Map<String, Value> {
        &self.raw
    }

    /// Converts this value back into its semantic JSON object.
    #[must_use]
    pub fn into_value(self) -> Value {
        Value::Object(self.raw)
    }
}

impl fmt::Debug for UnknownTaggedObject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UnknownTaggedObject")
            .field("discriminator", &self.discriminator)
            .field("field_count", &self.raw.len())
            .finish()
    }
}

impl Serialize for UnknownTaggedObject {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.raw.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for UnknownTaggedObject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::from_value(Value::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// A supplied value was not a tagged JSON object.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UnknownTaggedObjectError {
    /// The discriminator was absent or had the wrong JSON kind.
    #[error("{0}")]
    Invalid(&'static str),
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde::{Deserialize, Serialize, de::DeserializeOwned};
    use serde_json::{Map, Value, json};
    use static_assertions::{assert_impl_all, assert_not_impl_any};

    use super::{ExtraFields, Nullable, Omittable};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct ThreeState {
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        value: Omittable<Nullable<String>>,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct OptionalNonNull {
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        value: Omittable<String>,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct RequiredNullable {
        value: Nullable<String>,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct WithExtras {
        known: String,
        #[serde(flatten)]
        extra: ExtraFields,
    }

    assert_impl_all!(Omittable<String>: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(Nullable<String>: Serialize, DeserializeOwned, Send, Sync);
    assert_impl_all!(ExtraFields: Serialize, DeserializeOwned, Send, Sync);
    assert_not_impl_any!(Nullable<String>: Default);

    #[test]
    fn optional_nullable_preserves_all_three_states() {
        let omitted: ThreeState = serde_json::from_str("{}").expect("decode omitted state");
        let null: ThreeState = serde_json::from_str(r#"{"value":null}"#).expect("decode null");
        let value: ThreeState = serde_json::from_str(r#"{"value":"ok"}"#).expect("decode value");

        assert_eq!(omitted.value, Omittable::Omitted);
        assert_eq!(null.value, Omittable::Value(Nullable::Null));
        assert_eq!(
            value.value,
            Omittable::Value(Nullable::Value(String::from("ok")))
        );
        assert_eq!(
            serde_json::to_string(&omitted).expect("encode omitted state"),
            "{}"
        );
        assert_eq!(
            serde_json::to_string(&null).expect("encode null state"),
            r#"{"value":null}"#
        );
    }

    #[test]
    fn explicit_null_is_rejected_for_optional_non_null() {
        let error = serde_json::from_str::<OptionalNonNull>(r#"{"value":null}"#)
            .expect_err("null must fail for String");
        assert!(error.to_string().contains("string"));
    }

    #[test]
    fn missing_is_rejected_for_required_nullable() {
        let error = serde_json::from_str::<RequiredNullable>("{}")
            .expect_err("required nullable field must be present");
        assert!(error.to_string().contains("missing field `value`"));
    }

    #[test]
    fn omitted_cannot_be_serialized_outside_an_object_field() {
        let error = serde_json::to_string(&Omittable::<String>::Omitted)
            .expect_err("standalone omission has no JSON representation");
        assert!(error.to_string().contains("must be skipped"));
    }

    #[test]
    fn extra_fields_flatten_and_round_trip() {
        let decoded: WithExtras = serde_json::from_value(json!({
            "known": "stable",
            "future_object": {"nested": true},
            "future_number": 17
        }))
        .expect("decode object with future properties");

        assert_eq!(decoded.extra.len(), 2);
        assert_eq!(decoded.extra.get("future_number"), Some(&json!(17)));
        assert_eq!(
            serde_json::to_value(decoded).expect("encode object with future properties"),
            json!({
                "known": "stable",
                "future_object": {"nested": true},
                "future_number": 17
            })
        );
    }

    #[test]
    fn extra_fields_reject_reserved_keys_and_hide_debug_contents() {
        let mut fields = Map::<String, Value>::new();
        fields.insert(String::from("known"), json!("sensitive-value"));
        let error = ExtraFields::try_from_map(fields, ["known", "id"])
            .expect_err("known key must conflict");

        assert_eq!(error.key(), "known");

        let extra: ExtraFields =
            serde_json::from_value(json!({"token": "secret-text"})).expect("decode extra fields");
        let debug = format!("{extra:?}");
        assert_eq!(debug, "ExtraFields { len: 1 }");
        assert!(!debug.contains("token"));
        assert!(!debug.contains("secret-text"));
    }

    proptest! {
        #[test]
        fn optional_nullable_semantic_round_trip(value in proptest::option::of(proptest::option::of(".*"))) {
            let state = match value {
                None => ThreeState { value: Omittable::Omitted },
                Some(None) => ThreeState { value: Omittable::Value(Nullable::Null) },
                Some(Some(value)) => ThreeState {
                    value: Omittable::Value(Nullable::Value(value)),
                },
            };

            let encoded = serde_json::to_vec(&state).expect("encode three-state value");
            let decoded = serde_json::from_slice::<ThreeState>(&encoded)
                .expect("decode three-state value");
            prop_assert_eq!(decoded, state);
        }

        #[test]
        fn extra_fields_semantically_round_trip(entries in proptest::collection::btree_map("[a-z]{1,12}", any::<i64>(), 0..24)) {
            let object = entries
                .into_iter()
                .map(|(key, value)| (key, Value::from(value)))
                .collect::<Map<String, Value>>();
            let extra = ExtraFields::try_from_map(object, std::iter::empty::<&str>())
                .expect("there are no reserved keys");
            let encoded = serde_json::to_vec(&extra).expect("encode extra fields");
            let decoded = serde_json::from_slice::<ExtraFields>(&encoded)
                .expect("decode extra fields");
            prop_assert_eq!(decoded, extra);
        }
    }
}
