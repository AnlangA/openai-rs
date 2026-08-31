use std::io::{self, Write};

use openai_rs::{
    ApiKey, Client,
    responses::{CreateResponseRequest, ResponseAccumulator, ResponseStreamEvent},
};
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
