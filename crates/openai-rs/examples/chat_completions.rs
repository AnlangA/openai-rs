//! Creates a Chat Completions reply from typed system and user messages.
//!
//! Set `OPENAI_API_KEY` before running. This example uses the default Platform
//! endpoint and the model configured in the source. `RUST_LOG` controls logging.
//!
//! # Usage
//!
//! ```text
//! cargo run -p openai-rs-sdk --example chat_completions
//! ```

use openai_rs::{
    ApiKey, Client,
    types::chat::{ChatCompletionRequest, ChatSystemMessage, ChatUserMessage},
};

mod support;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    support::init_logging()?;
    let api_key = ApiKey::new(std::env::var("OPENAI_API_KEY")?)?;
    let client = Client::new(api_key)?;

    let request = ChatCompletionRequest::new(
        "gpt-5.6-sol",
        ChatUserMessage::text("Hello! Introduce yourself in one sentence."),
    )
    .with_message(ChatSystemMessage::new(
        "You are a helpful, concise assistant.",
    ));

    let response = client.chat_completions().create(request).await?;

    println!("Response: {}", response.output_text());
    println!("Model: {}", response.model);

    Ok(())
}
