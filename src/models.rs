use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: Uuid,
    pub title: String,
    pub completed: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateTodo {
    pub title: String,
}

// Body for a full update (PUT): the client sends the complete new state.
#[derive(Debug, Deserialize)]
pub struct UpdateTodo {
    pub title: String,
    pub completed: bool,
}

// Query string for `GET /todos?completed=true`. `Option` -> the param is
// optional; absent means "no filter, return everything".
#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub completed: Option<bool>,
}