pub mod http;

use pbkdf2::pbkdf2_hmac;
use rand::{RngCore, rngs::OsRng};
use serde_json::{Value, json};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;

pub const ROUTES: &[(&str, &str)] = &[
    ("GET", "/ping"),
    ("POST", "/echo"),
    ("POST", "/users"),
    ("POST", "/sessions"),
    ("PUT", "/texts/{name}"),
    ("GET", "/texts"),
    ("GET", "/texts/{name}"),
    ("DELETE", "/texts/{name}"),
    ("DELETE", "/sessions/current"),
    ("DELETE", "/users/me"),
];

fn allowed_methods(path: &str) -> Option<&'static [&'static str]> {
    match path {
        "/ping" => Some(&["GET"]),
        "/echo" => Some(&["POST"]),
        "/users" => Some(&["POST"]),
        "/sessions" => Some(&["POST"]),
        "/sessions/current" => Some(&["DELETE"]),
        "/users/me" => Some(&["DELETE"]),
        "/texts" => Some(&["GET"]),
        path if path
            .strip_prefix("/texts/")
            .is_some_and(|name| !name.is_empty() && !name.contains('/')) =>
        {
            Some(&["PUT", "GET", "DELETE"])
        }
        _ => None,
    }
}

pub fn route_error(method: &str, path: &str) -> Option<u16> {
    let Some(methods) = allowed_methods(path) else {
        return Some(404);
    };
    (!methods.contains(&method)).then_some(405)
}

pub struct User {
    pub salt: [u8; 16],
    pub digest: [u8; 32],
    pub token: Option<String>,
    pub token_expires_at: Option<Instant>,
    pub generation: u64,
    pub texts: BTreeMap<String, String>,
}

pub struct Service {
    pub users: Mutex<BTreeMap<String, User>>,
    token_ttl: Duration,
    next_generation: AtomicU64,
}

impl Default for Service {
    fn default() -> Self {
        Self::with_token_ttl(Duration::from_secs(300))
    }
}

impl Service {
    pub fn with_token_ttl(token_ttl: Duration) -> Self {
        Self {
            users: Mutex::new(BTreeMap::new()),
            token_ttl,
            next_generation: AtomicU64::new(1),
        }
    }
}

pub fn error(status: u16, message: &str) -> (u16, Value) {
    (status, json!({"message": message}))
}

