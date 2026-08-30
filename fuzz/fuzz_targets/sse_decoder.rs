#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let mut lines = text.lines();
        let mut event_name = None;
        let mut event_data = String::new();
        while let Some(line) = lines.next() {
            if let Some(rest) = line.strip_prefix("event:") {
                event_name = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("data:") {
                event_data.push_str(rest.trim());
            } else if line.trim().is_empty() {
                // Dispatch event frame
                let _ = (event_name.take(), std::mem::take(&mut event_data));
            }
        }
    }
});
