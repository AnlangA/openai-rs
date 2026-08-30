use openai_rs_client::{AdminApiKey, Client};

fn main() {
    let key = AdminApiKey::new("admin-placeholder").unwrap();
    let _client = Client::new(key);
}