pub fn valid_name(name: &str, max: usize) -> bool {
    !name.is_empty()
        && name.len() <= max
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn valid_text(text: &str) -> bool {
    text.len() <= 65_536
}

fn password_hash(password: &str, salt: &[u8; 16]) -> [u8; 32] {
    let mut output = [0; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, 100_000, &mut output);
    output
}

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn text_name(path: &str) -> Option<&str> {
    path.strip_prefix("/texts/")
}

fn text_from_body(body: &Value) -> Result<&str, u16> {
    let Some(object) = body.as_object() else {
        return Err(400);
    };
    if object.len() != 1 {
        return Err(400);
    }
    let Some(text) = object.get("text").and_then(Value::as_str) else {
        return Err(400);
    };
    valid_text(text).then_some(text).ok_or(413)
}

fn authenticated_name(users: &BTreeMap<String, User>, authorization: &str) -> Option<String> {
    let token = authorization.strip_prefix("Bearer ")?;
    if token.is_empty() {
        return None;
    }
    let now = Instant::now();
    users
        .iter()
        .find(|(_, user)| {
            user.token.as_deref() == Some(token)
                && user.token_expires_at.is_some_and(|expires| expires > now)
        })
        .map(|(name, _)| name.clone())
}

impl Service {
    pub fn handle(
        &self,
        method: &str,
        path: &str,
        body: &Value,
        authorization: &str,
    ) -> (u16, Value) {
        if let Some(status) = route_error(method, path) {
            return error(
                status,
                if status == 404 {
                    "Not found"
                } else {
                    "Method not allowed"
                },
            );
        }
        if method == "GET" && path == "/ping" {
            return (200, json!({"data":"pong"}));
        }
        if method == "POST" && path == "/echo" {
            let text = match text_from_body(body) {
                Ok(text) => text,
                Err(413) => return error(413, "Text too large"),
                Err(_) => return error(400, "Invalid text body"),
            };
            return (200, json!({"data":text}));
        }
        if method == "POST" && matches!(path, "/users" | "/sessions") {
            let Some(name) = body.get("username").and_then(Value::as_str) else {
                return error(400, "Expected username");
            };
            let Some(password) = body.get("password").and_then(Value::as_str) else {
                return error(400, "Expected password");
            };
            if body.as_object().map(|value| value.len()) != Some(2)
                || !valid_name(name, 32)
                || !(8..=128).contains(&password.chars().count())
            {
                return error(400, "Invalid account fields");
            }
            if path == "/users" {
                let mut salt = [0; 16];
                OsRng.fill_bytes(&mut salt);
                let digest = password_hash(password, &salt);
                let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
                let mut users = self.users.lock().unwrap();
                if users.contains_key(name) {
                    return error(409, "Username exists");
                }
                users.insert(
                    name.into(),
                    User {
                        salt,
                        digest,
                        token: None,
                        token_expires_at: None,
                        generation,
                        texts: BTreeMap::new(),
                    },
                );
                return (201, json!({"data":{"username":name}}));
            }
            let (salt, expected, generation) = {
                let users = self.users.lock().unwrap();
                let Some(user) = users.get(name) else {
                    return error(401, "Invalid username or password");
                };
                (user.salt, user.digest, user.generation)
            };
            let digest = password_hash(password, &salt);
            let mut users = self.users.lock().unwrap();
            let Some(user) = users.get_mut(name) else {
                return error(401, "Invalid username or password");
            };
            if user.generation != generation
                || user.salt != salt
                || !bool::from(digest.ct_eq(&expected))
            {
                return error(401, "Invalid username or password");
            }
            let token = new_token();
            user.token = Some(token.clone());
            user.token_expires_at = Some(Instant::now() + self.token_ttl);
            return (
                200,
                json!({"data":{"token":token,"expires_in":self.token_ttl.as_secs()}}),
            );
        }

        let protected = path == "/texts"
            || path == "/sessions/current"
            || path == "/users/me"
            || text_name(path).is_some();
        if protected {
            if let Some(name) = text_name(path) {
                if !valid_name(name, 64) {
                    return error(400, "Invalid text name");
                }
                if method == "PUT" {
                    match text_from_body(body) {
                        Ok(_) => {}
                        Err(413) => return error(413, "Text too large"),
                        Err(_) => return error(400, "Invalid text body"),
                    }
                }
            }
            let mut users = self.users.lock().unwrap();
            let Some(name) = authenticated_name(&users, authorization) else {
                return error(401, "Login required");
            };
            if method == "DELETE" && path == "/sessions/current" {
                let user = users.get_mut(&name).unwrap();
                user.token = None;
                user.token_expires_at = None;
                return (200, json!({"data":null}));
            }
            if method == "DELETE" && path == "/users/me" {
                users.remove(&name);
                return (200, json!({"data":null}));
            }
            let user = users.get_mut(&name).unwrap();
            if method == "GET" && path == "/texts" {
                return (200, json!({"data":user.texts.keys().collect::<Vec<_>>() }));
            }
            if let Some(text_name) = text_name(path) {
                if method == "PUT" {
                    let text = text_from_body(body).unwrap();
                    user.texts.insert(text_name.to_owned(), text.to_owned());
                    return (200, json!({"data":null}));
                }
                if method == "GET" {
                    return user
                        .texts
                        .get(text_name)
                        .map(|text| (200, json!({"data":text})))
                        .unwrap_or_else(|| error(404, "Text not found"));
                }
                if method == "DELETE" {
                    return user
                        .texts
                        .remove(text_name)
                        .map(|_| (200, json!({"data":null})))
                        .unwrap_or_else(|| error(404, "Text not found"));
                }
            }
        }
        error(404, "Not found")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_lifecycle() {
        let service = Service::default();
        let account = json!({"username":"alice", "password":"password1"});
        assert_eq!(service.handle("POST", "/users", &account, "").0, 201);
        assert_eq!(service.handle("POST", "/users", &account, "").0, 409);
        let login = service.handle("POST", "/sessions", &account, "").1;
        let old = format!("Bearer {}", login["data"]["token"].as_str().unwrap());
        let login = service.handle("POST", "/sessions", &account, "").1;
        let current = format!("Bearer {}", login["data"]["token"].as_str().unwrap());
        assert_ne!(old, current);
        assert_eq!(service.handle("GET", "/texts", &Value::Null, &old).0, 401);
        assert_eq!(
            service.handle("GET", "/texts", &Value::Null, &current),
            (200, json!({"data":[]}))
        );
        assert_eq!(
            service
                .handle("DELETE", "/sessions/current", &Value::Null, &current)
                .0,
            200
        );
        assert_eq!(
            service.handle("GET", "/texts", &Value::Null, &current).0,
            401
        );
    }
}
