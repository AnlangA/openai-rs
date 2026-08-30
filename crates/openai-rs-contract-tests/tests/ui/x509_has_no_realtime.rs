use openai_rs_client::X509Client;

fn assert_boundary(client: X509Client) {
    let _ = client.realtime();
}

fn main() {}
