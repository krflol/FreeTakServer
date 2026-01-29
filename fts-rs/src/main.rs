use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt, AsyncRead, AsyncWrite};
use tokio::sync::broadcast;
use tracing::{info, error, warn};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use quick_xml::Writer;
use quick_xml::events::BytesStart;
use std::io::Cursor;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::Row;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;
use std::collections::HashMap;
use axum::{
    routing::get,
    Router,
    response::{Json, IntoResponse},
};
use axum::extract::{Query, Path, FromRequest, DefaultBodyLimit};
use axum::http::{HeaderMap, StatusCode, Request};
use axum::body::{Body, to_bytes};
use axum::http::header::CONTENT_TYPE;
use serde_json::{Value, json};
use tokio_rustls::rustls::{self, pki_types::CertificateDer, pki_types::PrivateKeyDer};
use tokio_rustls::TlsAcceptor;
use std::fs::File;
use std::io::BufReader;
use rustls_pemfile::{certs, rsa_private_keys, pkcs8_private_keys};
use uuid::Uuid;
use std::time::Duration;

// Helper to load certs
fn load_certs(path: &str) -> std::io::Result<Vec<CertificateDer<'static>>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    certs(&mut reader).collect()
}

// Helper to load private key
fn load_keys(path: &str) -> std::io::Result<PrivateKeyDer<'static>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    
    // Try RSA first
    if let Ok(mut keys) = rsa_private_keys(&mut reader).collect::<Result<Vec<_>, _>>() {
        if !keys.is_empty() {
            return Ok(PrivateKeyDer::from(keys.remove(0)));
        }
    }
    
    // Try PKCS8
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    if let Ok(mut keys) = pkcs8_private_keys(&mut reader).collect::<Result<Vec<_>, _>>() {
         if !keys.is_empty() {
            return Ok(PrivateKeyDer::from(keys.remove(0)));
        }
    }

    Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "No private key found"))
}

use sha2::{Sha256, Digest};

