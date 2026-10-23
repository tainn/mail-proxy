mod api;
mod auth;
mod config;
mod error;
mod mail;
mod render;
mod routes;
mod smtp;

use std::io;
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};
use tracing::level_filters::LevelFilter;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use config::{Config, ConfigError};
use routes::AppState;

#[derive(Debug, thiserror::Error)]
enum StartupError {
    #[error("invalid configuration: {0}")]
    Config(#[from] ConfigError),
    #[error("failed to set up smtp transport: {0}")]
    Smtp(#[from] lettre::transport::smtp::Error),
    #[error("failed to listen on {0}: {1}")]
    Bind(SocketAddr, io::Error),
    #[error("server error: {0}")]
    Serve(io::Error),
}

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();
    let Err(error) = run().await else {
        return ExitCode::SUCCESS;
    };
    error!("{error}");
    ExitCode::FAILURE
}

fn init_tracing() {
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

async fn run() -> Result<(), StartupError> {
    let config = Config::from_env()?;
    log_config(&config);

    let state = Arc::new(AppState {
        transport: smtp::transport(config.smtp)?,
        default_from: config.default_from,
    });
    let verified = Arc::clone(&state);
    tokio::spawn(async move { smtp::verify(&verified.transport).await });

    let app = routes::router(state, config.api_token, config.max_request_bytes);
    let addr = config.listen_addr;
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|error| StartupError::Bind(addr, error))?;
    info!("listening on {addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(StartupError::Serve)
}

fn log_config(config: &Config) {
    let smtp = &config.smtp;
    info!(
        domain = %smtp.domain,
        port = smtp.port,
        tls = ?smtp.tls,
        authenticated = smtp.credentials.is_some(),
        "smtp relay configured"
    );
    if config.api_token.is_none() {
        info!("API_TOKEN is not set, authentication is disabled");
    }
}

async fn shutdown_signal() {
    let interrupt = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            error!("failed to listen for interrupt signal: {error}");
            std::future::pending::<()>().await;
        }
    };
    let terminate = async {
        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                terminate.recv().await;
            }
            Err(error) => {
                error!("failed to listen for terminate signal: {error}");
                std::future::pending::<()>().await;
            }
        }
    };
    tokio::select! {
        () = interrupt => {},
        () = terminate => {},
    }
    info!("shutting down");
}
