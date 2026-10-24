use reqlivo::desktop::dispatch;
use reqlivo::server::AppState;
use reqlivo::store::{CollectionStore, init_collection};
use serde_json::json;

#[tokio::test]
async fn ipc_creates_persists_and_reopens_requests_without_a_listener() {
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::new(init_collection(temp.path(), "IPC").unwrap());
    let created = dispatch(
        state.clone(),
        "/api/requests".into(),
        "POST".into(),
        Some(json!({
            "name": "Echo", "method": "POST", "url": "http://127.0.0.1:18080/echo"
        })),
    )
    .await
    .unwrap();
    assert_eq!(created.status, 200);
    let id = created.data["id"].as_str().unwrap();
    let reopened = CollectionStore::open(temp.path()).unwrap();
    assert_eq!(reopened.request(id).unwrap().file.http.method, "POST");
    let response = dispatch(
        state.clone(),
        format!("/api/requests/{id}"),
        "GET".into(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(response.status, 200);
    let deleted = dispatch(state, format!("/api/requests/{id}"), "DELETE".into(), None)
        .await
        .unwrap();
    assert_eq!(deleted.status, 200);
    assert!(
        CollectionStore::open(temp.path())
            .unwrap()
            .request(id)
            .is_err()
    );
}

#[tokio::test]
async fn ipc_preserves_errors_and_rejects_nonlocal_operations() {
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::new(init_collection(temp.path(), "IPC").unwrap());
    for path in [
        "https://example.com/api/health",
        "//example.com/api/health",
        "/",
        "/app.js",
    ] {
        assert!(
            dispatch(state.clone(), path.into(), "GET".into(), None)
                .await
                .is_err()
        );
    }
    assert!(
        dispatch(state.clone(), "/api/health".into(), "CONNECT".into(), None)
            .await
            .is_err()
    );
    let error = dispatch(
        state.clone(),
        "/api/requests/inexistente".into(),
        "GET".into(),
        None,
    )
    .await
    .unwrap();
    assert!(error.status >= 400);
    assert!(error.data["error"]["message"].is_string());
    let malformed = dispatch(
        state.clone(),
        "/api/requests".into(),
        "POST".into(),
        Some(json!({})),
    )
    .await
    .unwrap();
    assert_eq!(malformed.status, 422);
    let health = dispatch(state, "/api/health".into(), "GET".into(), None)
        .await
        .unwrap();
    assert_eq!(health.data["collection"], "IPC");
}
