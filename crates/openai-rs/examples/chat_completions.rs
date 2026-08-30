use openai_rs::{
    ApiKey, Client,
    types::chat::{ChatCompletionRequest, ChatSystemMessage, ChatUserMessage},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = ApiKey::new(std::env::var("OPENAI_API_KEY")?)?;
    let client = Client::new(api_key)?;

    let request = ChatCompletionRequest::new(
        "gpt-5.6",
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
