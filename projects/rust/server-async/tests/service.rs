use rm_server_async::Service;
use serde_json::{Value, json};

#[test]
fn input_validation_and_baseline() {
    let service = Service::default();
    assert_eq!(
        service.handle("GET", "/ping", &Value::Null, ""),
        (200, json!({"data":"pong"}))
    );
    for body in [
        Value::Null,
        json!([]),
        json!({"username":true,"password":"password1"}),
        json!({"username":"a/b","password":"password1"}),
    ] {
        assert_eq!(service.handle("POST", "/users", &body, "").0, 400);
    }
    assert_eq!(service.handle("GET", "/texts", &Value::Null, "").0, 401);
    assert_eq!(service.handle("GET", "/missing", &Value::Null, "").0, 404);
}

#[test]
fn concurrent_registration_has_one_winner() {
    let service = std::sync::Arc::new(Service::default());
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let service = service.clone();
            std::thread::spawn(move || {
                service
                    .handle(
                        "POST",
                        "/users",
                        &json!({"username":"alice","password":"password1"}),
                        "",
                    )
                    .0
            })
        })
        .collect();
    let statuses: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(statuses.iter().filter(|&&s| s == 201).count(), 1);
    assert_eq!(statuses.iter().filter(|&&s| s == 409).count(), 3);
}

#[test]
fn echo_and_text_lifecycle_are_user_scoped() {
    let service = Service::default();
    assert_eq!(
        service.handle("POST", "/echo", &json!({"text":"你好\nRust"}), ""),
        (200, json!({"data":"你好\nRust"}))
    );
    let alice = json!({"username":"alice","password":"password1"});
    let bob = json!({"username":"bob","password":"password1"});
    assert_eq!(service.handle("POST", "/users", &alice, "").0, 201);
    assert_eq!(service.handle("POST", "/users", &bob, "").0, 201);
    let alice_token = service.handle("POST", "/sessions", &alice, "").1["data"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let bob_token = service.handle("POST", "/sessions", &bob, "").1["data"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let alice_auth = format!("Bearer {alice_token}");
    let bob_auth = format!("Bearer {bob_token}");
    assert_eq!(
        service.handle(
            "PUT",
            "/texts/note",
            &json!({"text":"alice text"}),
            &alice_auth
        ),
        (200, json!({"data":null}))
    );
    assert_eq!(
        service.handle("GET", "/texts", &Value::Null, &alice_auth),
        (200, json!({"data":["note"]}))
    );
    assert_eq!(
        service.handle("GET", "/texts/note", &Value::Null, &alice_auth),
        (200, json!({"data":"alice text"}))
    );
    assert_eq!(
        service
            .handle("GET", "/texts/note", &Value::Null, &bob_auth)
            .0,
        404
    );
    assert_eq!(
        service.handle("DELETE", "/texts/note", &Value::Null, &alice_auth),
        (200, json!({"data":null}))
    );
    assert_eq!(
        service
            .handle("DELETE", "/texts/note", &Value::Null, &alice_auth)
            .0,
        404
    );
}

#[test]
fn account_deletion_revokes_token_and_data() {
    let service = Service::default();
    let account = json!({"username":"alice","password":"password1"});
    service.handle("POST", "/users", &account, "");
    let token = service.handle("POST", "/sessions", &account, "").1["data"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let auth = format!("Bearer {token}");
    service.handle("PUT", "/texts/note", &json!({"text":"old"}), &auth);
    assert_eq!(
        service.handle("DELETE", "/users/me", &Value::Null, &auth),
        (200, json!({"data":null}))
    );
    assert_eq!(service.handle("GET", "/texts", &Value::Null, &auth).0, 401);
    assert_eq!(service.handle("POST", "/users", &account, "").0, 201);
    let fresh = service.handle("POST", "/sessions", &account, "").1["data"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        service.handle("GET", "/texts", &Value::Null, &format!("Bearer {fresh}")),
        (200, json!({"data":[]}))
    );
}
