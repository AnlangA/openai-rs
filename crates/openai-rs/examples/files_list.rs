use openai_rs::{ApiKey, Client, types::files::FileListParams};
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
