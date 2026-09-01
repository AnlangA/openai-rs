use openai_rs_client::Client;

fn assert_boundary(client: Client) {
    let _ = client.voices();
}

fn main() {}
