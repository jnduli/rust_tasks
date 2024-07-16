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
use rust_tasks::{storage::storage::TaskStorage, tasks::summary::SummaryConfig};
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

fn app(shared_state: Arc<Mutex<AppState>>) -> Router {
    Router::new()
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
        .with_state(shared_state)
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
    let app = app(shared_state);

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

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

// which calls one of these handlers
async fn root() -> String {
    // Serve html for wesbite that can handle getting and marking a task as done
    "TODO: intent to add some web html end point here".to_string()
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
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let task_storage = state.lock().unwrap();
    let sql_storage = &task_storage.sql_storage;
    let summary_config = match params.get("summary_config") {
        None => SummaryConfig::default(), 
        Some(summary_config) => {
            serde_json::from_str(summary_config)?
        }
    };
    let day_summary = sql_storage.summarize_day(&summary_config)?;
    Ok(Json(json!(day_summary)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, extract::Request, http, Router};
    use sqlite_storage::SQLiteStorage;

    use http_body_util::BodyExt; // for `collect`
    use tower::util::ServiceExt;

    fn test_app() -> Router {
        let sqlite_storage = SQLiteStorage::new(":memory:");
        let insert_query = r#"INSERT INTO tasks (ulid, body, due_utc, closed_utc, modified_utc) VALUES
            ('8vag','follow up wit','2023-08-23 09:01:34',NULL,NULL),
            ('7nx0','deep dive int','2023-08-06 18:46:41',NULL,NULL);
        "#;
        let tags_query = r#"INSERT INTO task_to_tag (ulid, task_ulid, tag) VALUES
            ('abcd', '8vag', 'work'),
            ('7nx0', '8vag', 'meeting');
        "#;
        sqlite_storage.connection.execute(insert_query, ()).unwrap();
        sqlite_storage.connection.execute(tags_query, ()).unwrap();
        let shared_state = Arc::new(Mutex::new(AppState {
            sql_storage: sqlite_storage,
        }));
        app(shared_state)
    }

    #[tokio::test]
    async fn test_get_next_tasks() {
        let app = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/tasks/next/1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&body).unwrap();
        let expected = json!([{"body":"deep dive int","closed_utc":null,"due_utc":"2023-08-06T18:46:41Z","metadata":null,"modified_utc":null,"priority_adjustment":null,"ready_utc":null,"recurrence_duration":null,"tags":null,"ulid":"7nx0","user":null}]);
        assert_eq!(body, expected);
    }
}
