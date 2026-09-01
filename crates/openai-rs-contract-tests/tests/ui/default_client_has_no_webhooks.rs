use openai_rs_client::WebhookVerifier;

fn assert_boundary() {
    let _ = WebhookVerifier::new("whsec_test_placeholder_secret");
}

fn main() {}