// Helper to hash password
fn hash_password(password: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(password);
    format!("{:x}", hasher.finalize())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ... (logging and db setup)
    tracing_subscriber::fmt::init();
    
    // Database Setup
    let db_url = "sqlite:fts.db?mode=rwc";
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(db_url).await?;

    // Create Schema
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS cot_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
            uid TEXT NOT NULL,
            cot_type TEXT NOT NULL,
            raw_xml BLOB NOT NULL
        )"
    ).execute(&pool).await?;

    // Create Users Schema
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT NOT NULL UNIQUE,
            password TEXT NOT NULL
        )"
    ).execute(&pool).await?;

    // Create Emergency Schema
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS emergencies (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            uid TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            details TEXT NOT NULL,
            lat REAL NOT NULL,
            lon REAL NOT NULL,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
        )"
    ).execute(&pool).await?;

    // Enterprise sync schema
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS enterprise_sync (
            hash TEXT PRIMARY KEY,
            uid TEXT,
            file_name TEXT,
            creator_uid TEXT,
            tool TEXT,
            keywords TEXT,
            mime_type TEXT,
            size INTEGER,
            submitter TEXT,
            privacy INTEGER DEFAULT 0,
            start_time DATETIME DEFAULT CURRENT_TIMESTAMP,
            path TEXT
        )"
    ).execute(&pool).await?;

    info!("Database initialized at {}", db_url);
    
    // Explicitly wrap pool in Arc
    let pool = Arc::new(pool);

    // Broadcast channel for CoT fan-out (include sender id to avoid echo)
    let (tx, _rx) = broadcast::channel::<BroadcastMsg>(100);
    let connection_ids = Arc::new(AtomicU64::new(1));
    let presence_cache: Arc<RwLock<HashMap<String, Vec<u8>>>> = Arc::new(RwLock::new(HashMap::new()));
    let repeat_cache: Arc<RwLock<HashMap<String, Vec<u8>>>> = Arc::new(RwLock::new(HashMap::new()));
    let conn_state: Arc<RwLock<HashMap<u64, ConnState>>> = Arc::new(RwLock::new(HashMap::new()));
    let uid_to_conn: Arc<RwLock<HashMap<String, u64>>> = Arc::new(RwLock::new(HashMap::new()));

    // Periodic rebroadcast of cached presence to keep clients updated without ping/pong.
    let presence_repeat = presence_cache.clone();
    let repeat_repeat = repeat_cache.clone();
    let tx_repeat = tx.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(15));
        loop {
            ticker.tick().await;
            // Re-broadcast presence with refreshed timestamps.
            {
                let cache = presence_repeat.read().await;
                for (_uid, bytes) in cache.iter() {
                    if let Some(updated) = rewrite_event_times(bytes, chrono::Duration::minutes(5)) {
                        let _ = tx_repeat.send(BroadcastMsg { from: 0, bytes: updated });
                    } else {
                        let _ = tx_repeat.send(BroadcastMsg { from: 0, bytes: bytes.clone() });
                    }
                }
            }
            // Re-broadcast repeatable items less frequently (keepalive for points/shapes).
            {
                let cache = repeat_repeat.read().await;
                for (_uid, bytes) in cache.iter() {
                    if let Some(updated) = rewrite_event_times(bytes, chrono::Duration::hours(24)) {
                        let _ = tx_repeat.send(BroadcastMsg { from: 0, bytes: updated });
                    } else {
                        let _ = tx_repeat.send(BroadcastMsg { from: 0, bytes: bytes.clone() });
                    }
                }
            }
        }
    });

    // --- TLS setup for CoT ---
    let certs = load_certs("certs/server.pem")?;
    let key = load_keys("certs/server.key")?;
    let tls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let acceptor = TlsAcceptor::from(Arc::new(tls_config));

    // --- HTTP SERVER (Axum) ---
    let pool_http = pool.clone();
    let tx_api = tx.clone();
    let uid_map_http = uid_to_conn.clone();

    tokio::spawn(async move {
        let app_state = Arc::new(AppState { pool: pool_http, tx: tx_api, uid_to_conn: uid_map_http });

        let app = Router::new()
            .route("/Manage/HealthCheck", get(health_check))
            .route("/Manage/Users", axum::routing::post(create_user))
            .route("/ManageEmergency/getEmergency", get(get_emergencies))
            .route("/ManageEmergency/postEmergency", axum::routing::post(post_emergency))
            .route("/ManageEmergency/deleteEmergency", axum::routing::post(delete_emergency))
            .route("/Marti/api/sync/package", axum::routing::post(upload_package))
            .route("/Marti/api/sync/package/:filename", get(download_package))
            // Enterprise sync (Marti)
            .route("/Marti/sync/upload", axum::routing::post(enterprise_sync_upload))
            .route("/Marti/sync/search", get(enterprise_sync_search))
            .route("/Marti/sync/content", axum::routing::head(enterprise_sync_head))
            .route("/Marti/sync/content", axum::routing::put(enterprise_sync_upload_content))
            .route("/Marti/sync/content", axum::routing::post(enterprise_sync_upload_content))
            .route("/Marti/sync/content", get(enterprise_sync_get_content))
            .route("/Marti/sync/missionupload", axum::routing::put(enterprise_sync_mission_upload))
            .route("/Marti/sync/missionupload", axum::routing::post(enterprise_sync_mission_upload))
            .route("/Marti/sync/missionquery", get(enterprise_sync_mission_query))
            .route("/Marti/api/sync/metadata/:hash/tool", axum::routing::put(enterprise_sync_update_tool))
            .route("/Marti/api/sync/metadata/:hash/tool", get(enterprise_sync_get_tool))
            // Missions
            .route("/Marti/api/missions", get(not_implemented))
            .route("/Marti/api/missions/invitations", get(not_implemented))
            .route("/Marti/api/groups/all", get(not_implemented))
            .route("/Marti/api/missions/:mission_id", axum::routing::put(not_implemented))
            .route("/Marti/api/missions/:mission_id", get(not_implemented))
            .route("/Marti/api/missions/:mission_id/cot", get(not_implemented))
            .route("/Marti/api/missions/:mission_id/contents", axum::routing::put(not_implemented))
            .route("/Marti/api/missions/logs/entries", axum::routing::post(not_implemented))
            .route("/Marti/api/missions/logs/entries", axum::routing::put(not_implemented))
            .route("/Marti/api/missions/logs/entries/:id", axum::routing::delete(not_implemented))
            .route("/Marti/api/missions/logs/entries/:id", get(not_implemented))
            .route("/Marti/api/missions/:missionID/log", get(not_implemented))
            .route("/Marti/api/missions/all/logs", get(not_implemented))
            .route("/Marti/api/missions/:child_mission_id/parent/:parent_mission_id", axum::routing::put(not_implemented))
            .route("/Marti/api/missions/:child_mission_id/parent", axum::routing::delete(not_implemented))
            .route("/Marti/api/missions/:child_mission_id/parent", get(not_implemented))
            .route("/Marti/api/missions/:parent_mission_id/children", get(not_implemented))
            .route("/Marti/api/missions/all/subscriptions", get(not_implemented))
            .route("/Marti/api/missions/:mission_id/subscriptions", get(not_implemented))
            .route("/Marti/api/missions/:mission_id/subscription", axum::routing::put(not_implemented))
            .route("/Marti/api/missions/:mission_id/subscription", axum::routing::delete(not_implemented))
            .route("/Marti/api/missions/:mission_id/subscription", get(not_implemented))
            .route("/Marti/api/missions/:mission_id/subscriptions/roles", get(not_implemented))
            .route("/Marti/api/missions/:mission_id/externaldata", axum::routing::post(not_implemented))
            .route("/Marti/api/missions/:mission_id/changes", get(not_implemented))
            .route("/Marti/api/missions/:mission_id/contents/missionpackage", axum::routing::put(not_implemented))
            .route("/Marti/api/missions/:mission_id/invite/:type/:invitee", axum::routing::put(not_implemented))
            .route("/Marti/api/missions/:mission_id/invite", axum::routing::post(not_implemented))
            // Allow large uploads (ATAK attachments can exceed default limits)
            .layer(DefaultBodyLimit::max(50 * 1024 * 1024))
            .with_state(app_state);

        let http_addr = "0.0.0.0:8080";
        match TcpListener::bind(http_addr).await {
            Ok(listener) => {
                info!("HTTP TAK API listening on {}", http_addr);
                if let Err(e) = axum::serve(listener, app).await {
                    error!("HTTP server error: {}", e);
                }
            }
            Err(e) => error!("Failed to bind HTTP server: {}", e),
        }
    });

    // --- TCP LISTENER (8087) ---
    let addr = "0.0.0.0:8087";
    let listener = TcpListener::bind(addr).await?;

    info!("FreeTakServer-RS (The Parrot) listening on {}", addr);

    // --- TLS LISTENER (8089) ---
    let tls_addr = "0.0.0.0:8089";
    let tls_listener = TcpListener::bind(tls_addr).await?;
    info!("Secure CoT (TLS) listening on {}", tls_addr);

    // Shared state
    let tx_tcp = tx.clone();
    let tx_tls = tx.clone();
    let pool_tcp = pool.clone();
    let pool_tls = pool.clone();
    let presence_tcp = presence_cache.clone();
    let presence_tls = presence_cache.clone();
    let repeat_tcp = repeat_cache.clone();
    let repeat_tls = repeat_cache.clone();
    let state_tcp = conn_state.clone();
    let state_tls = conn_state.clone();
    let uid_map_tcp = uid_to_conn.clone();
    let uid_map_tls = uid_to_conn.clone();
    let ids_tcp = connection_ids.clone();
    let ids_tls = connection_ids.clone();
    // ... (TCP loop)
    tokio::spawn(async move {
        loop {
            if let Ok((socket, peer)) = listener.accept().await {
                 info!("New TCP connection from: {}", peer);
                 let tx = tx_tcp.clone();
                 let rx = tx.subscribe();
                 let pool = pool_tcp.clone(); // Clone Arc
                let presence = presence_tcp.clone();
                let repeat = repeat_tcp.clone();
                let state = state_tcp.clone();
                let uid_map = uid_map_tcp.clone();
                let conn_id = ids_tcp.fetch_add(1, Ordering::Relaxed);
                tokio::spawn(async move {
                    if let Err(e) = process_connection(socket, tx, rx, pool, presence, repeat, state, uid_map, conn_id).await {
                         error!("TCP Connection Error: {}", e);
                    }
                 });
            }
        }
    });

    // Run TLS Loop
    loop {
        if let Ok((socket, peer)) = tls_listener.accept().await {
             info!("New TLS connection from: {}", peer);
             let acceptor = acceptor.clone();
             let tx = tx_tls.clone();
             let rx = tx.subscribe();
             let pool = pool_tls.clone();
             let presence = presence_tls.clone();
             let repeat = repeat_tls.clone();
             let state = state_tls.clone();
             let uid_map = uid_map_tls.clone();
             let conn_id = ids_tls.fetch_add(1, Ordering::Relaxed);
             
             tokio::spawn(async move {
                match acceptor.accept(socket).await {
                    Ok(tls_stream) => {
                        if let Err(e) = process_connection(tls_stream, tx, rx, pool, presence, repeat, state, uid_map, conn_id).await {
                             error!("TLS Connection Error: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("TLS Handshake Error: {}", e);
                    }
                }
             });
        }
    }
}

// ... (handlers)
async fn create_user(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::Json(payload): axum::Json<Value>,
) -> impl IntoResponse {
    let username = payload.get("username").and_then(|v| v.as_str()).unwrap_or("");
    let password = payload.get("password").and_then(|v| v.as_str()).unwrap_or("");
    
    // ... validation ...
    if username.is_empty() || password.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"status": "error", "message": "Missing username or password"}))).into_response();
    }

    let hashed = hash_password(password);

    match sqlx::query("INSERT INTO users (username, password) VALUES (?, ?)")
        .bind(username)
        .bind(hashed)
        .execute(&*state.pool).await 
    {
        Ok(_) => (axum::http::StatusCode::OK, Json(json!({"status": "ok", "message": "User created"}))).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"status": "error", "message": e.to_string()}))).into_response()
    }
}

async fn health_check() -> Json<Value> {
    Json(json!({ "status": "ok", "message": "FTS-RS is running" }))
}

