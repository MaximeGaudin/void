use super::{handlers::validate_action_json, *};
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: TestCommand,
    }

    #[derive(Debug, clap::Subcommand)]
    enum TestCommand {
        AgentInbox(AgentInboxArgs),
    }

    fn parse(args: &[&str]) -> TestCli {
        TestCli::try_parse_from(args).expect("should parse")
    }

    fn parse_err(args: &[&str]) -> clap::Error {
        TestCli::try_parse_from(args).expect_err("should fail to parse")
    }

    // ---- Submit parsing ----

    #[test]
    fn parse_submit_minimal() {
        let cli = parse(&[
            "test",
            "agent-inbox",
            "submit",
            "--type",
            "fyi",
            "--source",
            "agent",
            "--title",
            "Title",
            "--body",
            "Body",
        ]);
        match cli.command {
            TestCommand::AgentInbox(ref a) => match &a.command {
                AgentInboxCommand::Submit {
                    item_type,
                    source,
                    title,
                    body,
                    priority,
                    ..
                } => {
                    assert_eq!(item_type, "fyi");
                    assert_eq!(source, "agent");
                    assert_eq!(title, "Title");
                    assert_eq!(body, "Body");
                    assert_eq!(priority, "normal");
                }
                other => panic!("expected Submit, got {other:?}"),
            },
        }
    }

    #[test]
    fn parse_submit_with_all_options() {
        let cli = parse(&[
            "test",
            "agent-inbox",
            "submit",
            "--type",
            "action",
            "--callback-id",
            "cb-123",
            "--source",
            "daily-routine",
            "--title",
            "Reply needed",
            "--body",
            "Full body",
            "--priority",
            "high",
            "--action",
            r#"{"command":"reply"}"#,
            "--input-label",
            "Your reply",
        ]);
        match cli.command {
            TestCommand::AgentInbox(ref a) => match &a.command {
                AgentInboxCommand::Submit {
                    item_type,
                    callback_id,
                    priority,
                    action,
                    input_label,
                    ..
                } => {
                    assert_eq!(item_type, "action");
                    assert_eq!(callback_id.as_deref(), Some("cb-123"));
                    assert_eq!(priority, "high");
                    assert!(action.as_ref().unwrap().contains("reply"));
                    assert_eq!(input_label.as_deref(), Some("Your reply"));
                }
                other => panic!("expected Submit, got {other:?}"),
            },
        }
    }

    #[test]
    fn parse_submit_action_and_action_file_conflict() {
        parse_err(&[
            "test",
            "agent-inbox",
            "submit",
            "--type",
            "action",
            "--source",
            "a",
            "--title",
            "t",
            "--body",
            "b",
            "--action",
            "{}",
            "--action-file",
            "path.json",
        ]);
    }

    #[test]
    fn parse_submit_requires_source() {
        parse_err(&[
            "test",
            "agent-inbox",
            "submit",
            "--type",
            "fyi",
            "--title",
            "t",
            "--body",
            "b",
        ]);
    }

    #[test]
    fn parse_submit_requires_title() {
        parse_err(&[
            "test",
            "agent-inbox",
            "submit",
            "--type",
            "fyi",
            "--source",
            "a",
            "--body",
            "b",
        ]);
    }

    #[test]
    fn parse_submit_requires_body() {
        parse_err(&[
            "test",
            "agent-inbox",
            "submit",
            "--type",
            "fyi",
            "--source",
            "a",
            "--title",
            "t",
        ]);
    }

    #[test]
    fn parse_submit_requires_type() {
        parse_err(&[
            "test",
            "agent-inbox",
            "submit",
            "--source",
            "a",
            "--title",
            "t",
            "--body",
            "b",
        ]);
    }

    // ---- List parsing ----

    #[test]
    fn parse_list_defaults() {
        let cli = parse(&["test", "agent-inbox", "list"]);
        match cli.command {
            TestCommand::AgentInbox(ref a) => match &a.command {
                AgentInboxCommand::List {
                    status,
                    item_type,
                    size,
                } => {
                    assert!(status.is_none());
                    assert!(item_type.is_none());
                    assert_eq!(*size, 50);
                }
                other => panic!("expected List, got {other:?}"),
            },
        }
    }

    #[test]
    fn parse_list_with_filters() {
        let cli = parse(&[
            "test",
            "agent-inbox",
            "list",
            "--status",
            "unread",
            "--type",
            "approval",
            "--size",
            "10",
        ]);
        match cli.command {
            TestCommand::AgentInbox(ref a) => match &a.command {
                AgentInboxCommand::List {
                    status,
                    item_type,
                    size,
                } => {
                    assert_eq!(status.as_deref(), Some("unread"));
                    assert_eq!(item_type.as_deref(), Some("approval"));
                    assert_eq!(*size, 10);
                }
                other => panic!("expected List, got {other:?}"),
            },
        }
    }

    // ---- Get parsing ----

    #[test]
    fn parse_get() {
        let cli = parse(&["test", "agent-inbox", "get", "cb-123"]);
        match cli.command {
            TestCommand::AgentInbox(ref a) => match &a.command {
                AgentInboxCommand::Get { callback_id } => {
                    assert_eq!(callback_id, "cb-123");
                }
                other => panic!("expected Get, got {other:?}"),
            },
        }
    }

    #[test]
    fn parse_get_requires_callback_id() {
        parse_err(&["test", "agent-inbox", "get"]);
    }

    // ---- Respond parsing ----

    #[test]
    fn parse_respond() {
        let cli = parse(&[
            "test",
            "agent-inbox",
            "respond",
            "cb-123",
            "--response",
            "approved",
            "--comment",
            "LGTM",
        ]);
        match cli.command {
            TestCommand::AgentInbox(ref a) => match &a.command {
                AgentInboxCommand::Respond {
                    callback_id,
                    response,
                    comment,
                } => {
                    assert_eq!(callback_id, "cb-123");
                    assert_eq!(response, "approved");
                    assert_eq!(comment.as_deref(), Some("LGTM"));
                }
                other => panic!("expected Respond, got {other:?}"),
            },
        }
    }

    #[test]
    fn parse_respond_requires_response_flag() {
        parse_err(&["test", "agent-inbox", "respond", "cb-123"]);
    }

    // ---- Mark-read parsing ----

    #[test]
    fn parse_mark_read() {
        let cli = parse(&["test", "agent-inbox", "mark-read", "cb-123"]);
        match cli.command {
            TestCommand::AgentInbox(ref a) => match &a.command {
                AgentInboxCommand::MarkRead { callback_id } => {
                    assert_eq!(callback_id, "cb-123");
                }
                other => panic!("expected MarkRead, got {other:?}"),
            },
        }
    }

    // ---- Archive parsing ----

    #[test]
    fn parse_archive_single() {
        let cli = parse(&["test", "agent-inbox", "archive", "cb-1"]);
        match cli.command {
            TestCommand::AgentInbox(ref a) => match &a.command {
                AgentInboxCommand::Archive { callback_ids } => {
                    assert_eq!(callback_ids, &["cb-1"]);
                }
                other => panic!("expected Archive, got {other:?}"),
            },
        }
    }

    #[test]
    fn parse_archive_multiple() {
        let cli = parse(&["test", "agent-inbox", "archive", "cb-1", "cb-2", "cb-3"]);
        match cli.command {
            TestCommand::AgentInbox(ref a) => match &a.command {
                AgentInboxCommand::Archive { callback_ids } => {
                    assert_eq!(callback_ids, &["cb-1", "cb-2", "cb-3"]);
                }
                other => panic!("expected Archive, got {other:?}"),
            },
        }
    }

    // ---- Validation tests ----

    #[test]
    fn validate_action_json_valid() {
        assert!(validate_action_json(r#"{"command":"reply","void_message_id":"m1"}"#).is_ok());
    }

    #[test]
    fn validate_action_json_missing_command() {
        let err = validate_action_json(r#"{"void_message_id":"m1"}"#).unwrap_err();
        assert!(err.to_string().contains("command"));
    }

    #[test]
    fn validate_action_json_not_object() {
        let err = validate_action_json(r#"["array"]"#).unwrap_err();
        assert!(err.to_string().contains("object"));
    }

    #[test]
fn validate_action_json_invalid_json() {
    let err = validate_action_json("not json").unwrap_err();
    assert!(err.to_string().contains("invalid"));
}
