//! `GET /api/meta` — phiên bản giao thức và năng lực đo được lúc start.

use ananke_protocol::MetaResponse;
use axum::extract::State;
use axum::Json;

use crate::state::AppState;

pub async fn get_meta(State(state): State<AppState>) -> Json<MetaResponse> {
    Json((*state.meta).clone())
}