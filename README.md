# axum-todo-api

A small, fully-tested **in-memory REST API** for managing todos, built with [Axum](https://github.com/tokio-rs/axum) 0.8 on top of Tokio. It demonstrates the core building blocks of a Rust web service: shared state, typed extractors, centralized error handling, middleware, input validation, and integration testing — with no database required.

> Data lives in memory and resets on restart. The focus is the HTTP layer, not persistence.

---

## ✨ Features

- **Full CRUD** — create, list, read-one, update, toggle, delete
- **Shared state** via `Arc<Mutex<HashMap<Uuid, Todo>>>` across all handlers
- **Centralized errors** — one `AppError` type renders consistent JSON (`{"error": "..."}`) with correct status codes
- **Request logging** — `tower-http`'s `TraceLayer` logs method, path, and status
- **Input validation** — empty / whitespace-only titles are rejected with `400`
- **Filtering** — `GET /todos?completed=true`
- **26 integration tests** driving the router in-process (no live server)

---

## 🧱 Tech stack

| Concern        | Crate |
|----------------|-------|
| Web framework  | `axum` 0.8 |
| Async runtime  | `tokio` |
| Serialization  | `serde` / `serde_json` |
| IDs            | `uuid` (v4) |
| Middleware     | `tower-http` (trace) |
| Logging        | `tracing` / `tracing-subscriber` |
| Test client    | `tower` (`ServiceExt::oneshot`) |

---

## 🚀 Getting started

```bash
# Run the server (listens on http://127.0.0.1:3000)
cargo run

# Run the test suite
cargo test
```

Logging is on by default. Override the level with `RUST_LOG`:

```bash
RUST_LOG=axum_todo_api=debug,tower_http=trace cargo run
```

---

## 📚 API reference

Base URL: `http://127.0.0.1:3000`

### The `Todo` shape

```json
{
  "id": "f3a1c2d4-...-uuid",
  "title": "buy milk",
  "completed": false
}
```

### Endpoints

| Method   | Path           | Body                          | Success           | Errors |
|----------|----------------|-------------------------------|-------------------|--------|
| `GET`    | `/`            | —                             | `200` count text  | — |
| `GET`    | `/todos`       | —                             | `200` `[Todo]`    | `400` bad `?completed` |
| `GET`    | `/todos/{id}`  | —                             | `200` `Todo`      | `400` bad UUID · `404` |
| `POST`   | `/todos`       | `{ "title" }`                 | `201` `Todo`      | `400` empty title · `415`/`422` bad body |
| `PUT`    | `/todos/{id}`  | `{ "title", "completed" }`    | `200` `Todo`      | `400` empty title · `404` · `422` |
| `PATCH`  | `/todos/{id}`  | —                             | `200` `Todo`      | `404` |
| `DELETE` | `/todos/{id}`  | —                             | `204` no content  | `404` |

Query filter: `GET /todos?completed=true` (or `false`) returns only matching todos.

### Error format

Every handled error returns the same shape:

```json
{ "error": "not found" }
```

| Status | When |
|--------|------|
| `400 Bad Request` | empty/whitespace title, invalid UUID, invalid query value, malformed JSON |
| `404 Not Found`   | no todo with that id |
| `415 Unsupported Media Type` | body sent without `content-type: application/json` |
| `422 Unprocessable Entity`   | valid JSON but wrong/missing fields |

---

## 🧪 Examples

```bash
# Create
curl -s -X POST http://127.0.0.1:3000/todos \
  -H 'content-type: application/json' \
  -d '{"title":"buy milk"}'

ID=$(curl -s -X POST http://127.0.0.1:3000/todos \
  -H 'content-type: application/json' -d '{"title":"write docs"}' | jq -r .id)

# List (and filter)
curl http://127.0.0.1:3000/todos
curl "http://127.0.0.1:3000/todos?completed=false"

# Read one
curl http://127.0.0.1:3000/todos/$ID

# Replace
curl -X PUT http://127.0.0.1:3000/todos/$ID \
  -H 'content-type: application/json' \
  -d '{"title":"write better docs","completed":true}'

# Toggle completed
curl -X PATCH http://127.0.0.1:3000/todos/$ID

# Delete
curl -i -X DELETE http://127.0.0.1:3000/todos/$ID   # -> 204
```

---

## 🗂️ Project layout

```
src/
├── main.rs     # router, handlers, server bootstrap, integration tests
├── models.rs   # Todo, CreateTodo, UpdateTodo, ListParams
└── error.rs    # AppError + IntoResponse (centralized error rendering)
```

### How it fits together

- **`app()`** builds the `Router` with a fresh store and attaches `TraceLayer`. It's factored out of `main` so tests can exercise the exact same app without binding a TCP socket.
- **State** is `Arc<Mutex<HashMap<Uuid, Todo>>>`. `Arc` shares one store across handlers; `Mutex` makes concurrent access safe. Handlers clone values out so the lock releases before the response is serialized.
- **Errors** flow through `Result<_, AppError>`. Axum calls `AppError::into_response()`, so adding a new error variant updates every endpoint at once.

---

## ✅ Testing

The suite uses `tower::ServiceExt::oneshot` to send requests straight through the router as a `tower::Service` — fast, deterministic, and no networking. Coverage includes:

- happy paths for every verb
- validation (empty / whitespace titles)
- `404` for missing ids and idempotent double-delete
- extractor rejections (bad UUID, bad query, malformed JSON, missing content-type)
- `?completed=` filtering
- a full create → update → get → delete lifecycle

```bash
cargo test
```

---

## 🛣️ Possible next steps

- Swap `Mutex` for `RwLock` to allow concurrent reads
- Persist to a database (`sqlx` / SQLite)
- Pagination on `GET /todos`
- OpenAPI docs (`utoipa`)
