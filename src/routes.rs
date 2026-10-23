use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, FromRequest, State};
use axum::routing::{get, post};
use axum::{Json, Router, middleware};
use lettre::AsyncTransport;
use lettre::message::Mailbox;
use tokio::task;
use tracing::info;

use crate::api::{SendRequest, SendResponse};
use crate::auth;
use crate::error::ApiError;
use crate::mail::{self, Mail};
use crate::smtp::Transport;

pub struct AppState {
    pub transport: Transport,
    pub default_from: Option<Mailbox>,
}

#[derive(FromRequest)]
#[from_request(via(Json), rejection(ApiError))]
struct JsonBody<T>(T);

pub fn router(state: Arc<AppState>, api_token: Option<String>, max_request_bytes: usize) -> Router {
    let send = Router::new()
        .route("/send", post(send))
        .layer(DefaultBodyLimit::max(max_request_bytes));
    let send = match api_token {
        Some(token) => send.route_layer(middleware::from_fn_with_state(
            Arc::<str>::from(token),
            auth::require_bearer,
        )),
        None => send,
    };
    Router::new()
        .route("/health", get(health))
        .merge(send)
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn send(
    State(state): State<Arc<AppState>>,
    JsonBody(request): JsonBody<SendRequest>,
) -> Result<Json<SendResponse>, ApiError> {
    let Mail {
        message_id,
        message,
    } = build_mail(Arc::clone(&state), request).await?;
    let recipients = message.envelope().to().len();
    state.transport.send(message).await?;
    info!(%message_id, recipients, "mail sent");
    Ok(Json(SendResponse { message_id }))
}

async fn build_mail(state: Arc<AppState>, request: SendRequest) -> Result<Mail, ApiError> {
    let mail =
        task::spawn_blocking(move || mail::build(request, state.default_from.as_ref())).await??;
    Ok(mail)
}
