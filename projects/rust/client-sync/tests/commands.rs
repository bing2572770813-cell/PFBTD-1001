use rm_client_sync::{echo_body, multiline_text};
use serde_json::json;

#[test]
fn echo_body_preserves_unicode_and_newlines() {
    let body = echo_body("你好\nRust".to_owned());

    assert_eq!(body, json!({"text": "你好\nRust"}));
}

#[test]
fn multiline_input_preserves_empty_lines_trailing_newline_and_literal_terminator() {
    let text = multiline_text([
        "你好".to_owned(),
        "\\.".to_owned(),
        "".to_owned(),
        ".".to_owned(),
    ]);

    assert_eq!(text, "你好\n.\n");
}
