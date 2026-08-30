use openai_rs::{ApiKey, Client, responses::CreateResponseRequest};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = ApiKey::new(std::env::var("OPENAI_API_KEY")?)?;
    let client = Client::new(api_key)?;
    let request =
        CreateResponseRequest::new("gpt-5.4", "Explain typed API clients in one sentence.");

    let response = client.responses().create(request).await?;
    println!("{}", response.output_text());
    Ok(())
}
