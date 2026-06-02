use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use tower_http::trace::TraceLayer;
use uuid::Uuid;

mod error;
mod models;

use error::AppError;
use models::{CreateTodo, ListParams, Todo, UpdateTodo};

// Shared application state. `Clone` is cheap: cloning only bumps the `Arc`
// refcount, so every handler shares the *same* underlying map.
#[derive(Clone)]
struct AppState {
    // Arc  -> share one store across many handlers / tasks.
    // Mutex -> serialize access so concurrent requests can't race.
    todos: Arc<Mutex<HashMap<Uuid, Todo>>>,
}

// Build the router (with a fresh, empty store). Factored out of `main` so the
// integration tests can spin up the same app without a live TCP server.
fn app() -> Router {
    let state = AppState {
        todos: Arc::new(Mutex::new(HashMap::new())),
    };

    Router::new()
        .route("/", get(root))
        // One path, several methods — chained on a single MethodRouter.
        .route("/todos", get(list_todos).post(create_todo))
        .route(
            "/todos/{id}",
            get(get_todo)
                .put(update_todo)
                .patch(toggle_todo)
                .delete(delete_todo),
        )
        .with_state(state)
        // TraceLayer logs each request's method, path, and response status.
        .layer(TraceLayer::new_for_http())
}

#[tokio::main]
async fn main() {
    // Initialize logging. Set RUST_LOG to override; default shows our app +
    // tower-http request traces.
    tracing_subscriber::fmt()
        .with_env_filter("axum_todo_api=debug,tower_http=debug")
        .init();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!("Server running on http://127.0.0.1:3000");
    axum::serve(listener, app()).await.unwrap();
}

// Reject titles that are empty or only whitespace. Returns the trimmed,
// validated title so callers store the cleaned-up value.
fn validate_title(title: &str) -> Result<String, AppError> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        Err(AppError::Validation("title must not be empty".to_string()))
    } else {
        Ok(trimmed.to_string())
    }
}

async fn root(State(state): State<AppState>) -> String {
    let count = state.todos.lock().unwrap().len();
    format!("{count} todos stored")
}

