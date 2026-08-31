use openai_rs::{
    ApiKey, Client,
    responses::{CreateResponseRequest, FunctionCallOutput, FunctionTool},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
struct WeatherArgs {
    city: String,
}

#[derive(Debug, Serialize, JsonSchema)]
struct WeatherResult {
    city: String,
    temperature_c: i32,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = ApiKey::new(std::env::var("OPENAI_API_KEY")?)?;
    let client = Client::new(api_key)?;

    let tool = FunctionTool::for_type::<WeatherArgs>("get_weather", "Return current weather")?;

    let request =
        CreateResponseRequest::new("gpt-5.6", "What is the weather in Shenzhen?").with_tool(tool);

    let response = client.responses().create(request.clone()).await?;

    if let Some(call) = response.function_calls().next() {
        let args: WeatherArgs = call.arguments_as()?;
        let output = FunctionCallOutput::json(
            call.call_id(),
            &WeatherResult {
                city: args.city,
                temperature_c: 28,
            },
        )?;

        // `previous_response_id` does not carry the previous request's tools, so
        // every continuation turn must resend them (the official function-calling
        // examples do the same). `follow_up_from` copies the tools — plus other
        // stable prefix fields — from the original request onto the follow-up.
        // (Alternatively, use response.to_input_items() for local stateless multi-turn replay)
        let follow_up = client
            .responses()
            .create(CreateResponseRequest::follow_up_from(
                &request,
                &response,
                vec![output.into()],
            ))
            .await?;

        println!("{}", follow_up.output_text());
    } else {
        println!("{}", response.output_text());
    }

    Ok(())
}
