pub mod frames;
pub mod primitives;
pub mod snapshot;

pub use frames::*;
pub use primitives::*;
pub use snapshot::*;

#[cfg(test)]
mod tests {
    use crate::frames::*;
    use crate::snapshot::*;

    #[test]
    fn test_hello_ok_frame() {
        let hello = HelloOk {
            frame_type: HelloOkType::HelloOk,
            protocol: 1,
            server: ServerInfo {
                version: "0.1.0".to_string(),
                commit: None,
                host: None,
                conn_id: "conn_123".to_string(),
            },
            features: ServerFeatures {
                methods: vec!["session.create".to_string()],
                events: vec!["tick".to_string()],
            },
            snapshot: Snapshot {
                presence: vec![],
                health: serde_json::json!({}),
                state_version: StateVersion {
                    presence: 0,
                    health: 0,
                },
                uptime_ms: 1000,
                config_path: None,
                state_dir: None,
                session_defaults: None,
                auth_mode: Some(AuthMode::None),
                update_available: None,
            },
            canvas_host_url: None,
            auth: None,
            policy: Policy {
                max_payload: 1024,
                max_buffered_bytes: 1024,
                tick_interval_ms: 5000,
            },
        };

        assert_eq!(hello.protocol, 1);
        assert_eq!(hello.server.version, "0.1.0");
    }

    #[test]
    fn test_state_version() {
        let sv = StateVersion {
            presence: 5,
            health: 10,
        };

        assert_eq!(sv.presence, 5);
        assert_eq!(sv.health, 10);
    }

    #[test]
    fn test_auth_mode() {
        let modes = vec![AuthMode::None, AuthMode::Token, AuthMode::Password];

        for mode in modes {
            let json = serde_json::to_string(&mode).unwrap();
            let parsed: AuthMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, parsed);
        }
    }

    #[test]
    fn test_policy() {
        let policy = Policy {
            max_payload: 1024 * 1024,
            max_buffered_bytes: 1024 * 1024,
            tick_interval_ms: 5000,
        };

        assert_eq!(policy.max_payload, 1024 * 1024);
        assert_eq!(policy.max_buffered_bytes, 1024 * 1024);
        assert_eq!(policy.tick_interval_ms, 5000);
    }
}
