#![no_main]

use libfuzzer_sys::fuzz_target;
use openai_rs_client::sse::{SseEndpointPolicy, SseLimits, SseStreamDecoder};

fuzz_target!(|data: &[u8]| {
    let Ok(limits) = SseLimits::new(256, 4 * 1024, 32) else {
        return;
    };
    let policies = [
        SseEndpointPolicy::responses(),
        SseEndpointPolicy::legacy_done(),
        SseEndpointPolicy::eof_terminated(),
    ];
    let policy = policies[data.first().copied().unwrap_or(0) as usize % policies.len()].clone();
    let mut decoder = SseStreamDecoder::new(limits, policy);
    let payload = if data.len() > 1 { &data[1..] } else { data };
    let chunk_size = 1 + data.first().copied().unwrap_or(1) as usize % 64;
    for chunk in payload.chunks(chunk_size) {
        if decoder.push(chunk).is_err() {
            return;
        }
    }
    let _ = decoder.finish();
});
