use rm_client_sync::echo_body;
use serde_json::json;

#[test]
fn echo_body_preserves_unicode_and_newlines() {
    let body = echo_body("你好\nRust".to_owned());

    assert_eq!(body, json!({"text": "你好\nRust"}));
}