async fn download_package(
    axum::extract::Path(filename): axum::extract::Path<String>
) -> impl IntoResponse {
    let path = std::path::Path::new("datapackages").join(filename);
    match tokio::fs::read(path).await {
        Ok(bytes) => (axum::http::StatusCode::OK, bytes).into_response(),
        Err(_) => (axum::http::StatusCode::NOT_FOUND, "File not found").into_response()
    }
}

// --- Enterprise Sync (Marti) ---
const ENTERPRISE_SYNC_DIR: &str = "datapackages/enterprise_sync";

async fn enterprise_sync_upload(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
    req: Request<Body>,
) -> impl IntoResponse {
    let host = req.headers().get("host").and_then(|v| v.to_str().ok()).unwrap_or("10.8.0.1:8080").to_string();
    match extract_upload(&state, &params, req).await {
        Ok((data, mime_type)) => match save_enterprise_sync_bytes(&state.pool, &params, data, mime_type).await {
            Ok(meta) => {
                maybe_broadcast_fileshare(&state, &params, &meta, Some(host)).await;
                (StatusCode::OK, Json(meta)).into_response()
            }
            Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"status": "error", "message": e}))).into_response(),
        },
        Err(e) => {
            warn!("EnterpriseSync upload error: {}", e);
            (StatusCode::BAD_REQUEST, Json(json!({"status": "error", "message": e}))).into_response()
        }
    }
}

async fn enterprise_sync_upload_content(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
    req: Request<Body>,
) -> impl IntoResponse {
    let host = req.headers().get("host").and_then(|v| v.to_str().ok()).unwrap_or("10.8.0.1:8080").to_string();
    match extract_upload(&state, &params, req).await {
        Ok((data, mime_type)) => match save_enterprise_sync_bytes(&state.pool, &params, data, mime_type).await {
            Ok(meta) => {
                maybe_broadcast_fileshare(&state, &params, &meta, Some(host)).await;
                (StatusCode::OK, Json(json!({"objectid": meta["Hash"]}))).into_response()
            }
            Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"status": "error", "message": e}))).into_response(),
        },
        Err(e) => {
            warn!("EnterpriseSync upload-content error: {}", e);
            (StatusCode::BAD_REQUEST, Json(json!({"status": "error", "message": e}))).into_response()
        }
    }
}

async fn enterprise_sync_search(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let keyword = params.get("keyword").cloned().unwrap_or_else(|| "missionpackage".to_string());
    let tool = params.get("tool").cloned().unwrap_or_else(|| "public".to_string());

    let rows = sqlx::query(
        "SELECT hash, file_name, creator_uid, tool, keywords, mime_type, size, submitter, start_time, privacy
         FROM enterprise_sync WHERE tool = ? AND privacy = 0 AND keywords LIKE ?"
    )
    .bind(&tool)
    .bind(format!("%{}%", keyword))
    .fetch_all(&*state.pool)
    .await;

    match rows {
        Ok(rows) => {
            let mut results = Vec::new();
            for row in rows {
                let hash: String = row.try_get("hash").unwrap_or_default();
                let name: String = row.try_get("file_name").unwrap_or_default();
                let creator_uid: String = row.try_get("creator_uid").unwrap_or_default();
                let keywords_raw: String = row.try_get("keywords").unwrap_or_else(|_| "[]".to_string());
                let keywords: Vec<String> = serde_json::from_str(&keywords_raw).unwrap_or_else(|_| vec![]);
                let mime_type: String = row.try_get("mime_type").unwrap_or_default();
                let size: i64 = row.try_get("size").unwrap_or_default();
                let submitter: String = row.try_get("submitter").unwrap_or_default();
                let start_time: String = row.try_get("start_time").unwrap_or_default();

                results.push(json!({
                    "UID": hash,
                    "Name": name,
                    "Hash": hash,
                    "PrimaryKey": hash,
                    "SubmissionDateTime": start_time,
                    "SubmissionUser": submitter,
                    "CreatorUid": creator_uid,
                    "Keywords": keywords,
                    "MIMEType": mime_type,
                    "Size": size
                }));
            }
            Json(json!({ "resultCount": results.len(), "results": results })).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"status": "error", "message": e.to_string()}))).into_response(),
    }
}

