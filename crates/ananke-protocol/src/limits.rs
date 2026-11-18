//! `/api/limits` — ngân sách tài nguyên hiệu lực (AD-15).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::meta::DeviceProfileDto;

/// Giới hạn hiệu lực; giá trị mặc định đúng bảng AD-15.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LimitsDto {
    pub max_resident_sessions: u32,
    pub max_parallel_turns: u32,
    pub max_subagents: u32,
    pub max_context_tokens: u32,
    /// Vượt ngưỡng thì spill ra file, part giữ `ref`.
    pub max_part_bytes: u32,
    pub max_event_bytes: u32,
}

impl Default for LimitsDto {
    fn default() -> Self {
        Self {
            max_resident_sessions: 8,
            max_parallel_turns: 2,
            max_subagents: 2,
            max_context_tokens: 48_000,
            max_part_bytes: 256_000,
            max_event_bytes: 64_000,
        }
    }
}

impl LimitsDto {
    /// Giá trị mặc định theo profile; `constrained` hạ song song và context (AD-15).
    pub fn for_profile(profile: DeviceProfileDto) -> Self {
        match profile {
            DeviceProfileDto::Constrained => Self {
                max_resident_sessions: 4,
                max_parallel_turns: 1,
                max_subagents: 1,
                max_context_tokens: 24_000,
                ..Self::default()
            },
            DeviceProfileDto::Small | DeviceProfileDto::Default => Self::default(),
        }
    }
}

/// Body của `PATCH /api/limits`; field vắng mặt nghĩa là giữ nguyên.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchLimitsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_resident_sessions: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_parallel_turns: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_subagents: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_context_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_part_bytes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_event_bytes: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::LimitsDto;
    use crate::meta::DeviceProfileDto;

    #[test]
    fn mac_dinh_dung_bang_ad_15() {
        let limits = LimitsDto::default();
        assert_eq!(limits.max_resident_sessions, 8);
        assert_eq!(limits.max_parallel_turns, 2);
        assert_eq!(limits.max_subagents, 2);
        assert_eq!(limits.max_context_tokens, 48_000);
        assert_eq!(limits.max_part_bytes, 256_000);
        assert_eq!(limits.max_event_bytes, 64_000);
    }

    #[test]
    fn constrained_ha_song_song_va_context() {
        let limits = LimitsDto::for_profile(DeviceProfileDto::Constrained);
        assert_eq!(limits.max_resident_sessions, 4);
        assert_eq!(limits.max_parallel_turns, 1);
        assert_eq!(limits.max_subagents, 1);
        assert_eq!(limits.max_context_tokens, 24_000);
        assert_eq!(limits.max_part_bytes, 256_000);
    }

    #[test]
    fn patch_bo_field_vang_mat() {
        let patch = super::PatchLimitsRequest {
            max_subagents: Some(1),
            ..Default::default()
        };
        let value = serde_json::to_value(patch).unwrap();
        assert_eq!(value["maxSubagents"], 1);
        assert!(value.get("maxContextTokens").is_none());
    }
}