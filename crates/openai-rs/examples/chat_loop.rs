//! Runs an interactive terminal conversation through the Responses API.
//!
//! # Configuration
//!
//! Set `OPENAI_BASE_URL`, `OPENAI_MODEL`, and `OPENAI_API_KEY` before starting
//! the example. Set `RUST_LOG=openai_rs_client=trace` to write raw JSON request
//! and response bodies to stderr, including responses that fail to decode.
//!
//! # Usage
//!
//! ```text
//! cargo run -p openai-rs-sdk --example chat_loop
//! ```
//!
//! Enter one message per line. `/clear` resets the local conversation;
//! `/exit`, `/quit`, and end-of-file terminate the program. Successful turns
//! retain complete output items, including reasoning, for the next request.
//! Request failures leave the previous conversation history unchanged.

use std::{
    env,
    error::Error,
    io::{self, Write},
};

use openai_rs::{
    ApiKey, Client,
    responses::{
        CreateResponseRequest, InputMessage, ResponseIncludable, ResponseInputItem, ResponseStatus,
    },
};

/// Reads a required, nonblank environment variable without trimming its value.
///
/// # Errors
///
/// Returns [`io::ErrorKind::InvalidInput`] if the variable is missing, contains
/// only whitespace, or cannot be represented as UTF-8.
fn required_env(name: &str) -> io::Result<String> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Set the {name} environment variable to a nonblank value"),
        )),
    }
}

mod support;

/// Initializes the client and processes terminal messages until the user exits.
///
/// # Errors
///
/// Returns an error if logging initialization, environment validation, client
/// configuration, or terminal I/O fails. Individual API failures are printed
/// and leave the loop available for another message.
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    support::init_logging()?;

    let base_url = required_env("OPENAI_BASE_URL")?;
    let model = required_env("OPENAI_MODEL")?.trim().to_owned();
    let api_key = ApiKey::new(required_env("OPENAI_API_KEY")?)?;
    let client = Client::builder(api_key)
        .base_url(base_url.trim().parse()?)
        // Allow local HTTP services at literal loopback IPs such as 127.0.0.1.
        .allow_insecure_loopback(true)
        .build()?;

    let mut history: Vec<ResponseInputItem> = Vec::new();
    let stdin = io::stdin();
    println!("Terminal chat started. Model: {model}");
    println!("Enter one message per line; /clear resets history; /exit or /quit exits.\n");

    loop {
        print!("You> ");
        io::stdout().flush()?;
        let mut line = String::new();
        if stdin.read_line(&mut line)? == 0 {
            break; // EOF, including Ctrl+Z followed by Enter on Windows.
        }
        let message = line.trim();
        match message {
            "" => continue,
            "/exit" | "/quit" => break,
            "/clear" => {
                history.clear();
                println!("Conversation history cleared.\n");
                continue;
            }
            _ => {}
        }

        // Commit this turn to history only after the request succeeds.
        let user_message: ResponseInputItem = InputMessage::user(message).into();
        let mut input = history.clone();
        input.push(user_message.clone());
        let request = CreateResponseRequest::new(&model, input)
            .store(false)
            .include(ResponseIncludable::ReasoningEncryptedContent);
        let response = match client.responses().create(request).await {
            Ok(response) => response,
            Err(error) => {
                eprintln!("Request failed: {error}\n");
                if let Some(path) = error.decode_path() {
                    eprintln!("JSON decode path: {path}\n");
                }
                continue;
            }
        };
        if let Some(error) = response.error() {
            eprintln!(
                "Generation failed: {}: {}\n",
                error.code().as_str(),
                error.message()
            );
            continue;
        }
        if matches!(
            response.status(),
            Some(ResponseStatus::Failed | ResponseStatus::Cancelled)
        ) {
            eprintln!("This turn did not complete successfully. Enter another message.\n");
            continue;
        }

        let text = response.output_text();
        println!(
            "Assistant> {}\n",
            if text.is_empty() {
                response
                    .refusal()
                    .unwrap_or("(No text returned for this turn.)")
            } else {
                &text
            }
        );
        if matches!(response.status(), Some(ResponseStatus::Incomplete)) {
            eprintln!("This response is incomplete. You can send a follow-up message.\n");
        }

        // Replay full output items, including reasoning and message metadata.
        history.push(user_message);
        history.extend(response.to_input_items());
    }

    println!("Chat ended.");
    Ok(())
}