async fn enterprise_sync_head(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let hash = params.get("hash").cloned();
    let uid = params.get("uid").cloned();
    let key = hash.or(uid);
    if key.is_none() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let exists = sqlx::query("SELECT hash FROM enterprise_sync WHERE hash = ?")
        .bind(key.unwrap())
        .fetch_optional(&*state.pool)
        .await;
    match exists {
        Ok(Some(_)) => StatusCode::OK.into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn enterprise_sync_get_content(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let hash = params.get("hash").cloned();
    let uid = params.get("uid").cloned();
    let key = hash.or(uid);
    let Some(key) = key else {
        return StatusCode::BAD_REQUEST.into_response();
    };

    let row = sqlx::query("SELECT path, mime_type, file_name FROM enterprise_sync WHERE hash = ?")
        .bind(&key)
        .fetch_optional(&*state.pool)
        .await;

    match row {
        Ok(Some(row)) => {
            let path: String = row.try_get("path").unwrap_or_default();
            let mime_type: String = row.try_get("mime_type").unwrap_or_else(|_| "application/octet-stream".to_string());
            let file_name: String = row.try_get("file_name").unwrap_or_else(|_| "file".to_string());
            match tokio::fs::read(&path).await {
                Ok(bytes) => {
                    let mut headers = HeaderMap::new();
                    if let Ok(v) = mime_type.parse() {
                        headers.insert("Content-Type", v);
                    }
                    if let Ok(v) = format!("attachment; filename=\"{}\"", file_name).parse() {
                        headers.insert("Content-Disposition", v);
                    }
                    (StatusCode::OK, headers, bytes).into_response()
                }
                Err(_) => StatusCode::NOT_FOUND.into_response(),
            }
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn enterprise_sync_mission_upload(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
    req: Request<Body>,
) -> impl IntoResponse {
    let headers = req.headers().clone();
    let host = headers.get("host").and_then(|v| v.to_str().ok()).unwrap_or("10.8.0.1:8080").to_string();
    let result = match extract_upload(&state, &params, req).await {
        Ok((data, mime_type)) => save_enterprise_sync_bytes(&state.pool, &params, data, mime_type).await,
        Err(e) => {
            warn!("EnterpriseSync mission-upload error: {}", e);
            Err(e)
        }
    };
    match result {
        Ok(meta) => {
            maybe_broadcast_fileshare(&state, &params, &meta, Some(host)).await;
            let hash = meta.get("Hash").and_then(|v| v.as_str()).unwrap_or("");
            let host = headers.get("host").and_then(|v| v.to_str().ok()).unwrap_or("localhost:8080");
            let proto = headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()).unwrap_or("http");
            let url = format!("{}://{}/Marti/api/sync/metadata/{}/tool", proto, host, hash);
            (StatusCode::OK, url).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"status": "error", "message": e}))).into_response(),
    }
}

async fn enterprise_sync_mission_query(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let hash = params.get("hash").cloned();
    let Some(hash) = hash else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let row = sqlx::query("SELECT path, mime_type, file_name FROM enterprise_sync WHERE hash = ?")
        .bind(&hash)
        .fetch_optional(&*state.pool)
        .await;

    match row {
        Ok(Some(row)) => {
            let path: String = row.try_get("path").unwrap_or_default();
            let mime_type: String = row.try_get("mime_type").unwrap_or_else(|_| "application/octet-stream".to_string());
            let file_name: String = row.try_get("file_name").unwrap_or_else(|_| "file".to_string());
            match tokio::fs::read(&path).await {
                Ok(bytes) => {
                    let mut headers = HeaderMap::new();
                    if let Ok(v) = mime_type.parse() {
                        headers.insert("Content-Type", v);
                    }
                    if let Ok(v) = format!("attachment; filename=\"{}\"", file_name).parse() {
                        headers.insert("Content-Disposition", v);
                    }
                    (StatusCode::OK, headers, bytes).into_response()
                }
                Err(_) => StatusCode::NOT_FOUND.into_response(),
            }
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn enterprise_sync_update_tool(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Path(hash): Path<String>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let privacy = if body.as_ref() == b"private" { 1 } else { 0 };
    let res = sqlx::query("UPDATE enterprise_sync SET privacy = ? WHERE hash = ?")
        .bind(privacy)
        .bind(&hash)
        .execute(&*state.pool)
        .await;
    match res {
        Ok(_) => (StatusCode::OK, "Okay").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn enterprise_sync_get_tool(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> impl IntoResponse {
    let row = sqlx::query("SELECT path, mime_type, file_name FROM enterprise_sync WHERE hash = ?")
        .bind(&hash)
        .fetch_optional(&*state.pool)
        .await;
    match row {
        Ok(Some(row)) => {
            let path: String = row.try_get("path").unwrap_or_default();
            let mime_type: String = row.try_get("mime_type").unwrap_or_else(|_| "application/octet-stream".to_string());
            let file_name: String = row.try_get("file_name").unwrap_or_else(|_| "file".to_string());
            match tokio::fs::read(&path).await {
                Ok(bytes) => {
                    let mut headers = HeaderMap::new();
                    if let Ok(v) = mime_type.parse() {
                        headers.insert("Content-Type", v);
                    }
                    if let Ok(v) = format!("attachment; filename=\"{}\"", file_name).parse() {
                        headers.insert("Content-Disposition", v);
                    }
                    (StatusCode::OK, headers, bytes).into_response()
                }
                Err(_) => StatusCode::NOT_FOUND.into_response(),
            }
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn extract_upload(
    state: &Arc<AppState>,
    params: &HashMap<String, String>,
    req: Request<Body>,
) -> Result<(Vec<u8>, String), String> {
    let content_type = req.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let content_length = req.headers().get("content-length").and_then(|v| v.to_str().ok()).unwrap_or("?");
    info!(
        "EnterpriseSync upload: content-type='{}' content-length={} params={:?}",
        content_type, content_length, params
    );
    if content_type.contains("multipart/form-data") {
        let mut multipart = axum::extract::Multipart::from_request(req, state)
            .await
            .map_err(|e| e.to_string())?;
        let result = read_multipart_bytes(&mut multipart).await;
        if let Ok((ref data, ref mime)) = result {
            info!("EnterpriseSync multipart received: bytes={} mime={}", data.len(), mime);
        }
        result
    } else {
        let bytes = to_bytes(req.into_body(), usize::MAX)
            .await
            .map_err(|e| e.to_string())?
            .to_vec();
        if bytes.is_empty() {
            return Err("No file found".to_string());
        }
        let mime = params
            .get("mime")
            .cloned()
            .unwrap_or_else(|| content_type.clone())
            .trim()
            .to_string();
        let mime = if mime.is_empty() {
            "application/octet-stream".to_string()
        } else {
            mime
        };
        info!("EnterpriseSync raw body received: bytes={} mime={}", bytes.len(), mime);
        Ok((bytes, mime))
    }
}

async fn save_enterprise_sync_bytes(
    pool: &SqlitePool,
    params: &HashMap<String, String>,
    data: Vec<u8>,
    mime_type: String,
) -> Result<Value, String> {
    let file_name = params.get("filename").or_else(|| params.get("name")).cloned().unwrap_or_else(|| "file".to_string());
    let creator_uid = params.get("creatorUid").cloned().unwrap_or_default();
    let tool = params.get("tool").cloned().unwrap_or_else(|| "public".to_string());
    let provided_hash = params.get("hash").cloned();
    let hash = provided_hash.unwrap_or_else(|| hash_bytes(&data));
    let size = data.len() as i64;
    let keywords = vec![file_name.clone(), creator_uid.clone(), "missionpackage".to_string()];
    let keywords_json = serde_json::to_string(&keywords).unwrap_or_else(|_| "[]".to_string());
    let submitter = creator_uid.clone();
    let start_time = chrono::Utc::now().to_rfc3339();

    let save_path = std::path::Path::new(ENTERPRISE_SYNC_DIR).join(&hash);
    if let Some(parent) = save_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    tokio::fs::write(&save_path, &data).await.map_err(|e| e.to_string())?;

    sqlx::query(
        "INSERT OR REPLACE INTO enterprise_sync (hash, uid, file_name, creator_uid, tool, keywords, mime_type, size, submitter, privacy, start_time, path)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, COALESCE((SELECT privacy FROM enterprise_sync WHERE hash = ?), 0), ?, ?)"
    )
    .bind(&hash)
    .bind(&hash)
    .bind(&file_name)
    .bind(&creator_uid)
    .bind(&tool)
    .bind(&keywords_json)
    .bind(&mime_type)
    .bind(size)
    .bind(&submitter)
    .bind(&hash)
    .bind(&start_time)
    .bind(save_path.to_string_lossy().to_string())
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    info!("EnterpriseSync stored: hash={} bytes={} mime={} file={}", hash, size, mime_type, file_name);

    Ok(json!({
        "UID": hash,
        "SubmissionDateTime": start_time,
        "MIMEType": mime_type,
        "SubmissionUser": submitter,
        "PrimaryKey": hash,
        "Hash": hash,
        "Name": file_name,
        "Size": size
    }))
}

async fn read_multipart_bytes(multipart: &mut axum::extract::Multipart) -> Result<(Vec<u8>, String), String> {
    let mut fallback: Option<(Vec<u8>, String)> = None;
    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        let name = field.name().unwrap_or("").to_string();
        let has_file_name = field.file_name().is_some();
        let mime = field.content_type().map(|s| s.to_string()).unwrap_or_else(|| "application/octet-stream".to_string());
        let data = field.bytes().await.map_err(|e| e.to_string())?.to_vec();
        if has_file_name || name == "assetfile" {
            return Ok((data, mime));
        }
        if fallback.is_none() && !data.is_empty() {
            fallback = Some((data, mime));
        }
    }
    fallback.ok_or_else(|| "No file found".to_string())
}

fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

async fn maybe_broadcast_fileshare(
    state: &Arc<AppState>,
    params: &HashMap<String, String>,
    meta: &Value,
    server_host_override: Option<String>,
) {
    let broadcast = params.get("broadcast").map(|v| v == "true" || v == "1");
    let creator = params.get("creatorUid").cloned().unwrap_or_default();
    let auto_broadcast = creator.starts_with("ANDROID-");
    let should_broadcast = broadcast.unwrap_or(auto_broadcast);
    if !should_broadcast {
        return;
    }
    let hash = meta.get("Hash").and_then(|v| v.as_str()).unwrap_or("");
    if hash.is_empty() {
        return;
    }
    let name = meta.get("Name").and_then(|v| v.as_str()).unwrap_or("file");
    let size = meta.get("Size").and_then(|v| v.as_i64()).unwrap_or(0);
    let sender_uid = params
        .get("senderUid")
        .or_else(|| params.get("creatorUid"))
        .cloned()
        .unwrap_or_else(|| "server-uid".to_string());
    let sender_callsign = params.get("senderCallsign").cloned().unwrap_or_else(|| "server".to_string());
    let dest_callsign = params.get("dest").or_else(|| params.get("callsign")).cloned();

    let server_host = server_host_override
        .or_else(|| params.get("server").cloned())
        .unwrap_or_else(|| "10.8.0.1:8080".to_string());
    let server_url = format!("http://{}/Marti/api/sync/metadata/{}/tool", server_host, hash);
    let cot = build_fileshare_cot(hash, name, size, &server_url, &sender_uid, &sender_callsign, dest_callsign.as_deref());
    info!("EnterpriseSync broadcast fileshare: hash={} name={} size={} url={}", hash, name, size, server_url);
    let from_conn = {
        let map = state.uid_to_conn.read().await;
        map.get(&sender_uid).copied().unwrap_or(0)
    };
    let _ = state.tx.send(BroadcastMsg { from: from_conn, bytes: cot });
}

fn build_fileshare_cot(
    hash: &str,
    name: &str,
    size: i64,
    sender_url: &str,
    sender_uid: &str,
    sender_callsign: &str,
    dest_callsign: Option<&str>,
) -> Vec<u8> {
    let now = chrono::Utc::now();
    let time = now.to_rfc3339();
    let stale = (now + chrono::Duration::minutes(10)).to_rfc3339();
    let uid = Uuid::new_v4();
    let ack_uid = Uuid::new_v4();
    let filename = if name.to_lowercase().ends_with(".zip") {
        name.to_string()
    } else {
        format!("{name}.zip")
    };
    let filename = xml_escape(&filename);
    let sender_url = xml_escape(sender_url);
    let sender_uid = xml_escape(sender_uid);
    let sender_callsign = xml_escape(sender_callsign);
    let name = xml_escape(name);
    let hash = xml_escape(hash);
    let mut detail = format!(
        r#"<fileshare filename="{filename}" senderUrl="{sender_url}" sizeInBytes="{size}" sha256="{hash}" senderUid="{sender_uid}" senderCallsign="{sender_callsign}" name="{name}"/><ackrequest uid="{ack_uid}" ackrequested="true" tag="{name}"/>"#,
    );
    if let Some(dest) = dest_callsign {
        let dest = xml_escape(dest);
        detail.push_str(&format!(r#"<marti><dest callsign="{dest}"/></marti>"#));
    }
    format!(
        r#"<event version="2.0" uid="{uid}" type="b-f-t-r" time="{time}" start="{time}" stale="{stale}" how="h-e"><point lat="0.0" lon="0.0" hae="0.0" ce="9999999" le="9999999" /><detail>{detail}</detail></event>"#,
    )
    .into_bytes()
}

fn xml_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

async fn not_implemented() -> impl IntoResponse {
    (axum::http::StatusCode::NOT_IMPLEMENTED, Json(json!({"status": "error", "message": "Not implemented"})))
}

// State Struct
struct AppState {
    pool: Arc<SqlitePool>,
    tx: broadcast::Sender<BroadcastMsg>,
    uid_to_conn: Arc<RwLock<HashMap<String, u64>>>,
}

#[derive(Clone, Debug)]
struct BroadcastMsg {
    from: u64,
    bytes: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
struct ConnState {
    uid: Option<String>,
    callsign: Option<String>,
}

async fn get_emergencies(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> impl IntoResponse {
    let recs = sqlx::query("SELECT uid, name, details, lat, lon FROM emergencies")
        .fetch_all(&*state.pool)
        .await;

    match recs {
        Ok(rows) => {
            let mut json_list = Vec::new();
            for row in rows {
                let uid: String = match row.try_get("uid") {
                    Ok(v) => v,
                    Err(e) => {
                        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"status": "error", "message": e.to_string()}))).into_response();
                    }
                };
                let name: String = match row.try_get("name") {
                    Ok(v) => v,
                    Err(e) => {
                        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"status": "error", "message": e.to_string()}))).into_response();
                    }
                };
                let details: String = match row.try_get("details") {
                    Ok(v) => v,
                    Err(e) => {
                        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"status": "error", "message": e.to_string()}))).into_response();
                    }
                };
                let lat: f64 = match row.try_get("lat") {
                    Ok(v) => v,
                    Err(e) => {
                        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"status": "error", "message": e.to_string()}))).into_response();
                    }
                };
                let lon: f64 = match row.try_get("lon") {
                    Ok(v) => v,
                    Err(e) => {
                        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"status": "error", "message": e.to_string()}))).into_response();
                    }
                };

                json_list.push(json!({
                    "uid": uid,
                    "name": name,
                    "type": "9-1-1",
                    "lat": lat,
                    "lon": lon,
                    "details": details
                }));
            }
            Json(json!({ "json_list": json_list })).into_response()
        }
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"status": "error", "message": e.to_string()}))).into_response(),
    }
}

