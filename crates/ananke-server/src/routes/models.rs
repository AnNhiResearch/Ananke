//! Handler cho `/api/models` (danh sách và đăng ký model động).

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use ananke_protocol::{ListModelsResponse, ModelDto, RegisterModelRequest};
use ananke_provider::ModelSpec;

use crate::state::AppState;

/// GET /api/models: Trả danh sách các models đang được hỗ trợ.
pub async fn list_models(State(state): State<AppState>) -> Json<ListModelsResponse> {
    let registry = state.models.lock().await;
    let list = registry.list_models();
    let models = list
        .into_iter()
        .map(|spec| {
            let name = if spec.name.is_empty() {
                spec.id.clone()
            } else {
                spec.name
            };
            ModelDto {
                id: spec.id,
                name,
                base_url: spec.base_url,
                context_window: spec.context_window,
                max_tokens: spec.max_tokens,
            }
        })
        .collect();

    Json(ListModelsResponse { models })
}

/// POST /api/models: Đăng ký thêm model từ client.
pub async fn register_model(
    State(state): State<AppState>,
    Json(body): Json<RegisterModelRequest>,
) -> impl IntoResponse {
    let spec = ModelSpec {
        id: body.id.clone(),
        name: body.name.unwrap_or_else(|| body.id.clone()),
        base_url: body.base_url,
        api_key: body.api_key,
        api_key_env: body.api_key_env,
        context_window: body.context_window,
        max_tokens: body.max_tokens,
        headers: std::collections::HashMap::new(),
        extra_body: Default::default(),
    };

    let mut registry = state.models.lock().await;
    registry.register(spec);

    StatusCode::CREATED
}
