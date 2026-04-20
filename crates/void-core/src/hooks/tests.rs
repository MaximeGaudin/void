use crate::hooks::hook_fs::{
    delete_hook, find_hook, load_hooks, save_hook, slugify, update_hook_enabled,
};
use crate::hooks::model::{Hook, PromptConfig, Trigger};
use crate::hooks::placeholders::expand_placeholders;
use crate::models::Message;

#[test]
fn load_hooks_returns_empty_for_nonexistent_dir() {
    let dir =
        std::env::temp_dir().join(format!("void-hooks-nonexistent-{}", uuid::Uuid::new_v4()));
    assert!(!dir.exists(), "dir should not exist");
    let hooks = load_hooks(&dir);
    assert!(hooks.is_empty());
}

#[test]
fn slugify_basic() {
    assert_eq!(slugify("Gmail Auto-Archive"), "gmail-auto-archive");
    assert_eq!(slugify("  Daily  Digest  "), "daily-digest");
    assert_eq!(slugify("foo_bar__baz"), "foo-bar-baz");
}

#[test]
fn hook_roundtrip() {
    let hook = Hook {
        name: "Test Hook".into(),
        enabled: true,
        max_turns: 5,
        trigger: Trigger::NewMessage {
            connector: Some("gmail".into()),
        },
        prompt: PromptConfig {
            text: "Hello {message_id}".into(),
        },
    };
    let toml_str = toml::to_string_pretty(&hook).unwrap();
    let parsed: Hook = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.name, "Test Hook");
    assert_eq!(parsed.max_turns, 5);
    assert!(
        matches!(parsed.trigger, Trigger::NewMessage { connector: Some(ref c) } if c == "gmail")
    );
}

#[test]
fn schedule_hook_roundtrip() {
    let hook = Hook {
        name: "Daily Digest".into(),
        enabled: true,
        max_turns: 10,
        trigger: Trigger::Schedule {
            cron: "0 9 * * 1-5".into(),
        },
        prompt: PromptConfig {
            text: "Run digest for {today}".into(),
        },
    };
    let toml_str = toml::to_string_pretty(&hook).unwrap();
    let parsed: Hook = toml::from_str(&toml_str).unwrap();
    assert!(matches!(parsed.trigger, Trigger::Schedule { ref cron } if cron == "0 9 * * 1-5"));
}

#[test]
fn expand_placeholders_no_message() {
    let result = expand_placeholders("Today is {today}, now is {now}", None);
    assert!(!result.contains("{today}"));
    assert!(!result.contains("{now}"));
}

#[test]
fn expand_placeholders_keeps_message_tokens_when_no_message() {
    let result =
        expand_placeholders("before {message_id} after {connector} {connection_id}", None);
    assert_eq!(
        result, "before {message_id} after {connector} {connection_id}",
        "message placeholders must remain literal when no Message is supplied"
    );
}

#[test]
fn expand_placeholders_with_message() {
    let msg = Message {
        id: "msg-123".into(),
        conversation_id: "c1".into(),
        connection_id: "acc1".into(),
        connector: "gmail".into(),
        external_id: "ext1".into(),
        sender: "alice@example.com".into(),
        sender_name: None,
        sender_avatar_url: None,
        body: Some("Hello".into()),
        timestamp: 1_700_000_000,
        synced_at: None,
        is_archived: false,
        reply_to_id: None,
        media_type: None,
        metadata: None,
        context_id: None,
        context: None,
    };
    let result = expand_placeholders("ID={message_id} CONN={connector}", Some(&msg));
    assert_eq!(result, "ID=msg-123 CONN=gmail");
}

#[test]
fn save_and_load_hook() {
    let dir = std::env::temp_dir().join(format!("void-hooks-test-{}", uuid::Uuid::new_v4()));
    let hook = Hook {
        name: "My Test Hook".into(),
        enabled: true,
        max_turns: 3,
        trigger: Trigger::NewMessage { connector: None },
        prompt: PromptConfig {
            text: "test".into(),
        },
    };
    save_hook(&dir, &hook).unwrap();
    let loaded = load_hooks(&dir);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "My Test Hook");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn delete_hook_works() {
    let dir = std::env::temp_dir().join(format!("void-hooks-test-{}", uuid::Uuid::new_v4()));
    let hook = Hook {
        name: "To Delete".into(),
        enabled: true,
        max_turns: 3,
        trigger: Trigger::NewMessage { connector: None },
        prompt: PromptConfig {
            text: "test".into(),
        },
    };
    save_hook(&dir, &hook).unwrap();
    assert!(delete_hook(&dir, "To Delete").unwrap());
    assert!(!delete_hook(&dir, "To Delete").unwrap());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn find_hook_works() {
    let dir = std::env::temp_dir().join(format!("void-hooks-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hook = Hook {
        name: "Find Me".into(),
        enabled: true,
        max_turns: 2,
        trigger: Trigger::NewMessage { connector: None },
        prompt: PromptConfig {
            text: "prompt".into(),
        },
    };
    save_hook(&dir, &hook).unwrap();
    let found = find_hook(&dir, "Find Me").expect("hook should exist");
    assert_eq!(found.name, "Find Me");
    assert_eq!(found.max_turns, 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn update_hook_enabled_toggles() {
    let dir = std::env::temp_dir().join(format!("void-hooks-test-{}", uuid::Uuid::new_v4()));
    let hook = Hook {
        name: "Toggle Test".into(),
        enabled: true,
        max_turns: 1,
        trigger: Trigger::NewMessage { connector: None },
        prompt: PromptConfig { text: "x".into() },
    };
    save_hook(&dir, &hook).unwrap();
    assert!(update_hook_enabled(&dir, "Toggle Test", false).unwrap());
    let loaded = find_hook(&dir, "Toggle Test").unwrap();
    assert!(!loaded.enabled);
    assert!(update_hook_enabled(&dir, "Toggle Test", true).unwrap());
    let loaded = find_hook(&dir, "Toggle Test").unwrap();
    assert!(loaded.enabled);
    assert!(!update_hook_enabled(&dir, "Nonexistent", true).unwrap());
    std::fs::remove_dir_all(&dir).ok();
}