async fn post_emergency(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::Json(payload): axum::Json<Value>,
) -> impl IntoResponse {
    // 1. Extract Info
    let uid = payload.get("uid").and_then(|v| v.as_str()).unwrap_or_else(|| "unknown");
    let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("Alert");
    let lat = payload.get("lat").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let lon = payload.get("lon").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let details = payload.to_string(); // Save full payload

    // 2. Persist
    let res = sqlx::query("INSERT OR REPLACE INTO emergencies (uid, name, details, lat, lon) VALUES (?, ?, ?, ?, ?)")
        .bind(uid)
        .bind(name)
        .bind(&details)
        .bind(lat)
        .bind(lon)
        .execute(&*state.pool).await;

    if let Err(e) = res {
         return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"status": "error", "message": e.to_string()}))).into_response();
    }

    // 3. Broadcast 911 CoT
    // Construct simple 911 CoT XML
    let cot_xml = format!(r#"<?xml version="1.0" standalone="yes"?>
<event version="2.0" uid="{}" type="9-1-1" time="{}" start="{}" stale="{}" how="m-g">
    <point lat="{}" lon="{}" hae="0.0" ce="9999999" le="9999999"/>
    <detail>
        <emergency type="9-1-1" cancel="false"/>
        <contact callsign="{}"/>
    </detail>
</event>"#, 
    uid, 
    chrono::Utc::now().to_rfc3339(), 
    chrono::Utc::now().to_rfc3339(), 
    (chrono::Utc::now() + chrono::Duration::minutes(10)).to_rfc3339(),
    lat, lon, name);

    let _ = state.tx.send(BroadcastMsg { from: 0, bytes: cot_xml.as_bytes().to_vec() });

    Json(json!({ "message": uid })).into_response()
}

