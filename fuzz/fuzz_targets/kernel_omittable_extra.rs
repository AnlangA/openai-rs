#![no_main]

use libfuzzer_sys::fuzz_target;
use openai_rs_types::{ExtraFields, Nullable, Omittable};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct FuzzSubject {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    name: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    score: Omittable<Nullable<f64>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

fuzz_target!(|data: &[u8]| {
    if let Ok(subject) = serde_json::from_slice::<FuzzSubject>(data) {
        if let Ok(serialized) = serde_json::to_vec(&subject) {
            let _ = serde_json::from_slice::<FuzzSubject>(&serialized);
        }
    }
});
