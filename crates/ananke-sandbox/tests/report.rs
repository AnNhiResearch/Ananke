use ananke_sandbox::{
    detect, device_profile, mem_total_bytes, DeviceProfile, SandboxKind, SandboxReport,
};

#[test]
fn sandbox_kind_serialize_dung_chuoi_api_meta() {
    for (kind, expected) in [
        (SandboxKind::Landlock, "landlock"),
        (SandboxKind::Seatbelt, "seatbelt"),
        (SandboxKind::RestrictedToken, "restricted-token"),
        (SandboxKind::None, "none"),
    ] {
        let json = serde_json::to_value(kind).unwrap();
        assert_eq!(json, serde_json::json!(expected));
        assert_eq!(kind.as_str(), expected);
        let back: SandboxKind = serde_json::from_value(json).unwrap();
        assert_eq!(back, kind);
    }
}

#[test]
fn report_none_co_ly_do_va_can_canh_bao() {
    let report = SandboxReport::none("Termux không có Landlock");
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["kind"], "none");
    assert_eq!(json["mechanisms"], serde_json::json!([]));
    assert_eq!(json["reason"], "Termux không có Landlock");
    assert!(report.needs_warning());
    assert!(!report.is_enforced());
}

#[test]
fn report_khong_co_reason_thi_khong_serialize_field() {
    let report: SandboxReport =
        serde_json::from_value(serde_json::json!({"kind": "landlock", "mechanisms": ["landlock"]}))
            .unwrap();
    assert_eq!(report.kind(), SandboxKind::Landlock);
    assert_eq!(report.mechanisms(), ["landlock"]);
    assert_eq!(report.reason(), None);

    let json = serde_json::to_value(&report).unwrap();
    assert!(json.get("reason").is_none());
}

#[test]
fn detect_tra_ve_bao_cao_nhat_quan() {
    let report = detect();
    assert_eq!(serde_json::to_value(report.kind()).unwrap(), report.kind().as_str());
    assert_eq!(report.needs_warning(), !report.is_enforced());

    if report.is_enforced() {
        assert!(!report.mechanisms().is_empty());
    } else {
        assert_eq!(report.kind(), SandboxKind::None);
        assert!(report.mechanisms().is_empty());
        assert!(report.reason().is_some_and(|reason| !reason.is_empty()));
    }
}

#[test]
fn device_profile_serialize_chuoi_thuong() {
    for (profile, expected) in [
        (DeviceProfile::Constrained, "constrained"),
        (DeviceProfile::Small, "small"),
        (DeviceProfile::Default, "default"),
    ] {
        assert_eq!(serde_json::to_value(profile).unwrap(), serde_json::json!(expected));
        assert_eq!(profile.as_str(), expected);
    }
}

#[test]
fn device_profile_khop_voi_mem_total_doc_duoc() {
    let expected = mem_total_bytes()
        .map(DeviceProfile::from_mem_total)
        .unwrap_or(DeviceProfile::Default);
    assert_eq!(device_profile(), expected);
}