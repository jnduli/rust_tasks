mod config;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{anyhow, Result};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, patch},
    Json, Router,
};
use rust_tasks::storage::storage::TaskStorage;
use rust_tasks::{storage::sqlite_storage, tasks::Task};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::signal;
use tower_http::{timeout::TimeoutLayer, trace::TraceLayer};

struct AppState {
    sql_storage: sqlite_storage::SQLiteStorage,
}

// #[derive(Debug, Serialize, Deserialize)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum TaskStates {
    Done,
    Open,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PatchTask {
    state: TaskStates,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct GenericBody {
    body: String,
}

// Error handline
// Copied from https://github.com//tokio-rs/axum/blob/e3bb7083c886247f4e6931e149ef6067e6b82e1b/examples/anyhow-error-response/src/main.rs#L35

// Make our own error that wraps `anyhow::Error`.
struct AppError(anyhow::Error);

// Tell axum how to convert `AppError` into a response.
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Something went wrong: {}", self.0),
        )
            .into_response()
    }
}

// This enables using `?` on functions that return `Result<_, anyhow::Error>` to turn them into
// `Result<_, AppError>`. That way you don't need to do that manually.
impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();
    // FIXME! Use configs to get the db location
    let tasks_config = config::Config::load(None)?;
    let task_storage = tasks_config.db_connection()?;

    let shared_state = Arc::new(Mutex::new(AppState {
        sql_storage: task_storage,
    }));

    let app = Router::new()
        .route("/", get(root))
        .route("/health", get(get_health))
        .route("/tasks/", get(get_tasks).post(save_task))
        .route("/tasks/:ulid", patch(patch_task).delete(delete_task))
        .route("/tasks/search", get(search_tasks))
        .route("/tasks/next/:count", get(get_next_tasks))
        .route("/tasks/unsafe_query/", get(get_unsafe_query_tasks))
        .route("/tasks/summarize_day/", get(get_day_summary))
        .layer((
            TraceLayer::new_for_http(),
            // Graceful shutdown will wait for outstanding requests to complete. Add a timeout so
            // requests don't hang forever.
            TimeoutLayer::new(Duration::from_secs(10)),
        ))
        .with_state(shared_state);

    let listener = tokio::net::TcpListener::bind(&tasks_config.bind_address)
        .await
        .unwrap();
    // Use logger for this
    println!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

// which calls one of these handlers
async fn root() -> String {
    // Serve html for wesbite that can handle getting and marking a task as done
    "Hello World".to_string()
}

async fn get_health() -> Result<Json<serde_json::Value>, AppError> {
    let mut result = HashMap::new();
    result.insert("running", "ok");
    result.insert("TODO", "add more statuses");
    Ok(Json(json!(result)))
}

async fn get_tasks(
    State(state): State<Arc<Mutex<AppState>>>,
) -> Result<Json<serde_json::Value>, AppError> {
    get_next_tasks(State(state), Path(10)).await
}

async fn get_next_tasks(
    State(state): State<Arc<Mutex<AppState>>>,
    Path(count): Path<usize>,
) -> Result<Json<serde_json::Value>, AppError> {
    let task_storage = state.lock().unwrap();
    let tasks = task_storage.sql_storage.next_tasks(count)?;
    Ok(Json(json!(tasks)))
}

async fn save_task(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(task): Json<Task>,
) -> Result<Json<Value>, AppError> {
    let app_state = state.lock().unwrap();
    app_state.sql_storage.save(&task)?;
    Ok(Json(json!("Success")))
}

async fn patch_task(
    State(state): State<Arc<Mutex<AppState>>>,
    Path(ulid): Path<String>,
    Json(task): Json<Task>,
) -> Result<Json<serde_json::Value>, AppError> {
    let task_storage = state.lock().unwrap();
    if ulid != task.ulid {
        let err = anyhow!("The uilds don't match");
        return Err(AppError(err));
    }
    task_storage.sql_storage.update(&task)?;
    Ok(Json(json!("Successfully updated task")))
}

async fn delete_task(
    State(state): State<Arc<Mutex<AppState>>>,
    Path(ulid): Path<String>,
) -> Result<Json<Value>, AppError> {
    let task_storage = state.lock().unwrap();
    let sql_storage = &task_storage.sql_storage;
    let tasks = sql_storage.search_using_ulid(&ulid)?;

    if tasks.len() != 1 {
        let msg = format!("Expected to get one task but found {}", tasks.len());
        let err = anyhow!(msg);
        return Err(AppError(err));
    }
    sql_storage.delete(&tasks[0])?;
    Ok(Json(json!("Successfully deleted")))
}

async fn search_tasks(
    State(state): State<Arc<Mutex<AppState>>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let task_storage = state.lock().unwrap();
    let sql_storage = &task_storage.sql_storage;
    match params.get("ulid") {
        None => {
            let err = anyhow!(format!("Expected ulid in params"));
            Err(AppError(err))
        }
        Some(ulid) => {
            let tasks = sql_storage.search_using_ulid(ulid)?;
            Ok(Json(json!(tasks)))
        }
    }
}

async fn get_unsafe_query_tasks(
    State(state): State<Arc<Mutex<AppState>>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let task_storage = state.lock().unwrap();
    let sql_storage = &task_storage.sql_storage;
    match params.get("clause") {
        None => {
            let err = anyhow!(format!("Expected clause in params"));
            Err(AppError(err))
        }
        Some(clause) => {
            let tasks = sql_storage.unsafe_query(clause)?;
            Ok(Json(json!(tasks)))
        }
    }
}

async fn get_day_summary(
    State(state): State<Arc<Mutex<AppState>>>,
) -> Result<Json<Value>, AppError> {
    let task_storage = state.lock().unwrap();
    let sql_storage = &task_storage.sql_storage;
    let day_summary = sql_storage.summarize_day()?;
    Ok(Json(json!(day_summary)))
}
