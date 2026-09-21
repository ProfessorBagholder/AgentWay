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

    let token = tokio_util::sync::CancellationToken::new();
    let publisher = agentway_server::publishing::Publisher::new(
        db.clone(),
        std::env::var("PUBLISHING_DIR")
            .unwrap_or_else(|_| ".agentway/publishing".into())
            .into(),
        token.clone(),
    )
    .await?;
    let bridge_addr: SocketAddr = std::env::var("BRIDGE_BIND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8788".into())
        .parse()?;
    let bridge_listener = tokio::net::TcpListener::bind(bridge_addr).await?;
    let bridge_token = token.clone();
    let bridge_router = publisher.bridge_router();
    let bridge = tokio::spawn(async move {
        axum::serve(bridge_listener, bridge_router)
            .with_graceful_shutdown(bridge_token.cancelled_owned())
            .await
    });
    let worker = publisher.start_worker();
    tracing::info!(%addr, %bridge_addr, "AgentWay ready");
    let server = axum::serve(
        listener,
        router_with_shutdown(db.clone(), &assets, token.clone()).merge(publisher.admin_router()),
    )
    .with_graceful_shutdown(async move {
        shutdown().await;
        token.cancel();
    });
    server.await?;
    bridge.await??;
    worker.await?;
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
