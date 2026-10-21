mod auth;
mod pages;
mod routes;
mod state;

#[cfg(test)]
mod tests;

use std::error::Error;

use tracing_subscriber::EnvFilter;

use crate::state::App;

fn port_from_env(name: &str, default: u16) -> u16 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("darkbox_lab=info")),
        )
        .init();

    let lab_port = port_from_env("LAB_PORT", 8080);
    let excluded_port = port_from_env("EXCLUDED_PORT", 9099);
    let app = App::new(lab_port, excluded_port);

    let main_listener = tokio::net::TcpListener::bind(("127.0.0.1", lab_port)).await?;
    let excluded_listener = tokio::net::TcpListener::bind(("127.0.0.1", excluded_port)).await?;

    tracing::info!(port = lab_port, "lab listener ready");
    tracing::info!(port = excluded_port, "excluded listener ready");

    tokio::try_join!(
        axum::serve(main_listener, routes::main_router(app.clone())),
        axum::serve(excluded_listener, routes::excluded_router(app)),
    )?;
    Ok(())
}
