//! Terminal conversation using OPENAI_BASE_URL, OPENAI_MODEL and OPENAI_API_KEY.
//! Run from the repository root: cargo run -p openai-rs-sdk --example chat_loop

use std::{
    env,
    error::Error,
    io::{self, Write},
};

use openai_rs::{
    ApiKey, Client,
    responses::{
        CreateResponseRequest, InputMessage, ResponseIncludable, ResponseInputItem, ResponseStatus,
    },
};

fn required_env(name: &str) -> io::Result<String> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("请设置非空环境变量 {name}"),
        )),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let base_url = required_env("OPENAI_BASE_URL")?;
    let model = required_env("OPENAI_MODEL")?.trim().to_owned();
    let api_key = ApiKey::new(required_env("OPENAI_API_KEY")?)?;
    let client = Client::builder(api_key)
        .base_url(base_url.trim().parse()?)
        // Allow local HTTP services at literal loopback IPs such as 127.0.0.1.
        .allow_insecure_loopback(true)
        .build()?;

    let mut history: Vec<ResponseInputItem> = Vec::new();
    let stdin = io::stdin();
    println!("终端对话已启动，模型：{model}");
    println!("每行发送一条消息；/clear 清空上下文，/exit 或 /quit 退出。\n");

    loop {
        print!("你> ");
        io::stdout().flush()?;
        let mut line = String::new();
        if stdin.read_line(&mut line)? == 0 {
            break; // EOF, including Ctrl+Z followed by Enter on Windows.
        }
        let message = line.trim();
        match message {
            "" => continue,
            "/exit" | "/quit" => break,
            "/clear" => {
                history.clear();
                println!("已清空上下文。\n");
                continue;
            }
            _ => {}
        }

        // Commit this turn to history only after the request succeeds.
        let user_message: ResponseInputItem = InputMessage::user(message).into();
        let mut input = history.clone();
        input.push(user_message.clone());
        let request = CreateResponseRequest::new(&model, input)
            .store(false)
            .include(ResponseIncludable::ReasoningEncryptedContent);
        let response = match client.responses().create(request).await {
            Ok(response) => response,
            Err(error) => {
                eprintln!("请求失败：{error}\n");
                continue;
            }
        };
        if let Some(error) = response.error() {
            eprintln!("生成失败：{}：{}\n", error.code().as_str(), error.message());
            continue;
        }
        if matches!(
            response.status(),
            Some(ResponseStatus::Failed | ResponseStatus::Cancelled)
        ) {
            eprintln!("本轮生成未成功完成，请重新输入。\n");
            continue;
        }

        let text = response.output_text();
        println!(
            "助手> {}\n",
            if text.is_empty() {
                response.refusal().unwrap_or("（本轮未返回文本）")
            } else {
                &text
            }
        );
        if matches!(response.status(), Some(ResponseStatus::Incomplete)) {
            eprintln!("本轮回复未完成，可以继续追问。\n");
        }

        // Replay full output items, including reasoning and message metadata.
        history.push(user_message);
        history.extend(response.to_input_items());
    }

    println!("对话已结束。");
    Ok(())
}