async fn delete_emergency(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::Json(payload): axum::Json<Value>,
) -> impl IntoResponse {
    let uid = payload.get("uid").and_then(|v| v.as_str());

    if let Some(uid) = uid {
        // 1. Remove from DB
        let _ = sqlx::query("DELETE FROM emergencies WHERE uid = ?")
            .bind(uid)
            .execute(&*state.pool).await;

        // 2. Broadcast Cancel
        // Assuming we have enough info, or just broadcast a blind cancel for that UID
        // For a proper cancel we need a type="9-1-1" and <emergency cancel="true"/>
        let cot_xml = format!(r#"<?xml version="1.0" standalone="yes"?>
<event version="2.0" uid="{}" type="9-1-1" time="{}" start="{}" stale="{}" how="m-g">
    <point lat="0.0" lon="0.0" hae="0.0" ce="9999999" le="9999999"/>
    <detail>
        <emergency type="9-1-1" cancel="true"/>
    </detail>
</event>"#, 
        uid, 
        chrono::Utc::now().to_rfc3339(), 
        chrono::Utc::now().to_rfc3339(), 
        (chrono::Utc::now() + chrono::Duration::minutes(1)).to_rfc3339());
        
        let _ = state.tx.send(BroadcastMsg { from: 0, bytes: cot_xml.as_bytes().to_vec() });

        Json(json!({ "message": uid })).into_response()
    } else {
         (axum::http::StatusCode::BAD_REQUEST, Json(json!({"status": "error", "message": "Missing uid"}))).into_response()
    }
}

async fn upload_package(
    axum::extract::State(_state): axum::extract::State<Arc<AppState>>,
    mut multipart: axum::extract::Multipart,
) -> impl IntoResponse {
    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        let file_name = if let Some(name) = field.file_name() {
             name.to_string()
        } else {
             continue; 
        };
        
        let save_path = std::path::Path::new("datapackages").join(&file_name);
        
        if let Some(parent) = save_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        if let Ok(mut file) = tokio::fs::File::create(&save_path).await {
            use tokio::io::AsyncWriteExt;
            
            let data_result = field.bytes().await;
            
            match data_result {
                Ok(bytes) => {
                    // Force deref to slice
                    if let Err(e) = file.write_all(&bytes[..]).await {
                         return Json(json!({ "status": "error", "message": e.to_string() })).into_response();
                    }
                }
                Err(e) => {
                     return Json(json!({ "status": "error", "message": e.to_string() })).into_response();
                }
            }
            
            info!("Saved datapackage: {}", file_name);
            return Json(json!({ "status": "ok", "message": "File uploaded", "path": file_name })).into_response();
        }
    }
    Json(json!({ "status": "error", "message": "No file found" })).into_response()
}

async fn process_connection<S>(
    socket: S, 
    tx: broadcast::Sender<BroadcastMsg>, 
    mut rx: broadcast::Receiver<BroadcastMsg>,
    pool: Arc<SqlitePool>,
    presence_cache: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    repeat_cache: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    conn_state: Arc<RwLock<HashMap<u64, ConnState>>>,
    uid_to_conn: Arc<RwLock<HashMap<String, u64>>>,
    conn_id: u64,
) -> Result<(), Box<dyn std::error::Error>> 
where S: AsyncRead + AsyncWrite + Unpin
{
    // peer_addr is specific to TcpStream. For generic streams we might lose this logging detail 
    // or need to pass it in separately. For now, let's omit or pass as arg.
    // Changing signature to accept address string.
    
    let (mut reader, mut writer) = tokio::io::split(socket);
    let mut buf = [0; 4096];
    let mut pending: Vec<u8> = Vec::new();
    let mut last_sent: Option<String> = None;

    let mut replay_sent = false;

    loop {
        tokio::select! {
            result = reader.read(&mut buf) => {
                let n = match result {
                    Ok(n) => n,
                    Err(e) => {
                        let uid = {
                            let state = conn_state.read().await;
                            state.get(&conn_id).and_then(|s| s.uid.clone())
                        };
                        error!(
                            "Read error conn_id={} uid={:?} last_sent={:?}: {}",
                            conn_id,
                            uid,
                            last_sent,
                            e
                        );
                        return Err(e.into());
                    }
                };
                if n == 0 {
                    if let Some(uid) = cleanup_connection(conn_id, &conn_state, &uid_to_conn, &presence_cache).await {
                        let disconnect = build_disconnect_cot(&uid);
                        let _ = tx.send(BroadcastMsg { from: conn_id, bytes: disconnect });
                    }
                    return Ok(());
                }
                
                pending.extend_from_slice(&buf[0..n]);
                let events = extract_cot_events(&mut pending);
                for event in events {
                    let event = strip_xml_declaration(&event);
                    if event.is_empty() {
                        continue;
                    }
                    // Parse and validate CoT (for logging purposes)
                    if let Some((uid, cot_type, callsign, has_contact)) = parse_cot(&event) {
                        if !replay_sent {
                            replay_sent = true;
                            if is_wintak_uid(&uid) {
                                info!("Skipping cached replay for WinTAK uid={}", uid);
                            } else {
                                send_cached_events(
                                    &mut writer,
                                    &presence_cache,
                                    &repeat_cache,
                                    &mut last_sent
                                ).await?;
                            }
                        }
                        // Persist to DB
                        let _ = sqlx::query(
                            "INSERT INTO cot_history (uid, cot_type, raw_xml) VALUES (?, ?, ?)"
                        )
                        .bind(&uid)
                        .bind(&cot_type)
                        .bind(&event)
                        .execute(&*pool).await;

                        if cot_type == "t-x-c-t" {
                            // Reply directly to keep the client connection alive.
                            if is_wintak_uid(&uid) {
                                info!("Skipping takPong for WinTAK uid={}", uid);
                            } else {
                                let pong = build_tak_pong();
                                record_send(&mut writer, &pong, &mut last_sent).await?;
                            }
                            continue;
                        } else if cot_type == "t-x-c-t-r" {
                            // Ignore client pong traffic.
                            continue;
                        } else if is_disconnect_type(&cot_type) {
                            let _ = remove_presence(&uid, &presence_cache).await;
                        } else if is_presence_type(&cot_type) {
                            update_connection_state(conn_id, &uid, callsign.as_deref(), &conn_state, &uid_to_conn).await;
                            let mut cache = presence_cache.write().await;
                            cache.insert(uid.clone(), event.clone());
                        } else if should_repeat_type(&cot_type) {
                            // Cache original event; refresh timestamps only when sending from cache.
                            let mut cache = repeat_cache.write().await;
                            cache.insert(uid.clone(), event.clone());
                            let _ = tx.send(BroadcastMsg { from: conn_id, bytes: event.clone() });
                            continue;
                        } else if has_contact {
                            update_connection_state(conn_id, &uid, callsign.as_deref(), &conn_state, &uid_to_conn).await;
                        }

                        // Broadcast the raw bytes to all other subscribers
                        let _ = tx.send(BroadcastMsg { from: conn_id, bytes: event });
                    }
                }
            }
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        if msg.from != conn_id {
                            record_send(&mut writer, &msg.bytes, &mut last_sent).await?;
                        }
                    }
                    Err(e) => {
                        warn!("Broadcast receive error: {}", e);
                    }
                }
            }
        }
    }
}

