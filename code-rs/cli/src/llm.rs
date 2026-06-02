use std::io::Read;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use code_common::CliConfigOverrides;
use code_core::config::Config;
use code_core::config::ConfigOverrides;
use code_core::ResponseEvent;
use code_core::ModelClient;
use code_core::ModelProviderInfo;
use code_core::agent_defaults::model_guide_markdown_with_custom;
use code_core::AuthManager;
use code_core::Prompt;
use code_core::TextFormat;
use code_app_server_protocol::AuthMode;
use code_protocol::models::{ContentItem, ResponseItem};
use futures::StreamExt;

#[derive(Debug, Parser)]
pub struct LlmCli {
    #[clap(skip)]
    pub config_overrides: CliConfigOverrides,

    #[command(subcommand)]
    pub cmd: LlmSubcommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum LlmSubcommand {
    /// Send a one-off structured request to the model (side-channel; no TUI events)
    Request(RequestArgs),
}

#[derive(Debug, Parser)]
#[command(group(
    clap::ArgGroup::new("message_input")
        .required(true)
        .args(["message", "message_file"])
))]
pub struct RequestArgs {
    /// Developer message to prepend (kept separate from system instructions)
    #[arg(long)]
    pub developer: String,

    /// Primary user message/content
    #[arg(long)]
    pub message: Option<String>,

    /// Read primary user message/content from a UTF-8 file
    #[arg(long = "message-file", value_name = "PATH")]
    pub message_file: Option<PathBuf>,

    /// `text.format.type` (e.g. json_schema)
    #[arg(long = "format-type", default_value = "json_schema")]
    pub format_type: String,

    /// Optional `text.format.name`
    #[arg(long = "format-name")]
    pub format_name: Option<String>,

    /// Set `text.format.strict`
    #[arg(long = "format-strict", default_value_t = true)]
    pub format_strict: bool,

    /// Inline JSON for the schema (mutually exclusive with --schema-file)
    #[arg(long = "schema-json")]
    pub schema_json: Option<String>,

    /// Path to a JSON schema file (mutually exclusive with --schema-json)
    #[arg(long = "schema-file")]
    pub schema_file: Option<PathBuf>,

    /// Optional model override (e.g. gpt-4.1, gpt-5.1)
    #[arg(long)]
    pub model: Option<String>,
}

pub async fn run_llm(opts: LlmCli) -> anyhow::Result<()> {
    match opts.cmd {
        LlmSubcommand::Request(req) => run_llm_request(opts.config_overrides, req).await,
    }
}

async fn run_llm_request(
    cli_overrides: CliConfigOverrides,
    args: RequestArgs,
) -> anyhow::Result<()> {
    let overrides_vec = cli_overrides.parse_overrides().map_err(anyhow::Error::msg)?;

    let overrides = if let Some(model) = &args.model {
        ConfigOverrides {
            model: Some(model.clone()),
            compact_prompt_override: None,
            compact_prompt_override_file: None,
            ..ConfigOverrides::default()
        }
    } else {
        ConfigOverrides::default()
    };

    let config = Config::load_with_cli_overrides(overrides_vec, overrides)?;
    let message = read_request_message(&args)?;

    // Build Prompt with custom developer + user messages, no extra tools
    let mut input: Vec<ResponseItem> = Vec::new();
    input.push(ResponseItem::Message {
        id: None,
        role: "developer".to_string(),
        content: vec![ContentItem::InputText { text: args.developer.clone() }],
        end_turn: None,
        phase: None,
    });
    input.push(ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText { text: message }],
        end_turn: None,
        phase: None,
    });

    // Resolve schema
    let schema_val: Option<serde_json::Value> = if let Some(s) = &args.schema_json {
        Some(serde_json::from_str::<serde_json::Value>(s)?)
    } else if let Some(p) = &args.schema_file {
        let data = std::fs::read_to_string(p)?;
        Some(serde_json::from_str::<serde_json::Value>(&data)?)
    } else {
        None
    };

    let text_format = TextFormat {
        r#type: args.format_type.clone(),
        name: args.format_name.clone(),
        strict: Some(args.format_strict),
        schema: schema_val,
    };

    let mut prompt = Prompt::default();
    prompt.input = input;
    prompt.store = true;
    prompt.user_instructions = None;
    prompt.status_items = vec![];
    prompt.base_instructions_override = None;
    prompt.text_format = Some(text_format);
    if let Some(custom) = model_guide_markdown_with_custom(&config.agents) {
        prompt.model_descriptions = Some(custom);
    }
    prompt.set_log_tag("cli/manual_prompt");

    // Auth + provider
    let auth_mgr = AuthManager::shared_with_mode_and_originator(
        config.code_home.clone(),
        AuthMode::ApiKey,
        config.responses_originator_header.clone(),
    );
    let provider: ModelProviderInfo = config.model_provider.clone();
    let client = ModelClient::new(
        std::sync::Arc::new(config.clone()),
        Some(auth_mgr),
        None,
        provider,
        config.model_reasoning_effort,
        config.model_reasoning_summary,
        config.model_text_verbosity,
        uuid::Uuid::new_v4(),
        std::sync::Arc::new(std::sync::Mutex::new(code_core::debug_logger::DebugLogger::new(false)?)),
    );

    // Collect the assistant message text from the stream (no TUI events)
    let mut stream = client.stream(&prompt).await?;
    let mut final_text: String = String::new();
    let mut saw_output_text_delta = false;
    tracing::info!("LLM: created");
    while let Some(ev) = stream.next().await {
        let ev = ev?;
        match ev {
            ResponseEvent::ReasoningSummaryDelta { delta, .. } => { tracing::info!(target: "llm", "thinking: {}", delta); }
            ResponseEvent::ReasoningContentDelta { delta, .. } => { tracing::info!(target: "llm", "reasoning: {}", delta); }
            ResponseEvent::OutputItemDone { item, .. } => {
                append_output_item_done(&mut final_text, &mut saw_output_text_delta, item);
            }
            ResponseEvent::OutputTextDelta { delta, .. } => {
                tracing::info!(target: "llm", "delta: {}", delta);
                append_output_text_delta(&mut final_text, &mut saw_output_text_delta, delta);
            }
            ResponseEvent::Completed { .. } => { tracing::info!("LLM: completed"); break; }
            _ => {}
        }
    }

    println!("{}", final_text);
    Ok(())
}

