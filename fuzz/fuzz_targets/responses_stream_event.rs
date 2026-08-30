#![no_main]

use libfuzzer_sys::fuzz_target;
use openai_rs_types::ResponseStreamEvent;

fuzz_target!(|data: &[u8]| {
    if let Ok(event) = serde_json::from_slice::<ResponseStreamEvent>(data) {
        if let Ok(bytes) = serde_json::to_vec(&event) {
            let _ = serde_json::from_slice::<ResponseStreamEvent>(&bytes);
        }
    }
});
