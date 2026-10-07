use reqwest::{Method, blocking::Client};
use serde_json::{Value, json};

/// Build the JSON body required by `POST /echo`.
pub fn echo_body(text: String) -> Value {
    json!({"text": text})
}

/// Turn lines entered with `.` as the terminator into one text value.
/// `\.` represents a literal line containing a period.
pub fn multiline_text<I>(lines: I) -> String
where
    I: IntoIterator<Item = String>,
{
    lines
        .into_iter()
        .take_while(|line| line != ".")
        .map(|line| match line.as_str() {
            "\\." => ".".to_owned(),
            "\\\\" => "\\".to_owned(),
            _ => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Preserve HTTP status even when the error body is not JSON.
pub fn exchange(
    client: &Client,
    url: &str,
    method: Method,
    path: &str,
    token: &str,
    body: Option<&Value>,
) -> Result<(u16, Value), reqwest::Error> {
    let mut request = client.request(method, format!("{}{path}", url.trim_end_matches('/')));
    if !token.is_empty() {
        request = request.bearer_auth(token);
    }
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request.send()?;
    let status = response.status().as_u16();
    let text = response.text()?;
    let value =
        serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({"message": text}));
    Ok((status, value))
}
