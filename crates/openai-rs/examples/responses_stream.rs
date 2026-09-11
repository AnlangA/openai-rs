//! Streams a Responses completion and prints text deltas to the terminal.
//!
//! Set `OPENAI_API_KEY` before running. This example uses the default Platform
//! endpoint and the model configured in the source. `RUST_LOG` controls logging.
//!
//! # Usage
//!
//! ```text
//! cargo run -p openai-rs-sdk --example responses_stream
//! ```

use std::io::{self, Write};

use openai_rs::{
    ApiKey, Client,
    responses::{CreateResponseRequest, ResponseAccumulator, ResponseStreamEvent},
};
use tokio_stream::StreamExt;

mod support;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    support::init_logging()?;
    let api_key = ApiKey::new(std::env::var("OPENAI_API_KEY")?)?;
    let client = Client::new(api_key)?;

    let request =
        CreateResponseRequest::new("gpt-5.6-sol", "Write a haiku about Rust programming.")
            .into_streaming();

    let mut stream = client.responses().create_stream(request).await?;
    let mut accumulator = ResponseAccumulator::new();

    while let Some(event) = stream.next().await {
        let event = event?;

        if let ResponseStreamEvent::OutputTextDelta(delta) = &event {
            print!("{}", delta.delta());
            io::stdout().flush()?;
        }

        accumulator.push(event)?;
    }

    println!();

    let final_response = accumulator.finish()?;
    println!("\n[Stream completed, response ID: {}]", final_response.id());

    Ok(())
}