// GET /todos  (optionally ?completed=true|false)
async fn list_todos(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Json<Vec<Todo>> {
    let todos: Vec<Todo> = state
        .todos
        .lock()
        .unwrap()
        .values()
        // Keep all when no filter is given; otherwise match the flag.
        .filter(|t| params.completed.map_or(true, |c| t.completed == c))
        .cloned()
        .collect();

    Json(todos)
}

// GET /todos/{id}
async fn get_todo(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Todo>, AppError> {
    let found = state.todos.lock().unwrap().get(&id).cloned();
    found.map(Json).ok_or(AppError::NotFound)
}

// POST /todos
async fn create_todo(
    State(state): State<AppState>,
    Json(input): Json<CreateTodo>,
) -> Result<(StatusCode, Json<Todo>), AppError> {
    let title = validate_title(&input.title)?;

    let todo = Todo {
        id: Uuid::new_v4(),
        title,
        completed: false,
    };

    state.todos.lock().unwrap().insert(todo.id, todo.clone());
    Ok((StatusCode::CREATED, Json(todo)))
}

// PUT /todos/{id} — full replace of title + completed.
async fn update_todo(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateTodo>,
) -> Result<Json<Todo>, AppError> {
    let title = validate_title(&input.title)?;

    let mut map = state.todos.lock().unwrap();
    // `get_mut` borrows the stored todo so we can mutate it in place; `?`
    // short-circuits to 404 if the id is absent.
    let todo = map.get_mut(&id).ok_or(AppError::NotFound)?;
    todo.title = title;
    todo.completed = input.completed;

    Ok(Json(todo.clone()))
}

// PATCH /todos/{id} — flip `completed` (stretch goal).
async fn toggle_todo(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Todo>, AppError> {
    let mut map = state.todos.lock().unwrap();
    let todo = map.get_mut(&id).ok_or(AppError::NotFound)?;
    todo.completed = !todo.completed;

    Ok(Json(todo.clone()))
}

// DELETE /todos/{id} — 204 on success, 404 if it wasn't there.
async fn delete_todo(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    match state.todos.lock().unwrap().remove(&id) {
        Some(_) => Ok(StatusCode::NO_CONTENT),
        None => Err(AppError::NotFound),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use serde_json::{json, Value};
    use tower::ServiceExt; // brings `.oneshot()` onto Router

    // ---- helpers ---------------------------------------------------------

    // Drive one request through the router and return (status, parsed body).
    // Empty bodies (e.g. 204) come back as `Value::Null`.
    async fn send(app: &Router, req: Request<Body>) -> (StatusCode, Value) {
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        // Extractor rejections (bad JSON, bad UUID, bad query) return a
        // plain-text body, so fall back to Null when it isn't JSON.
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, body)
    }

    // Build a request with an optional JSON body (sets content-type when present).
    fn req(method: &str, uri: &str, body: Option<Value>) -> Request<Body> {
        let builder = Request::builder().method(method).uri(uri);
        match body {
            Some(v) => builder
                .header("content-type", "application/json")
                .body(Body::from(v.to_string()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        }
    }

    // Create a todo and return its id (asserts the happy path).
    async fn create(app: &Router, title: &str) -> String {
        let (status, body) = send(app, req("POST", "/todos", Some(json!({ "title": title })))).await;
        assert_eq!(status, StatusCode::CREATED);
        body["id"].as_str().unwrap().to_string()
    }

    // A syntactically valid UUID that won't exist in a fresh store.
    const MISSING_ID: &str = "00000000-0000-0000-0000-000000000000";

    // ---- create (POST /todos) -------------------------------------------

    #[tokio::test]
    async fn create_returns_201_with_generated_fields() {
        let app = app();
        let (status, body) = send(&app, req("POST", "/todos", Some(json!({ "title": "buy milk" })))).await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["title"], "buy milk");
        assert_eq!(body["completed"], false);
        // id must be a valid UUID.
        assert!(Uuid::parse_str(body["id"].as_str().unwrap()).is_ok());
    }

    #[tokio::test]
    async fn create_trims_surrounding_whitespace() {
        let app = app();
        let (status, body) = send(&app, req("POST", "/todos", Some(json!({ "title": "  spaced  " })))).await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["title"], "spaced");
    }

    #[tokio::test]
    async fn create_rejects_empty_title() {
        let app = app();
        let (status, body) = send(&app, req("POST", "/todos", Some(json!({ "title": "" })))).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "title must not be empty");
    }

    #[tokio::test]
    async fn create_rejects_whitespace_only_title() {
        let app = app();
        let (status, body) = send(&app, req("POST", "/todos", Some(json!({ "title": "   \t  " })))).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "title must not be empty");
    }

    #[tokio::test]
    async fn invalid_input_does_not_store_anything() {
        let app = app();
        let _ = send(&app, req("POST", "/todos", Some(json!({ "title": "  " })))).await;

        let (_, list) = send(&app, req("GET", "/todos", None)).await;
        assert_eq!(list.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn create_missing_title_field_is_422() {
        let app = app();
        // Valid JSON, but no `title` -> serde data error -> 422.
        let (status, _) = send(&app, req("POST", "/todos", Some(json!({ "name": "oops" })))).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn create_wrong_title_type_is_422() {
        let app = app();
        let (status, _) = send(&app, req("POST", "/todos", Some(json!({ "title": 123 })))).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn create_malformed_json_is_400() {
        let app = app();
        let request = Request::builder()
            .method("POST")
            .uri("/todos")
            .header("content-type", "application/json")
            .body(Body::from("{ not valid json"))
            .unwrap();
        let (status, _) = send(&app, request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_without_content_type_is_415() {
        let app = app();
        // Body present but no content-type header -> Json extractor rejects.
        let request = Request::builder()
            .method("POST")
            .uri("/todos")
            .body(Body::from(json!({ "title": "x" }).to_string()))
            .unwrap();
        let (status, _) = send(&app, request).await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    // ---- read (GET /todos, GET /todos/{id}) ------------------------------

    #[tokio::test]
    async fn list_is_empty_array_initially() {
        let app = app();
        let (status, body) = send(&app, req("GET", "/todos", None)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!([]));
    }

    #[tokio::test]
    async fn root_reports_count() {
        let app = app();
        // `root` returns plain text, not JSON, so read the body as a string.
        let res = app
            .clone()
            .oneshot(req("GET", "/", None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&bytes[..], b"0 todos stored");

        // After creating one, the count reflects it.
        create(&app, "one").await;
        let res = app.clone().oneshot(req("GET", "/", None)).await.unwrap();
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&bytes[..], b"1 todos stored");
    }

    #[tokio::test]
    async fn create_then_get_round_trips() {
        let app = app();
        let id = create(&app, "write tests").await;

        let (status, body) = send(&app, req("GET", &format!("/todos/{id}"), None)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["id"].as_str().unwrap(), id);
        assert_eq!(body["title"], "write tests");
    }

    #[tokio::test]
    async fn get_missing_id_is_404_with_json_error() {
        let app = app();
        let (status, body) = send(&app, req("GET", &format!("/todos/{MISSING_ID}"), None)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "not found");
    }

    #[tokio::test]
    async fn get_invalid_uuid_is_400() {
        let app = app();
        // Path<Uuid> can't parse "not-a-uuid" -> rejected before the handler.
        let (status, _) = send(&app, req("GET", "/todos/not-a-uuid", None)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    // ---- filtering (GET /todos?completed=) -------------------------------

    #[tokio::test]
    async fn list_filters_by_completed() {
        let app = app();
        let done = create(&app, "done one").await;
        let _open = create(&app, "still open").await;
        // Mark the first one completed.
        let (status, _) = send(&app, req("PATCH", &format!("/todos/{done}"), None)).await;
        assert_eq!(status, StatusCode::OK);

        let (_, all) = send(&app, req("GET", "/todos", None)).await;
        assert_eq!(all.as_array().unwrap().len(), 2);

        let (_, completed) = send(&app, req("GET", "/todos?completed=true", None)).await;
        assert_eq!(completed.as_array().unwrap().len(), 1);
        assert_eq!(completed[0]["id"].as_str().unwrap(), done);

        let (_, open) = send(&app, req("GET", "/todos?completed=false", None)).await;
        assert_eq!(open.as_array().unwrap().len(), 1);
        assert_eq!(open[0]["title"], "still open");
    }

    #[tokio::test]
    async fn list_invalid_completed_value_is_400() {
        let app = app();
        // Query<ListParams> can't parse completed=maybe as a bool.
        let (status, _) = send(&app, req("GET", "/todos?completed=maybe", None)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    // ---- update (PUT /todos/{id}) ---------------------------------------

    #[tokio::test]
    async fn update_replaces_fields() {
        let app = app();
        let id = create(&app, "old title").await;

        let (status, body) = send(
            &app,
            req(
                "PUT",
                &format!("/todos/{id}"),
                Some(json!({ "title": "new title", "completed": true })),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["title"], "new title");
        assert_eq!(body["completed"], true);

        // Confirm it persisted via a follow-up GET.
        let (_, got) = send(&app, req("GET", &format!("/todos/{id}"), None)).await;
        assert_eq!(got["title"], "new title");
        assert_eq!(got["completed"], true);
    }

    #[tokio::test]
    async fn update_missing_id_is_404() {
        let app = app();
        let (status, body) = send(
            &app,
            req(
                "PUT",
                &format!("/todos/{MISSING_ID}"),
                Some(json!({ "title": "x", "completed": false })),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "not found");
    }

    #[tokio::test]
    async fn update_empty_title_is_400() {
        let app = app();
        let id = create(&app, "keep me").await;

        let (status, _) = send(
            &app,
            req(
                "PUT",
                &format!("/todos/{id}"),
                Some(json!({ "title": "  ", "completed": true })),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // The original must be untouched.
        let (_, got) = send(&app, req("GET", &format!("/todos/{id}"), None)).await;
        assert_eq!(got["title"], "keep me");
        assert_eq!(got["completed"], false);
    }

    #[tokio::test]
    async fn update_missing_completed_field_is_422() {
        let app = app();
        let id = create(&app, "t").await;
        let (status, _) = send(
            &app,
            req("PUT", &format!("/todos/{id}"), Some(json!({ "title": "only title" }))),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    // ---- toggle (PATCH /todos/{id}) -------------------------------------

    #[tokio::test]
    async fn toggle_flips_completed_both_ways() {
        let app = app();
        let id = create(&app, "flip me").await;

        let (_, first) = send(&app, req("PATCH", &format!("/todos/{id}"), None)).await;
        assert_eq!(first["completed"], true);

        let (_, second) = send(&app, req("PATCH", &format!("/todos/{id}"), None)).await;
        assert_eq!(second["completed"], false);
    }

    #[tokio::test]
    async fn toggle_missing_id_is_404() {
        let app = app();
        let (status, _) = send(&app, req("PATCH", &format!("/todos/{MISSING_ID}"), None)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    // ---- delete (DELETE /todos/{id}) ------------------------------------

    #[tokio::test]
    async fn delete_removes_and_returns_204() {
        let app = app();
        let id = create(&app, "temporary").await;

        let (status, body) = send(&app, req("DELETE", &format!("/todos/{id}"), None)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(body, Value::Null); // 204 has no body

        // Gone from GET-one and from the list.
        let (status, _) = send(&app, req("GET", &format!("/todos/{id}"), None)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (_, list) = send(&app, req("GET", "/todos", None)).await;
        assert_eq!(list.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn delete_missing_id_is_404() {
        let app = app();
        let (status, body) = send(&app, req("DELETE", &format!("/todos/{MISSING_ID}"), None)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "not found");
    }

    #[tokio::test]
    async fn delete_twice_second_is_404() {
        let app = app();
        let id = create(&app, "delete me twice").await;

        let (first, _) = send(&app, req("DELETE", &format!("/todos/{id}"), None)).await;
        assert_eq!(first, StatusCode::NO_CONTENT);

        let (second, _) = send(&app, req("DELETE", &format!("/todos/{id}"), None)).await;
        assert_eq!(second, StatusCode::NOT_FOUND);
    }

    // ---- full lifecycle --------------------------------------------------

    #[tokio::test]
    async fn full_crud_lifecycle() {
        let app = app();

        // create
        let id = create(&app, "lifecycle").await;
        // update
        let (s, _) = send(
            &app,
            req("PUT", &format!("/todos/{id}"), Some(json!({ "title": "updated", "completed": true }))),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        // get reflects the update
        let (_, got) = send(&app, req("GET", &format!("/todos/{id}"), None)).await;
        assert_eq!(got["title"], "updated");
        assert_eq!(got["completed"], true);
        // delete
        let (s, _) = send(&app, req("DELETE", &format!("/todos/{id}"), None)).await;
        assert_eq!(s, StatusCode::NO_CONTENT);
        // list empty
        let (_, list) = send(&app, req("GET", "/todos", None)).await;
        assert_eq!(list.as_array().unwrap().len(), 0);
    }
}