fn read_request_message(args: &RequestArgs) -> anyhow::Result<String> {
    read_request_message_from(args, || {
        let mut input = String::new();
        std::io::stdin()
            .read_to_string(&mut input)
            .context("failed to read --message - from stdin")?;
        Ok(input)
    })
}

fn read_request_message_from<F>(args: &RequestArgs, read_stdin: F) -> anyhow::Result<String>
where
    F: FnOnce() -> anyhow::Result<String>,
{
    match (&args.message, &args.message_file) {
        (Some(_), Some(_)) => anyhow::bail!("--message and --message-file are mutually exclusive"),
        (Some(message), None) if message == "-" => read_stdin(),
        (Some(message), None) => Ok(message.clone()),
        (None, Some(path)) => read_message_file(path),
        (None, None) => anyhow::bail!("one of --message or --message-file is required"),
    }
}

fn read_message_file(path: &Path) -> anyhow::Result<String> {
    std::fs::read_to_string(path)
        .with_context(|| format!("failed to read --message-file {}", path.display()))
}

fn append_output_text_delta(
    final_text: &mut String,
    saw_output_text_delta: &mut bool,
    delta: String,
) {
    *saw_output_text_delta = true;
    final_text.push_str(&delta);
}

fn append_output_item_done(
    final_text: &mut String,
    saw_output_text_delta: &mut bool,
    item: ResponseItem,
) {
    if *saw_output_text_delta {
        return;
    }

    if let ResponseItem::Message { content, .. } = item {
        for c in content {
            if let ContentItem::OutputText { text } = c {
                final_text.push_str(&text);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_with_message(message: Option<&str>, message_file: Option<PathBuf>) -> RequestArgs {
        RequestArgs {
            developer: "developer".to_string(),
            message: message.map(str::to_string),
            message_file,
            format_type: "json_schema".to_string(),
            format_name: None,
            format_strict: true,
            schema_json: None,
            schema_file: None,
            model: None,
        }
    }

    #[test]
    fn request_message_uses_inline_message() {
        let args = args_with_message(Some("hello"), None);

        let message = read_request_message_from(&args, || anyhow::bail!("stdin should not be read"))
            .expect("inline message should resolve");

        assert_eq!(message, "hello");
    }

    #[test]
    fn request_message_dash_reads_stdin() {
        let args = args_with_message(Some("-"), None);

        let message = read_request_message_from(&args, || Ok("from stdin".to_string()))
            .expect("stdin message should resolve");

        assert_eq!(message, "from stdin");
    }

    #[test]
    fn request_message_reads_message_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("prompt.txt");
        std::fs::write(&path, "from file").expect("write prompt file");
        let args = args_with_message(None, Some(path));

        let message = read_request_message_from(&args, || anyhow::bail!("stdin should not be read"))
            .expect("file message should resolve");

        assert_eq!(message, "from file");
    }

    #[test]
    fn request_message_rejects_multiple_sources() {
        let args = args_with_message(Some("hello"), Some(PathBuf::from("prompt.txt")));

        let err = read_request_message_from(&args, || anyhow::bail!("stdin should not be read"))
            .expect_err("multiple message sources should fail");

        assert!(
            err.to_string()
                .contains("--message and --message-file are mutually exclusive")
        );
    }

    #[test]
    fn request_cli_requires_a_message_source() {
        let err = LlmCli::try_parse_from(["code", "request", "--developer", "developer"])
            .expect_err("missing message source should fail");

        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn request_cli_rejects_multiple_message_sources() {
        let err = LlmCli::try_parse_from([
            "code",
            "request",
            "--developer",
            "developer",
            "--message",
            "hello",
            "--message-file",
            "prompt.txt",
        ])
        .expect_err("multiple message sources should fail");

        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn output_item_done_is_used_when_no_deltas_arrive() {
        let mut final_text = String::new();
        let mut saw_delta = false;

        append_output_item_done(&mut final_text, &mut saw_delta, output_message("complete"));

        assert_eq!(final_text, "complete");
        assert!(!saw_delta);
    }

    #[test]
    fn output_item_done_does_not_duplicate_streamed_deltas() {
        let mut final_text = String::new();
        let mut saw_delta = false;

        append_output_text_delta(&mut final_text, &mut saw_delta, "partial".to_string());
        append_output_item_done(&mut final_text, &mut saw_delta, output_message("partial"));

        assert_eq!(final_text, "partial");
        assert!(saw_delta);
    }

    fn output_message(text: &str) -> ResponseItem {
        ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText {
                text: text.to_string(),
            }],
            end_turn: None,
            phase: None,
        }
    }
}
