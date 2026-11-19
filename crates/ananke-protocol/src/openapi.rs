//! Dựng tài liệu OpenAPI 3.1 từ các DTO (AD-03, AD-10).
//!
//! `openapi.json` ở gốc repo được sinh từ đây bằng
//! `cargo run -p ananke-protocol -- gen openapi` và **phải commit** cùng PR
//! đổi giao thức. CI chạy lại generator và fail nếu file có diff.
//!
//! Đường dẫn HTTP do `ananke-server` khai báo bằng `utoipa::path` khi router
//! được hiện thực (M1); tài liệu ở M0 gồm `info`, `servers` và toàn bộ schema.

use utoipa::OpenApi;

use crate::error::ProblemDetails;
use crate::event::{EventDto, EventPayloadDto};
use crate::limits::{LimitsDto, PatchLimitsRequest};
use crate::message::{MessageDto, RoleDto};
    use crate::meta::{DeviceProfileDto, MetaResponse, SandboxKindDto};
    use crate::model::{ListModelsResponse, ModelDto, RegisterModelRequest};
    use crate::part::{PartDto, PartKindDto};
use crate::permission::{
    PermissionDecisionDto, PermissionDecisionRequest, PermissionModeDto, PermissionScopeDto,
};
use crate::session::{CreateSessionRequest, SessionDto, SessionStateDto, UpdateSessionRequest};
use crate::turn::{
    CreateTurnRequest, InputPartDto, SteerRequest, TurnAcceptedResponse, TurnDto, TurnStatusDto,
    UsageDto,
};

/// Tài liệu gốc; chỉ cần `ApiDoc::openapi()` để lấy `OpenApi` đầy đủ.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Ananke API",
        version = env!("CARGO_PKG_VERSION"),
        description = "Hợp đồng HTTP/JSON + SSE của Ananke (AD-03). File được sinh từ \
                       crate `ananke-protocol`; không sửa tay (AD-10). Mọi request mang header \
                       `X-Ananke-Protocol: 1`; lỗi dùng RFC 9457 với content type \
                       `application/problem+json`."
    ),
    servers((url = "/api", description = "Base path của mọi endpoint (§5)")),
    components(schemas(
        ProblemDetails,
        MetaResponse,
        DeviceProfileDto,
        SandboxKindDto,
        LimitsDto,
        PatchLimitsRequest,
        SessionDto,
        CreateSessionRequest,
        UpdateSessionRequest,
        SessionStateDto,
        TurnDto,
        TurnStatusDto,
        UsageDto,
        CreateTurnRequest,
        SteerRequest,
        TurnAcceptedResponse,
        InputPartDto,
        PartDto,
        PartKindDto,
        MessageDto,
        RoleDto,
        ModelDto,
        ListModelsResponse,
        RegisterModelRequest,
        PermissionModeDto,
        PermissionDecisionDto,
        PermissionScopeDto,
        PermissionDecisionRequest,
        EventDto,
        EventPayloadDto
    ))
)]
pub struct ApiDoc;

/// Tài liệu OpenAPI của giao thức hiện tại.
pub fn document() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

#[cfg(test)]
mod tests {
    use super::document;

    #[test]
    fn tai_lieu_co_info_va_base_path() {
        let json = serde_json::to_value(document()).unwrap();
        assert_eq!(json["openapi"], "3.1.0");
        assert_eq!(json["info"]["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(json["servers"][0]["url"], "/api");
    }

    #[test]
    fn tai_lieu_khai_bao_du_schema_cot_loi() {
        let json = serde_json::to_value(document()).unwrap();
        let schemas = &json["components"]["schemas"];
        for name in [
            "MetaResponse",
            "LimitsDto",
            "SessionDto",
            "SessionStateDto",
            "TurnDto",
            "PartDto",
            "MessageDto",
            "PermissionDecisionRequest",
            "EventDto",
            "EventPayloadDto",
            "ProblemDetails",
        ] {
            assert!(schemas.get(name).is_some(), "thiếu schema {name}");
        }
        assert_eq!(json["info"]["title"], "Ananke API");
    }
}