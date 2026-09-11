//! Creates a server-side conversation and uses it in a Responses request.
//!
//! Set `OPENAI_API_KEY` before running. This example uses the default Platform
//! endpoint and the model configured in the source. `RUST_LOG` controls logging.
//!
//! # Usage
//!
//! ```text
//! cargo run -p openai-rs-sdk --example conversations
//! ```

use openai_rs::{
    ApiKey, Client, responses::CreateResponseRequest,
    types::conversations::CreateConversationRequest,
};

mod support;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    support::init_logging()?;
    let api_key = ApiKey::new(std::env::var("OPENAI_API_KEY")?)?;
    let client = Client::new(api_key)?;

    // 1. Create a server-managed Conversation resource
    let conversation = client
        .conversations()
        .create(CreateConversationRequest::new())
        .await?;
    println!("Created conversation: {}", conversation.id().as_str());

    // 2. Turn 1: Send a message scoped to this conversation
    let turn1 = client
        .responses()
        .create(
            CreateResponseRequest::new("gpt-5.6-sol", "My favorite color is emerald green.")
                .conversation(conversation.id().as_str()),
        )
        .await?;
    println!("Assistant: {}", turn1.output_text());

    // 3. Turn 2: Follow up within the same conversation without resending context
    let turn2 = client
        .responses()
        .create(
            CreateResponseRequest::new("gpt-5.6-sol", "What is my favorite color?")
                .conversation(conversation.id().as_str()),
        )
        .await?;
    println!("Assistant: {}", turn2.output_text());

    // 4. Clean up conversation resource
    client.conversations().delete(conversation.id()).await?;
    println!("Deleted conversation: {}", conversation.id().as_str());

    Ok(())
}
