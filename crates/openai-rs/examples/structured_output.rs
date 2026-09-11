//! Requests JSON matching a Rust type and decodes the structured response.
//!
//! Set `OPENAI_API_KEY` before running. This example uses the default Platform
//! endpoint and the model configured in the source. `RUST_LOG` controls logging.
//!
//! # Usage
//!
//! ```text
//! cargo run -p openai-rs-sdk --example structured_output
//! ```

use openai_rs::{
    ApiKey, Client, StructuredOutput, responses::CreateResponseRequest, types::ModelId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct Step {
    explanation: String,
    output: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct MathSolution {
    problem: String,
    steps: Vec<Step>,
    final_answer: String,
}

mod support;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    support::init_logging()?;
    let api_key = ApiKey::new(std::env::var("OPENAI_API_KEY")?)?;
    let client = Client::new(api_key)?;

    let format = StructuredOutput::<MathSolution>::new("math_solution")?
        .with_description("Step-by-step mathematical reasoning and solution");

    let request = CreateResponseRequest::new(
        ModelId::GPT_5_6_SOL.as_str(),
        "Solve the linear equation: 3x + 12 = 27",
    )
    .text_format(&format);

    let response = client.responses().create(request).await?;

    let solution: MathSolution = response.output_parsed()?;

    println!("Problem: {}", solution.problem);
    println!("Steps:");
    for (i, step) in solution.steps.iter().enumerate() {
        println!("  {}. {}: {}", i + 1, step.explanation, step.output);
    }
    println!("Final Answer: {}", solution.final_answer);

    Ok(())
}
