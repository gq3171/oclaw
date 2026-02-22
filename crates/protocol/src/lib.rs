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

    #[test]
    fn test_hello_frame_default() {
        let hello = HelloFrame::default();
        assert_eq!(hello.min_protocol, 1);
        assert_eq!(hello.max_protocol, 1);
        assert_eq!(hello.client.version, "0.1.0");
    }

    #[test]
    fn test_error_details() {
        let error = ErrorDetails::new("NOT_FOUND", "Resource not found");
        assert_eq!(error.code, "NOT_FOUND");
        assert_eq!(error.message, "Resource not found");
        assert!(error.details.is_none());

        let error_with_details = error.with_details(serde_json::json!({"id": 123}));
        assert!(error_with_details.details.is_some());

        let retryable_error =
            ErrorDetails::new("RATE_LIMITED", "Too many requests").retryable(5000);
        assert!(retryable_error.retryable.is_some());
        assert_eq!(retryable_error.retry_after_ms, Some(5000));
    }

    #[test]
    fn test_error_frame_serialization() {
        let error_frame = ErrorFrame {
            frame_type: ErrorFrameType::Error,
            id: "req_123".to_string(),
            error: ErrorDetails::new("INVALID_REQUEST", "Missing required field"),
        };

        let json = serde_json::to_string(&error_frame).unwrap();
        assert!(json.contains("error"));
        assert!(json.contains("INVALID_REQUEST"));
    }

    #[test]
    fn test_session_create_frame() {
        let session_create = SessionCreate {
            frame_type: SessionCreateType::SessionCreate,
            id: "req_456".to_string(),
            params: Some(SessionCreateParams {
                agent_id: "agent_1".to_string(),
                agent_config: None,
                model: Some("gpt-4".to_string()),
                provider: Some("openai".to_string()),
                system_prompt: Some("You are a helpful assistant".to_string()),
                tools: Some(vec!["browser".to_string()]),
                context: None,
            }),
        };

        assert_eq!(session_create.frame_type, SessionCreateType::SessionCreate);
        assert!(session_create.params.is_some());
        assert_eq!(
            session_create.params.as_ref().unwrap().model,
            Some("gpt-4".to_string())
        );
    }

    #[test]
    fn test_session_info() {
        let session = SessionInfo {
            session_id: "sess_123".to_string(),
            agent_id: "agent_1".to_string(),
            status: SessionStatus::Running,
            created_at: 1000000,
            last_activity_at: Some(1000100),
            message_count: Some(10),
        };

        assert_eq!(session.status, SessionStatus::Running);
        assert!(session.last_activity_at.is_some());
    }

    #[test]
    fn test_session_status_serialization() {
        let statuses = vec![
            SessionStatus::Pending,
            SessionStatus::Running,
            SessionStatus::Waiting,
            SessionStatus::Completed,
            SessionStatus::Error,
        ];

        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: SessionStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn test_presence_event() {
        let presence = PresenceEvent {
            user_id: "user_123".to_string(),
            status: PresenceStatus::Online,
            status_text: Some("Available".to_string()),
            roles: Some(vec!["admin".to_string()]),
            scopes: Some(vec!["read".to_string(), "write".to_string()]),
            ts: 1000000,
            instance_id: Some("inst_1".to_string()),
        };

        assert_eq!(presence.status, PresenceStatus::Online);
        assert_eq!(presence.roles.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_presence_status() {
        let statuses = vec![
            PresenceStatus::Online,
            PresenceStatus::Away,
            PresenceStatus::Busy,
            PresenceStatus::Dnd,
            PresenceStatus::Offline,
        ];

        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: PresenceStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn test_message_event_text() {
        let msg = MessageEvent {
            id: "msg_123".to_string(),
            from: "user_1".to_string(),
            to: Some("user_2".to_string()),
            group_id: None,
            content: MessageContent::text("Hello world"),
            ts: 1000000,
            thread_id: None,
            reply_to: None,
            mentions: None,
        };

        if let MessageContent::Text { text } = msg.content {
            assert_eq!(text, "Hello world");
        } else {
            panic!("Expected Text content");
        }
    }

    #[test]
    fn test_message_event_html() {
        let msg = MessageEvent {
            id: "msg_456".to_string(),
            from: "user_1".to_string(),
            to: None,
            group_id: Some("group_1".to_string()),
            content: MessageContent::html("<b>Hello</b>"),
            ts: 1000000,
            thread_id: Some("thread_1".to_string()),
            reply_to: Some("msg_123".to_string()),
            mentions: Some(vec!["user_2".to_string()]),
        };

        assert!(msg.group_id.is_some());
        assert!(msg.thread_id.is_some());
        assert!(msg.mentions.unwrap().len() == 1);
    }

    #[test]
    fn test_tick_event() {
        let tick = TickEvent { ts: 1000000 };
        assert_eq!(tick.ts, 1000000);
    }

    #[test]
    fn test_shutdown_event() {
        let shutdown = ShutdownEvent {
            reason: "Restarting".to_string(),
            restart_expected_ms: Some(5000),
        };

        assert_eq!(shutdown.reason, "Restarting");
        assert_eq!(shutdown.restart_expected_ms, Some(5000));
    }

    #[test]
    fn test_gateway_frame_request() {
        let frame = GatewayFrame::Request(RequestFrame {
            frame_type: RequestFrameType::Req,
            id: "req_123".to_string(),
            method: "session.create".to_string(),
            params: Some(serde_json::json!({"agent_id": "test"})),
        });

        assert_eq!(frame.frame_id(), Some("req_123"));
    }

    #[test]
    fn test_gateway_frame_response() {
        let frame = GatewayFrame::Response(ResponseFrame {
            frame_type: ResponseFrameType::Res,
            id: "req_123".to_string(),
            ok: true,
            payload: Some(serde_json::json!({"result": "ok"})),
            error: None,
        });

        assert_eq!(frame.frame_id(), Some("req_123"));
    }

    #[test]
    fn test_gateway_frame_event() {
        let frame = GatewayFrame::Event(EventFrame {
            frame_type: EventFrameType::Event,
            event: "tick".to_string(),
            payload: Some(serde_json::json!({"ts": 1000})),
            seq: Some(1),
            state_version: None,
        });

        assert_eq!(frame.frame_id(), None);
    }

    #[test]
    fn test_gateway_frame_error() {
        let frame = GatewayFrame::Error(ErrorFrame {
            frame_type: ErrorFrameType::Error,
            id: "req_123".to_string(),
            error: ErrorDetails::new("TEST_ERROR", "Test error message"),
        });

        assert_eq!(frame.frame_id(), Some("req_123"));
    }

    #[test]
    fn test_client_auth_serialization() {
        let auth = ClientAuth {
            token: Some("test_token".to_string()),
            password: None,
        };

        let json = serde_json::to_string(&auth).unwrap();
        assert!(json.contains("test_token"));
    }

    #[test]
    fn test_device_auth_serialization() {
        let device = DeviceAuth {
            id: "device_123".to_string(),
            public_key: "pub_key_abc".to_string(),
            signature: "sig_xyz".to_string(),
            signed_at: 1000000,
            nonce: Some("nonce_123".to_string()),
        };

        let json = serde_json::to_string(&device).unwrap();
        assert!(json.contains("device_123"));
        assert!(json.contains("nonce_123"));
    }

    #[test]
    fn test_snapshot_serialization() {
        let snapshot = Snapshot {
            presence: vec![PresenceEntry {
                host: Some("localhost".to_string()),
                ip: Some("127.0.0.1".to_string()),
                version: Some("1.0.0".to_string()),
                platform: Some("linux".to_string()),
                device_family: None,
                model_identifier: None,
                mode: None,
                last_input_seconds: None,
                reason: None,
                tags: None,
                text: Some("Online".to_string()),
                ts: 1000000,
                device_id: None,
                roles: None,
                scopes: None,
                instance_id: None,
            }],
            health: serde_json::json!({"status": "healthy"}),
            state_version: StateVersion {
                presence: 1,
                health: 1,
            },
            uptime_ms: 3600000,
            config_path: Some("/etc/openclaw".to_string()),
            state_dir: Some("/var/lib/openclaw".to_string()),
            session_defaults: Some(SessionDefaults {
                default_agent_id: "default".to_string(),
                main_key: "key123".to_string(),
                main_session_key: "sess123".to_string(),
                scope: None,
            }),
            auth_mode: Some(AuthMode::Token),
            update_available: None,
        };

        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("presence"));
        assert!(json.contains("healthy"));
        assert!(json.contains("token"));
    }
}
