use anyhow::Result;
use clap::Parser;
use mcp_server::{Cli, RustWatcherMcpServer};
use rmcp::transport::stdio;
use rmcp::ServiceExt;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "mcp_server=info,graph_builder=info,project_indexer=info".into()
            }),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = Cli::parse();
    let server = RustWatcherMcpServer::build(args)?;
    let service = match server.serve(stdio()).await {
        Ok(service) => service,
        Err(error)
            if error
                .to_string()
                .contains("connection closed: initialize request") =>
        {
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    service.waiting().await?;
    Ok(())
}