fn parse_cot(data: &[u8]) -> Option<(String, String, Option<String>, bool)> {
    parse_cot_internal(data, true)
}

fn parse_cot_silent(data: &[u8]) -> Option<(String, String, Option<String>, bool)> {
    parse_cot_internal(data, false)
}

fn parse_cot_internal(data: &[u8], log_event: bool) -> Option<(String, String, Option<String>, bool)> {
    let mut reader = Reader::from_reader(Cursor::new(data));
    reader.trim_text(true);

    let mut buf = Vec::new();
    let mut cot_type = String::new();
    let mut cot_uid = String::new();
    let mut is_event = false;
    let mut has_contact = false;
    let mut callsign: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if e.name().as_ref() == b"event" {
                    is_event = true;
                    for attr in e.attributes() {
                        if let Ok(attr) = attr {
                            if attr.key.as_ref() == b"type" {
                                if let Ok(val) = attr.decode_and_unescape_value(&reader) {
                                    cot_type = val.into_owned();
                                }
                            }
                            if attr.key.as_ref() == b"uid" {
                                if let Ok(val) = attr.decode_and_unescape_value(&reader) {
                                    cot_uid = val.into_owned();
                                }
                            }
                        }
                    }
                } else if e.name().as_ref() == b"contact" {
                    has_contact = true;
                    for attr in e.attributes() {
                        if let Ok(attr) = attr {
                            if attr.key.as_ref() == b"callsign" {
                                if let Ok(val) = attr.decode_and_unescape_value(&reader) {
                                    callsign = Some(val.into_owned());
                                }
                            }
                        }
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                if e.name().as_ref() == b"contact" {
                    has_contact = true;
                    for attr in e.attributes() {
                        if let Ok(attr) = attr {
                            if attr.key.as_ref() == b"callsign" {
                                if let Ok(val) = attr.decode_and_unescape_value(&reader) {
                                    callsign = Some(val.into_owned());
                                }
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break, 
            _ => (),
        }
        buf.clear();
    }

    if is_event {
        if log_event {
            info!("Parsed CoT Event: UID={} TYPE={}", cot_uid, cot_type);
        }
        return Some((cot_uid, cot_type, callsign, has_contact));
    }
    
    // Return None if not a valid start packet
    None
}

fn is_presence_type(cot_type: &str) -> bool {
    // Presence updates are the "user update" family, not generic a-* points.
    // Keep this aligned with FTS user update handling.
    cot_type.starts_with("a-f-G-")
}

fn is_drop_point_type(cot_type: &str) -> bool {
    matches!(cot_type, "a-h-G" | "a-n-G" | "a-f-G" | "a-u-G")
}

fn is_disconnect_type(cot_type: &str) -> bool {
    cot_type == "t-x-d-d"
}

fn should_repeat_type(cot_type: &str) -> bool {
    cot_type.starts_with("b-m-") || is_drop_point_type(cot_type)
}

fn is_wintak_uid(uid: &str) -> bool {
    // WinTAK on Windows typically uses a SID-form UID.
    uid.starts_with("S-1-5-21-")
}

fn describe_cot_for_log(data: &[u8]) -> String {
    let trimmed = strip_xml_declaration(data);
    if let Some((uid, cot_type, _callsign, _has_contact)) = parse_cot_silent(&trimmed) {
        return format!("type={} uid={} bytes={}", cot_type, uid, trimmed.len());
    }
    let preview_len = 120.min(trimmed.len());
    let preview = String::from_utf8_lossy(&trimmed[..preview_len]);
    format!("bytes={} preview={}", trimmed.len(), preview)
}

fn rewrite_event_times(data: &[u8], stale_after: chrono::Duration) -> Option<Vec<u8>> {
    let now = chrono::Utc::now();
    let time = now.to_rfc3339();
    let stale = (now + stale_after).to_rfc3339();

    let mut reader = Reader::from_reader(Cursor::new(data));
    reader.trim_text(false);
    let mut writer = Writer::new(Vec::new());
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.name().as_ref() == b"event" => {
                let mut new = BytesStart::new("event");
                for attr in e.attributes() {
                    if let Ok(attr) = attr {
                        let key = attr.key.as_ref();
                        if key == b"time" || key == b"start" || key == b"stale" {
                            continue;
                        }
                        new.push_attribute((key, attr.value.as_ref()));
                    }
                }
                new.push_attribute(("time", time.as_str()));
                new.push_attribute(("start", time.as_str()));
                new.push_attribute(("stale", stale.as_str()));
                writer.write_event(Event::Start(new)).ok()?;
            }
            Ok(Event::Empty(e)) if e.name().as_ref() == b"event" => {
                let mut new = BytesStart::new("event");
                for attr in e.attributes() {
                    if let Ok(attr) = attr {
                        let key = attr.key.as_ref();
                        if key == b"time" || key == b"start" || key == b"stale" {
                            continue;
                        }
                        new.push_attribute((key, attr.value.as_ref()));
                    }
                }
                new.push_attribute(("time", time.as_str()));
                new.push_attribute(("start", time.as_str()));
                new.push_attribute(("stale", stale.as_str()));
                writer.write_event(Event::Empty(new)).ok()?;
            }
            Ok(Event::Eof) => break,
            Ok(ev) => {
                writer.write_event(ev).ok()?;
            }
            Err(_) => return None,
        }
        buf.clear();
    }

    Some(writer.into_inner())
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn find_self_closing_event_end(buffer: &[u8]) -> Option<usize> {
    if !buffer.starts_with(b"<event") {
        return None;
    }
    if let Some(gt_idx) = buffer.iter().position(|b| *b == b'>') {
        if gt_idx > 0 && buffer[gt_idx - 1] == b'/' {
            return Some(gt_idx + 1);
        }
    }
    None
}

fn trim_leading_whitespace(data: &mut Vec<u8>) {
    while let Some(first) = data.first() {
        match first {
            b' ' | b'\n' | b'\r' | b'\t' => {
                data.remove(0);
            }
            _ => break,
        }
    }
}

fn strip_xml_declaration(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    trim_leading_whitespace(&mut out);
    if out.starts_with(b"<?xml") {
        if let Some(end) = find_subsequence(&out, b"?>") {
            let end = end + 2;
            out.drain(0..end);
            trim_leading_whitespace(&mut out);
        }
    }
    out
}

async fn send_cached_events<W>(
    writer: &mut W,
    presence_cache: &Arc<RwLock<HashMap<String, Vec<u8>>>>,
    repeat_cache: &Arc<RwLock<HashMap<String, Vec<u8>>>>,
    last_sent: &mut Option<String>,
) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    {
        let cache = presence_cache.read().await;
        for (_uid, bytes) in cache.iter() {
            if let Some(updated) = rewrite_event_times(bytes, chrono::Duration::minutes(5)) {
                record_send(writer, &updated, last_sent).await?;
            } else {
                record_send(writer, bytes, last_sent).await?;
            }
        }
    }
    {
        let cache = repeat_cache.read().await;
        for (_uid, bytes) in cache.iter() {
            if let Some(updated) = rewrite_event_times(bytes, chrono::Duration::hours(24)) {
                record_send(writer, &updated, last_sent).await?;
            } else {
                record_send(writer, bytes, last_sent).await?;
            }
        }
    }
    Ok(())
}

async fn record_send<W>(
    writer: &mut W,
    data: &[u8],
    last_sent: &mut Option<String>,
) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    *last_sent = Some(describe_cot_for_log(data));
    write_event_line(writer, data).await
}

async fn write_event_line<W>(writer: &mut W, data: &[u8]) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    if data.is_empty() {
        return Ok(());
    }
    writer.write_all(data).await?;
    writer.write_all(b"\n").await?;
    Ok(())
}

