use agentway_server::{database, router_with_shutdown};
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "agentway_server=info,tower_http=info".into()),
        )
        .init();
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://agentway.db".into());
    let assets = std::env::var("ASSET_DIR").unwrap_or_else(|_| "web/dist".into());
    let addr: SocketAddr = std::env::var("BIND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8787".into())
        .parse()?;
    let db = database(&url).await?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "AgentWay ready; provider execution is not yet implemented");
    let token = tokio_util::sync::CancellationToken::new();
    let server = axum::serve(
        listener,
        router_with_shutdown(db.clone(), &assets, token.clone()),
    )
    .with_graceful_shutdown(async move {
        shutdown().await;
        token.cancel();
    });
    server.await?;
    db.close().await;
    Ok(())
}
async fn shutdown() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("signal handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}
