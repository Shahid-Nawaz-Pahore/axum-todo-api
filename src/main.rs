use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::{
    extract::State,
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