fn extract_cot_events(buffer: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut events = Vec::new();
    const EVENT_START: &[u8] = b"<event";
    const EVENT_END: &[u8] = b"</event>";

    loop {
        let start = match find_subsequence(buffer, EVENT_START) {
            Some(idx) => idx,
            None => {
                // Keep a small tail in case the start tag is split across reads.
                let keep = EVENT_START.len().saturating_sub(1);
                if buffer.len() > keep {
                    buffer.drain(0..buffer.len() - keep);
                }
                break;
            }
        };

        if start > 0 {
            buffer.drain(0..start);
        }

        if let Some(end_rel) = find_subsequence(buffer, EVENT_END) {
            let end = end_rel + EVENT_END.len();
            let event = buffer[..end].to_vec();
            buffer.drain(0..end);
            events.push(event);
            continue;
        }

        if let Some(end) = find_self_closing_event_end(buffer) {
            let event = buffer[..end].to_vec();
            buffer.drain(0..end);
            events.push(event);
            continue;
        }

        // Need more data to complete the event.
        break;
    }

    events
}

async fn update_connection_state(
    conn_id: u64,
    uid: &str,
    callsign: Option<&str>,
    conn_state: &Arc<RwLock<HashMap<u64, ConnState>>>,
    uid_to_conn: &Arc<RwLock<HashMap<String, u64>>>,
) {
    {
        let mut state = conn_state.write().await;
        let entry = state.entry(conn_id).or_default();
        entry.uid = Some(uid.to_string());
        if let Some(cs) = callsign {
            if !cs.is_empty() {
                entry.callsign = Some(cs.to_string());
            }
        }
    }

    let mut uid_map = uid_to_conn.write().await;
    uid_map.insert(uid.to_string(), conn_id);
}

async fn cleanup_connection(
    conn_id: u64,
    conn_state: &Arc<RwLock<HashMap<u64, ConnState>>>,
    uid_to_conn: &Arc<RwLock<HashMap<String, u64>>>,
    presence_cache: &Arc<RwLock<HashMap<String, Vec<u8>>>>,
) -> Option<String> {
    let uid_opt = {
        let mut state = conn_state.write().await;
        state.remove(&conn_id).and_then(|s| s.uid)
    };

    if let Some(uid) = uid_opt {
        let mut uid_map = uid_to_conn.write().await;
        uid_map.remove(&uid);

        let mut cache = presence_cache.write().await;
        cache.remove(&uid);
        return Some(uid);
    }
    None
}

async fn remove_presence(
    uid: &str,
    presence_cache: &Arc<RwLock<HashMap<String, Vec<u8>>>>,
) -> bool {
    let mut cache = presence_cache.write().await;
    cache.remove(uid).is_some()
}

fn build_disconnect_cot(uid: &str) -> Vec<u8> {
    let now = chrono::Utc::now();
    let time = now.to_rfc3339();
    let stale = (now + chrono::Duration::minutes(1)).to_rfc3339();
    format!(r#"<event version="2.0" uid="{}" type="t-x-d-d" time="{}" start="{}" stale="{}" how="h-g-i-g-o">
    <point lat="0.0" lon="0.0" hae="0.0" ce="9999999" le="9999999"/>
    <detail><link uid="{}"/></detail>
</event>"#, uid, time, time, stale, uid).into_bytes()
}

fn build_tak_pong() -> Vec<u8> {
    // Match FTS-style takPong: no time/start/stale, default point values.
    format!(r#"<event version="2.0" uid="takPong" type="t-x-c-t-r" how="h-g-i-g-o">
    <point lat="0" lon="0" hae="9999999.0" ce="9999999.0" le="9999999.0"/>
</event>"#).into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[test]
    fn parse_cot_extracts_uid_type_callsign() {
        let xml = br#"<?xml version="1.0" standalone="yes"?>
<event version="2.0" uid="Alpha" type="a-f-G-U-C" time="2026-01-01T00:00:00Z" start="2026-01-01T00:00:00Z" stale="2026-01-01T00:10:00Z">
  <point lat="0" lon="0" hae="0" ce="9999999" le="9999999"/>
  <detail>
    <contact callsign="ECHO-1"/>
  </detail>
</event>"#;

        let parsed = parse_cot(xml).expect("parse_cot should return data");
        assert_eq!(parsed.0, "Alpha");
        assert_eq!(parsed.1, "a-f-G-U-C");
        assert_eq!(parsed.2.as_deref(), Some("ECHO-1"));
        assert!(parsed.3);
    }

    #[test]
    fn presence_rules_match_taxonomy() {
        assert!(is_presence_type("a-f-G-U-C"));
        assert!(is_presence_type("a-f-G-U-C-I"));
        assert!(!is_presence_type("a-h-G"));
        assert!(!is_presence_type("a-f-G"));
        assert!(!is_presence_type("b-t-f"));
        assert!(!is_presence_type("t-x-d-d"));
    }

    #[tokio::test]
    async fn presence_cache_sent_after_first_event() {
        let (tx, _rx) = broadcast::channel::<BroadcastMsg>(10);
        let presence_cache: Arc<RwLock<HashMap<String, Vec<u8>>>> = Arc::new(RwLock::new(HashMap::new()));
        let repeat_cache: Arc<RwLock<HashMap<String, Vec<u8>>>> = Arc::new(RwLock::new(HashMap::new()));
        let conn_state: Arc<RwLock<HashMap<u64, ConnState>>> = Arc::new(RwLock::new(HashMap::new()));
        let uid_to_conn: Arc<RwLock<HashMap<String, u64>>> = Arc::new(RwLock::new(HashMap::new()));

        let cached = b"<event uid=\"Presence\" type=\"a-f-G-U-C\"/>";
        {
            let mut cache = presence_cache.write().await;
            cache.insert("Presence".to_string(), cached.to_vec());
        }

        let (server, mut client) = tokio::io::duplex(1024);
        let rx = tx.subscribe();

        let server_task = tokio::spawn(async move {
            let _ = process_connection(
                server,
                tx,
                rx,
                Arc::new(SqlitePoolOptions::new().connect_lazy("sqlite::memory:").unwrap()),
                presence_cache,
                repeat_cache,
                conn_state,
                uid_to_conn,
                1
            ).await;
        });

        // Send a single presence event to trigger replay
        let trigger = b"<event uid=\"Tester\" type=\"a-f-G-U-C\"/>";
        client.write_all(trigger).await.unwrap();

        let mut buf = [0u8; 128];
        let n = client.read(&mut buf).await.unwrap();
        assert!(n > 0);
        assert!(buf[..n].windows(b"uid=\"Presence\"".len()).any(|w| w == b"uid=\"Presence\""));

        drop(client);
        let _ = server_task.await;
    }
}
