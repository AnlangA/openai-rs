use openai_rs_client::{AdminClient, ApiKey};

fn main() {
    let key = ApiKey::new("platform-placeholder").unwrap();
    let _client = AdminClient::new(key);
}
