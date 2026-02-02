use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    code_gateway::run_main().await
}
