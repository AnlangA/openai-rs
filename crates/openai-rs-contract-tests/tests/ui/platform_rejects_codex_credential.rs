use openai_rs_client::Client;
use openai_rs_codex::ManagedAppServerCredential;

fn main() {
    let _client = Client::new(ManagedAppServerCredential);
}
