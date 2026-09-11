//! Iterates through uploaded-file pages using the SDK pagination stream.
//!
//! Set `OPENAI_API_KEY` before running. This example uses the default Platform
//! endpoint. `RUST_LOG` controls logging.
//!
//! # Usage
//!
//! ```text
//! cargo run -p openai-rs-sdk --example files_list
//! ```

use openai_rs::{ApiKey, Client, types::files::FileListParams};
use tokio_stream::StreamExt;

mod support;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    support::init_logging()?;
    let api_key = ApiKey::new(std::env::var("OPENAI_API_KEY")?)?;
    let client = Client::new(api_key)?;

    // `list_pages` yields one page per HTTP round trip, validates the returned
    // cursor, and advances `after` automatically until `has_more` is
    // exhausted. Unlike the python/node SDKs' auto-paging iterators, callers
    // flatten the pages themselves.
    let mut stream = client.files().list_pages(FileListParams::new());

    let mut total = 0usize;
    while let Some(page) = stream.next().await {
        let page = page?;
        for file in page.data() {
            println!("{} {}", file.id().as_str(), file.filename());
            total += 1;
        }
    }

    println!("Listed {total} file(s).");

    Ok(())
}
