//! Dựng axum Router: probe, OpenAPI và mount toàn bộ `/api` (M1).

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use ananke_protocol::{ProblemDetails, PROBLEM_CONTENT_TYPE};

use crate::error::ApiError;
use crate::routes;
use crate::state::AppState;

pub fn build(state: AppState) -> Router {
    let api = Router::new()
        .route("/meta", get(routes::meta::get_meta))
        .route(
            "/models",
            get(routes::models::list_models).post(routes::models::register_model),
        )
        .route(
            "/limits",
            get(routes::limits::get_limits).patch(routes::limits::patch_limits),
        )
        .route(
            "/session",
            get(routes::session::list_sessions).post(routes::session::create_session),
        )
        .route(
            "/session/{id}",
            get(routes::session::get_session)
                .patch(routes::session::update_session)
                .delete(routes::session::delete_session),
        )
        .route(
            "/session/{id}/state",
            get(routes::session::get_session_state),
        )
        .route("/session/{id}/event", get(routes::sse::events))
        // Alias cho client dùng tên số nhiều; cùng handler nên hành vi y hệt.
        .route("/session/{id}/events", get(routes::sse::events))
        .route("/session/{id}/turn", post(routes::turn::create_turn))
        .route(
            "/session/{id}/turn/{turn_id}/interrupt",
            post(routes::turn::interrupt_turn),
        )
        .route(
            "/session/{id}/turn/{turn_id}/steer",
            post(routes::turn::steer_turn),
        )
        .route(
            "/session/{id}/turn/{turn_id}/resume",
            post(routes::turn::resume_turn),
        )
        .route(
            "/session/{id}/permission/{perm_id}",
            post(routes::permission::resolve_permission),
        );

    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/api/openapi.json", get(openapi))
        .nest("/api", api)
        .fallback(not_found)
        // Layer áp cho mọi route; `/healthz`, `/readyz`, `/api/openapi.json`
        // nằm trong `EXEMPT_PATHS` nên vẫn qua.
        .layer(axum::middleware::from_fn(crate::middleware::protocol_guard))
        .with_state(state)
}

async fn healthz() -> &'static str {
    "ok"
}

/// Readiness = ping DB. Lỗi trả 503 kèm ProblemDetails để probe phân biệt
/// "process sống nhưng chưa phục vụ được" với lỗi request thường.
async fn readyz(State(state): State<AppState>) -> Response {
    match state.store.list_session_meta() {
        Ok(_) => (StatusCode::OK, "ready").into_response(),
        Err(err) => {
            let problem = ProblemDetails::new(
                StatusCode::SERVICE_UNAVAILABLE.as_u16(),
                "Service Unavailable",
            )
            .with_detail(format!("DB chưa sẵn sàng: {err}"))
            .with_code("internal");

            (
                StatusCode::SERVICE_UNAVAILABLE,
                [(header::CONTENT_TYPE, PROBLEM_CONTENT_TYPE)],
                Json(problem),
            )
                .into_response()
        }
    }
}

async fn openapi() -> Json<utoipa::openapi::OpenApi> {
    Json(ananke_protocol::openapi::document())
}

async fn not_found() -> ApiError {
    let problem = ProblemDetails::new(
        StatusCode::NOT_FOUND.as_u16(),
        "Not Found",
    )
    .with_detail("route không tồn tại")
    .with_code("not_found");
    ApiError::NotFound(problem)
}