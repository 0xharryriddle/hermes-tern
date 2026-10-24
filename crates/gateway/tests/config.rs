use hermes_tern_gateway::config::{AttachOptions, Limits};
use std::time::Duration;

#[test]
fn sane_defaults_are_finite() {
    assert!(Limits::default().validate().is_ok());
}

#[test]
fn zero_or_huge_limits_are_rejected() {
    for value in [0, usize::MAX] {
        let l = Limits {
            max_frame_bytes: value,
            ..Limits::default()
        };
        assert!(l.validate().is_err());
    }
    let l = Limits {
        request_timeout: Duration::ZERO,
        ..Limits::default()
    };
    assert!(l.validate().is_err());
}

#[test]
fn runtime_paths_must_be_absolute() {
    assert!(AttachOptions::new("relative", "/usr/bin/python3", None).is_err());
    assert!(AttachOptions::new("/opt/hermes", "python3", None).is_err());
    assert!(
        AttachOptions::new("/opt/hermes", "/usr/bin/python3", Some("relative".into())).is_err()
    );
    assert!(AttachOptions::new("/opt/hermes", "/usr/bin/python3", None).is_ok());
}
