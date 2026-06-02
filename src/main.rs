use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use uuid::Uuid;

mod models;
use models::{CreateTodo, Todo};

// Shared application state. `Clone` is cheap: cloning only bumps the `Arc`
// refcount, so every handler shares the *same* underlying map.
#[derive(Clone)]
struct AppState {
    // Arc  -> share one store across many handlers / tasks.
    // Mutex -> serialize access so concurrent requests can't race.
    todos: Arc<Mutex<HashMap<Uuid, Todo>>>,
}

#[tokio::main]
async fn main() {
    let state = AppState {
        todos: Arc::new(Mutex::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/", get(root))
        .route("/todos", get(list_todos))
        .route("/todos/{id}", get(get_todo))
        .route("/create-todos", post(create_todo))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!("Server running on http://127.0.0.1:3000");
    axum::serve(listener, app).await.unwrap();
}

async fn root(State(state): State<AppState>) -> String {
    // Lock the map for the duration of this read. The guard unlocks when it
    // drops at the end of the expression.
    let count = state.todos.lock().unwrap().len();
    format!("{count} todos stored")
}

async fn list_todos(State(state): State<AppState>) -> Json<Vec<Todo>> {
    // `.cloned()` copies each Todo out of the map so we own them in the Vec.
    // That lets the lock guard drop at the end of this line — we don't hold
    // the mutex while Axum serializes the response.
    let todos: Vec<Todo> = state.todos.lock().unwrap().values().cloned().collect();

    // A `Vec<Todo>` serializes straight to a JSON array (`[]` when empty).
    Json(todos)
}

// `Path<Uuid>` parses the `{id}` segment from the URL into a typed `Uuid`.
// If the segment isn't a valid UUID, Axum rejects the request before we run.
async fn get_todo(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Todo>, StatusCode> {
    // `.get()` returns Option<&Todo>; `.cloned()` makes an owned Option<Todo>
    // so the lock can drop at the end of this line.
    let found = state.todos.lock().unwrap().get(&id).cloned();

    // Ok -> 200 with the todo as JSON; Err -> 404 with no body.
    match found {
        Some(todo) => Ok(Json(todo)),
        None => Err(StatusCode::NOT_FOUND),
    }
}

// Extractors run in order: `State` pulls the shared store, then `Json` reads
// the request body into a `CreateTodo`. The body extractor must come last.
async fn create_todo(
    State(state): State<AppState>,
    Json(input): Json<CreateTodo>,
) -> (StatusCode, Json<Todo>) {
    // Build the server-side record from the client's input.
    let todo = Todo {
        id: Uuid::new_v4(),
        title: input.title,
        completed: false,
    };

    // Lock, insert a clone (we still need `todo` to return), then unlock.
    state.todos.lock().unwrap().insert(todo.id, todo.clone());

    // The tuple tells Axum: send 201 with this JSON body.
    (StatusCode::CREATED, Json(todo))
}
