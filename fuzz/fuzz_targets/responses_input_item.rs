#![no_main]

use libfuzzer_sys::fuzz_target;
use openai_rs_types::responses::ResponseInputItem;

fuzz_target!(|data: &[u8]| {
    if let Ok(item) = serde_json::from_slice::<ResponseInputItem>(data) {
        if let Ok(bytes) = serde_json::to_vec(&item) {
            let _ = serde_json::from_slice::<ResponseInputItem>(&bytes);
        }
    }
});
