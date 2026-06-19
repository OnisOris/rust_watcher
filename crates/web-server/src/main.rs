use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path as AxumPath, Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::{Parser, Subcommand};
use futures_util::{SinkExt, StreamExt};
use graph_builder::{
    build_fallback_graph, enrich_file_symbols, filter_snapshot, focus_subgraph,
    push_unique_edge_with_confidence,
};
use graph_core::{
    AnalysisEvent, AnalysisEventType, AnalyzerStatus, AppState, AppStatus, DiagnosticRecord,
    DiagnosticSeverity, EdgeConfidence, EdgeType, FocusDepth, FocusRequest, FocusResponse,
    GraphMode, GraphNode, GraphPatch, GraphSnapshot, LanguageId, NodeDetailsResponse,
    ReferenceRecord, SearchResult, ServerMessage, SourceLocation, SymbolIndex, SymbolKindName,
};
use parking_lot::RwLock;
use project_indexer::{index_project, start_watcher};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::sync::{broadcast, Mutex as AsyncMutex};
use tokio::time::{sleep, timeout, Duration};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};
use url::Url;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "rust-code-command-center")]
#[command(about = "Local browser command center for Rust project graphs")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Serve(ServeArgs),
}

#[derive(Parser, Clone)]
struct ServeArgs {
    #[arg(long)]
    project: Option<PathBuf>,
    #[arg(long, default_value = "127.0.0.1")]
    host: IpAddr,
    #[arg(long, default_value_t = 0)]
    port: u16,
    #[arg(long)]
    open: bool,
    #[arg(long, default_value = "frontend/dist")]
    frontend_dist: PathBuf,
    #[arg(long, default_value = "rust-analyzer")]
    rust_analyzer: PathBuf,
    #[arg(long)]
    enable_editor_open: bool,
}

#[derive(Clone)]
struct AppStateHandle {
    project_root: Arc<RwLock<PathBuf>>,
    graph: Arc<RwLock<GraphSnapshot>>,
    status: Arc<RwLock<AppStatus>>,
    ws_tx: broadcast::Sender<ServerMessage>,
    analyzer: Arc<AnalyzerState>,
    diagnostics_by_file: Arc<RwLock<HashMap<String, Vec<DiagnosticRecord>>>>,
    diagnostics_by_node: Arc<RwLock<HashMap<String, Vec<DiagnosticRecord>>>>,
    watcher: Arc<RwLock<Option<notify::RecommendedWatcher>>>,
    is_indexing: Arc<AtomicBool>,
    enable_editor_open: bool,
}

struct AnalyzerState {
    binary: PathBuf,
    root: RwLock<PathBuf>,
    client: AsyncMutex<Option<ra_client::RaClient>>,
}

#[allow(dead_code)]
impl AnalyzerState {
    async fn set_root(&self, root: PathBuf) {
        *self.root.write() = root;
        let mut client = self.client.lock().await;
        if let Some(client) = client.as_mut() {
            let _ = client.shutdown().await;
        }
        *client = None;
    }

    async fn ensure_started_locked(&self, client: &mut Option<ra_client::RaClient>) -> Result<()> {
        if client.is_some() {
            return Ok(());
        }
        let root = self.root.read().clone();
        let started = timeout(
            Duration::from_secs(8),
            ra_client::RaClient::start(&self.binary, &root),
        )
        .await
        .context("rust-analyzer initialize timed out")??;
        *client = Some(started);
        Ok(())
    }

    pub async fn document_symbols(&self, file: &Path) -> Result<Vec<graph_core::DiscoveredSymbol>> {
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        let result = guard.as_ref().unwrap().document_symbols(file).await;
        if result.is_err() {
            *guard = None;
        }
        result
    }

    pub async fn prepare_call_hierarchy(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<ra_client::LspCallHierarchyItem>> {
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        let result = guard
            .as_ref()
            .unwrap()
            .prepare_call_hierarchy(file, line, character)
            .await;
        if result.is_err() {
            *guard = None;
        }
        result
    }

    pub async fn outgoing_calls(
        &self,
        item: &ra_client::LspCallHierarchyItem,
    ) -> Result<Vec<ra_client::LspCallHierarchyOutgoingCall>> {
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        let result = guard.as_ref().unwrap().outgoing_calls(item).await;
        if result.is_err() {
            *guard = None;
        }
        result
    }

    pub async fn incoming_calls(
        &self,
        item: &ra_client::LspCallHierarchyItem,
    ) -> Result<Vec<ra_client::LspCallHierarchyIncomingCall>> {
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        let result = guard.as_ref().unwrap().incoming_calls(item).await;
        if result.is_err() {
            *guard = None;
        }
        result
    }

    pub async fn references(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<ra_client::LspLocation>> {
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        let result = guard
            .as_ref()
            .unwrap()
            .references(file, line, character)
            .await;
        if result.is_err() {
            *guard = None;
        }
        result
    }

    pub async fn definition(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<ra_client::LspGotoDefinitionResponse>> {
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        let result = guard
            .as_ref()
            .unwrap()
            .definition(file, line, character)
            .await;
        if result.is_err() {
            *guard = None;
        }
        result
    }

    pub async fn type_definition(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<ra_client::LspGotoDefinitionResponse>> {
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        let result = guard
            .as_ref()
            .unwrap()
            .type_definition(file, line, character)
            .await;
        if result.is_err() {
            *guard = None;
        }
        result
    }

    async fn subscribe_notifications(
        &self,
    ) -> Result<broadcast::Receiver<ra_client::LspNotification>> {
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        Ok(guard.as_ref().unwrap().subscribe_notifications())
    }
}

#[derive(Debug, Deserialize)]
struct SnapshotQuery {
    mode: Option<GraphMode>,
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenProjectRequest {
    path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenEditorRequest {
    file: PathBuf,
    line: Option<u32>,
    column: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenEditorResponse {
    editor: String,
    file: String,
    line: Option<u32>,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    ok: bool,
    version: &'static str,
}

#[derive(Debug, Serialize)]
struct SearchResponse {
    results: Vec<SearchResult>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "web_server=info,ra_client=info,project_indexer=info".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Serve(args) => serve(args).await,
    }
}

async fn serve(args: ServeArgs) -> Result<()> {
    if args.host.is_unspecified() {
        warn!(host = %args.host, "explicitly binding to an unspecified address");
    }

    let project_root = args
        .project
        .clone()
        .unwrap_or(std::env::current_dir().context("failed to read current directory")?)
        .canonicalize()
        .context("failed to canonicalize project root")?;

    let initial_status = AppStatus {
        app_state: AppState::Empty,
        analyzer_status: AnalyzerStatus::Starting,
        project_name: project_root
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string),
        project_path: Some(project_root.display().to_string()),
        last_updated: None,
        message: None,
        progress: None,
    };
    let initial_snapshot = GraphSnapshot {
        nodes: Vec::new(),
        edges: Vec::new(),
        files: Vec::new(),
        events: Vec::new(),
        status: initial_status.clone(),
    };
    let (ws_tx, _) = broadcast::channel(64);
    let analyzer = Arc::new(AnalyzerState {
        binary: args.rust_analyzer.clone(),
        root: RwLock::new(project_root.clone()),
        client: AsyncMutex::new(None),
    });
    let state = AppStateHandle {
        project_root: Arc::new(RwLock::new(project_root.clone())),
        graph: Arc::new(RwLock::new(initial_snapshot)),
        status: Arc::new(RwLock::new(initial_status)),
        ws_tx,
        analyzer,
        diagnostics_by_file: Arc::new(RwLock::new(HashMap::new())),
        diagnostics_by_node: Arc::new(RwLock::new(HashMap::new())),
        watcher: Arc::new(RwLock::new(None)),
        is_indexing: Arc::new(AtomicBool::new(false)),
        enable_editor_open: args.enable_editor_open,
    };
    install_watcher(&state, project_root.clone());

    info!(project_root = %project_root.display(), frontend_dist = %args.frontend_dist.display(), rust_analyzer = %args.rust_analyzer.display(), "starting Rust Code Command Center");

    let index_state = state.clone();
    tokio::spawn(async move {
        index_and_publish(index_state, project_root).await;
    });

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/status", get(status))
        .route("/api/graph/snapshot", get(snapshot))
        .route("/api/node/{id}", get(node))
        .route("/api/node/{id}/details", get(node_details))
        .route("/api/search", get(search))
        .route("/api/focus", post(focus))
        .route("/api/editor/open", post(open_in_editor))
        .route("/api/project/open", post(open_project))
        .route("/ws", get(ws_handler))
        .fallback_service(
            ServeDir::new(&args.frontend_dist)
                .not_found_service(ServeFile::new(args.frontend_dist.join("index.html"))),
        )
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = TcpListener::bind(SocketAddr::new(args.host, args.port))
        .await
        .with_context(|| format!("failed to bind {}:{}", args.host, args.port))?;
    let local_addr = listener.local_addr()?;
    let url = format!("http://{local_addr}");
    println!("{url}");
    info!(%url, "server listening");
    if args.open {
        if let Err(error) = webbrowser::open(&url) {
            warn!(?error, "failed to open browser");
        }
    }

    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        version: "0.1.0",
    })
}

async fn status(State(state): State<AppStateHandle>) -> Json<AppStatus> {
    Json(state.status.read().clone())
}

async fn snapshot(
    State(state): State<AppStateHandle>,
    Query(query): Query<SnapshotQuery>,
) -> Json<GraphSnapshot> {
    let snapshot = state.graph.read().clone();
    Json(
        query
            .mode
            .map_or(snapshot.clone(), |mode| filter_snapshot(&snapshot, mode)),
    )
}

async fn node(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let graph = state.graph.read();
    match graph.nodes.iter().find(|node| node.id == id) {
        Some(node) => (StatusCode::OK, Json(node.clone())).into_response(),
        None => (StatusCode::NOT_FOUND, "node not found").into_response(),
    }
}

async fn node_details(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let graph = state.graph.read().clone();
    let Some(node) = graph.nodes.iter().find(|node| node.id == id).cloned() else {
        return (StatusCode::NOT_FOUND, "node not found").into_response();
    };
    let node_by_id = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    let incoming_edges = graph
        .edges
        .iter()
        .filter(|edge| edge.target == id)
        .cloned()
        .collect::<Vec<_>>();
    let outgoing_edges = graph
        .edges
        .iter()
        .filter(|edge| edge.source == id)
        .cloned()
        .collect::<Vec<_>>();
    let callers = incoming_edges
        .iter()
        .filter(|edge| matches!(edge.edge_type, EdgeType::Calls | EdgeType::EndpointHandler))
        .filter_map(|edge| node_by_id.get(edge.source.as_str()).copied().cloned())
        .collect::<Vec<_>>();
    let callees = outgoing_edges
        .iter()
        .filter(|edge| matches!(edge.edge_type, EdgeType::Calls | EdgeType::EndpointHandler))
        .filter_map(|edge| node_by_id.get(edge.target.as_str()).copied().cloned())
        .collect::<Vec<_>>();
    let mut references = graph_reference_records(&incoming_edges, &node_by_id);
    references.extend(resolve_rust_references(&state, &graph, &node).await);
    dedupe_references(&mut references);
    let related_types = related_type_nodes(&incoming_edges, &outgoing_edges, &node_by_id);
    let diagnostics = state
        .diagnostics_by_node
        .read()
        .get(&id)
        .cloned()
        .unwrap_or_default();

    (
        StatusCode::OK,
        Json(NodeDetailsResponse {
            node,
            incoming_edges,
            outgoing_edges,
            callers,
            callees,
            references,
            related_types,
            diagnostics,
        }),
    )
        .into_response()
}

async fn search(
    State(state): State<AppStateHandle>,
    Query(query): Query<SearchQuery>,
) -> Json<SearchResponse> {
    let query = query.q.unwrap_or_default().to_lowercase();
    let nodes = state.graph.read().nodes.clone();
    let mut scored = nodes
        .iter()
        .filter_map(|node| score_node(node, &query).map(|score| (score, node)))
        .collect::<Vec<_>>();
    scored.sort_by(|(a_score, a), (b_score, b)| a_score.cmp(b_score).then(a.label.cmp(&b.label)));
    Json(SearchResponse {
        results: scored
            .into_iter()
            .take(30)
            .map(|(_, node)| SearchResult {
                id: node.id.clone(),
                label: node.label.clone(),
                node_type: node.node_type,
                file: node.file.clone(),
                module: node.module.clone(),
                crate_name: node.crate_name.clone(),
                line: node.line,
            })
            .collect(),
    })
}

async fn focus(
    State(state): State<AppStateHandle>,
    Json(request): Json<FocusRequest>,
) -> impl IntoResponse {
    let depth = match request.depth {
        FocusDepth::Number(depth) => Some(depth),
        FocusDepth::Full(_) => None,
    };
    let graph = state.graph.read();
    match focus_subgraph(&graph, &request.node_id, depth) {
        Some((nodes, edges)) => (
            StatusCode::OK,
            Json(FocusResponse {
                center: request.node_id,
                nodes,
                edges,
            }),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "node not found").into_response(),
    }
}

async fn open_in_editor(
    State(state): State<AppStateHandle>,
    Json(request): Json<OpenEditorRequest>,
) -> impl IntoResponse {
    if !state.enable_editor_open {
        return (
            StatusCode::FORBIDDEN,
            "Opening files in an editor is disabled. Restart with --enable-editor-open to enable it.",
        )
            .into_response();
    }
    let root = state.project_root.read().clone();
    let requested_path = if request.file.is_absolute() {
        request.file.clone()
    } else {
        root.join(&request.file)
    };
    let file = match requested_path.canonicalize() {
        Ok(file) => file,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("failed to resolve file path: {error}"),
            )
                .into_response();
        }
    };
    if !file.starts_with(&root) {
        return (
            StatusCode::BAD_REQUEST,
            "refusing to open a file outside the current project",
        )
            .into_response();
    }

    match launch_editor(&file, request.line, request.column.unwrap_or(1)).await {
        Ok(editor) => (
            StatusCode::ACCEPTED,
            Json(OpenEditorResponse {
                editor,
                file: file.display().to_string(),
                line: request.line,
            }),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to open editor: {error}"),
        )
            .into_response(),
    }
}

async fn open_project(
    State(state): State<AppStateHandle>,
    Json(request): Json<OpenProjectRequest>,
) -> impl IntoResponse {
    let root = request
        .path
        .unwrap_or_else(|| state.project_root.read().clone());
    let root = match root.canonicalize() {
        Ok(root) => root,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("failed to canonicalize project path: {error}"),
            )
                .into_response();
        }
    };
    *state.project_root.write() = root.clone();
    state.analyzer.set_root(root.clone()).await;
    install_watcher(&state, root.clone());
    let index_state = state.clone();
    tokio::spawn(async move {
        index_and_publish(index_state, root).await;
    });
    (StatusCode::ACCEPTED, Json(state.status.read().clone())).into_response()
}

async fn launch_editor(file: &Path, line: Option<u32>, column: u32) -> Result<String> {
    let mut errors = Vec::new();
    for candidate in editor_candidates(file, line, column) {
        for command_candidate in command_candidates(candidate) {
            let mut command = Command::new(&command_candidate.program);
            command.args(&command_candidate.args);
            match command.spawn() {
                Ok(_child) => return Ok(command_candidate.label),
                Err(error) => errors.push(format!("{}: {error}", command_candidate.label)),
            }
        }
    }
    anyhow::bail!(
        "no editor command worked. Set RUST_WATCHER_EDITOR, RUST_WATCHER_TERMINAL, VISUAL, or EDITOR. Tried: {}",
        errors.join("; ")
    )
}

struct EditorCommand {
    label: String,
    program: String,
    args: Vec<String>,
}

fn command_candidates(candidate: EditorCommand) -> Vec<EditorCommand> {
    if is_terminal_editor(&candidate.program) {
        return terminal_wrapped_editor_candidates(&candidate);
    }
    vec![candidate]
}

fn is_terminal_editor(program: &str) -> bool {
    matches!(
        Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(program),
        "nvim" | "vim" | "vi" | "hx" | "helix" | "kak" | "micro" | "nano"
    )
}

fn terminal_wrapped_editor_candidates(editor: &EditorCommand) -> Vec<EditorCommand> {
    let mut candidates = Vec::new();
    for env_name in ["RUST_WATCHER_TERMINAL", "TERMINAL"] {
        if let Ok(command) = std::env::var(env_name) {
            if let Some(candidate) = terminal_command_from_env(env_name, &command, editor) {
                candidates.push(candidate);
            }
        }
    }
    for terminal in [
        "x-terminal-emulator",
        "kitty",
        "alacritty",
        "wezterm",
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "xterm",
    ] {
        candidates.push(wrap_editor_in_terminal(
            terminal.to_string(),
            Vec::new(),
            editor,
        ));
    }
    candidates
}

fn terminal_command_from_env(
    env_name: &str,
    command: &str,
    editor: &EditorCommand,
) -> Option<EditorCommand> {
    let mut parts = command
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return None;
    }
    let program = parts.remove(0);
    let mut candidate = wrap_editor_in_terminal(program, parts, editor);
    candidate.label = format!("{env_name}: {}", candidate.label);
    Some(candidate)
}

fn wrap_editor_in_terminal(
    terminal: String,
    mut terminal_args: Vec<String>,
    editor: &EditorCommand,
) -> EditorCommand {
    let terminal_name = Path::new(&terminal)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&terminal);
    let editor_program = editor.program.clone();
    let editor_args = editor.args.clone();

    match terminal_name {
        "wezterm" => {
            terminal_args.push("start".into());
            terminal_args.push("--".into());
            terminal_args.push(editor_program);
            terminal_args.extend(editor_args);
        }
        "gnome-terminal" => {
            terminal_args.push("--".into());
            terminal_args.push(editor_program);
            terminal_args.extend(editor_args);
        }
        "xfce4-terminal" => {
            terminal_args.push("--disable-server".into());
            terminal_args.push("--command".into());
            terminal_args.push(shell_join(&editor.program, &editor.args));
        }
        "kitty" => {
            terminal_args.push("--".into());
            terminal_args.push(editor_program);
            terminal_args.extend(editor_args);
        }
        _ => {
            terminal_args.push("-e".into());
            terminal_args.push(editor_program);
            terminal_args.extend(editor_args);
        }
    }

    EditorCommand {
        label: format!("{terminal_name} -> {}", editor.label),
        program: terminal,
        args: terminal_args,
    }
}

fn shell_join(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_string())
        .chain(args.iter().cloned())
        .map(|part| shell_quote(&part))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "-_./:+".contains(ch))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

fn editor_candidates(file: &Path, line: Option<u32>, column: u32) -> Vec<EditorCommand> {
    let mut candidates = Vec::new();
    for env_name in ["RUST_WATCHER_EDITOR", "VISUAL"] {
        if let Ok(command) = std::env::var(env_name) {
            if !command.trim().is_empty() {
                candidates.push(editor_command_from_env(
                    env_name, &command, file, line, column,
                ));
            }
        }
    }

    let file_text = file.display().to_string();
    let line_col = match line {
        Some(line) => format!("{file_text}:{line}:{column}"),
        None => file_text.clone(),
    };
    let line_only = match line {
        Some(line) => format!("{file_text}:{line}"),
        None => file_text.clone(),
    };
    for program in ["code", "codium", "code-insiders"] {
        candidates.push(EditorCommand {
            label: program.to_string(),
            program: program.to_string(),
            args: if line.is_some() {
                vec!["--goto".into(), line_col.clone()]
            } else {
                vec![file_text.clone()]
            },
        });
    }
    candidates.push(EditorCommand {
        label: "zed".into(),
        program: "zed".into(),
        args: vec![line_only],
    });
    for program in ["rustrover", "idea"] {
        candidates.push(EditorCommand {
            label: program.to_string(),
            program: program.to_string(),
            args: match line {
                Some(line) => vec!["--line".into(), line.to_string(), file_text.clone()],
                None => vec![file_text.clone()],
            },
        });
    }
    candidates.push(EditorCommand {
        label: "xdg-open".into(),
        program: "xdg-open".into(),
        args: vec![file_text],
    });
    if let Ok(command) = std::env::var("EDITOR") {
        if !command.trim().is_empty() {
            candidates.push(editor_command_from_env(
                "EDITOR", &command, file, line, column,
            ));
        }
    }
    candidates
}

fn editor_command_from_env(
    env_name: &str,
    command: &str,
    file: &Path,
    line: Option<u32>,
    column: u32,
) -> EditorCommand {
    let mut parts = command
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let program = parts
        .first()
        .cloned()
        .unwrap_or_else(|| command.to_string());
    let file_text = file.display().to_string();
    let line_text = line.unwrap_or(1).to_string();
    let column_text = column.to_string();
    let had_template = parts.iter().any(|part| {
        part.contains("{file}") || part.contains("{line}") || part.contains("{column}")
    });
    if !parts.is_empty() {
        parts.remove(0);
    }
    let mut args = parts
        .into_iter()
        .map(|part| {
            part.replace("{file}", &file_text)
                .replace("{line}", &line_text)
                .replace("{column}", &column_text)
        })
        .collect::<Vec<_>>();
    if !had_template {
        args.extend(default_editor_args(
            &program, &args, &file_text, line, column,
        ));
    }
    EditorCommand {
        label: format!("{env_name}={program}"),
        program,
        args,
    }
}

fn default_editor_args(
    program: &str,
    existing_args: &[String],
    file: &str,
    line: Option<u32>,
    column: u32,
) -> Vec<String> {
    let program_name = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    match (program_name, line) {
        ("code" | "codium" | "code-insiders", Some(line)) => {
            if existing_args.iter().any(|arg| arg == "--goto") {
                vec![format!("{file}:{line}:{column}")]
            } else {
                vec!["--goto".into(), format!("{file}:{line}:{column}")]
            }
        }
        ("zed", Some(line)) => vec![format!("{file}:{line}")],
        ("idea" | "rustrover", Some(line)) => vec!["--line".into(), line.to_string(), file.into()],
        ("nvim" | "vim" | "vi" | "kak", Some(line)) => {
            vec![format!("+{line}"), file.into()]
        }
        ("nano" | "micro", Some(line)) => vec![format!("+{line},{column}"), file.into()],
        ("hx" | "helix", Some(line)) => vec![format!("{file}:{line}:{column}")],
        _ => vec![file.into()],
    }
}

fn install_watcher(state: &AppStateHandle, root: PathBuf) {
    let handle = tokio::runtime::Handle::current();
    let watch_state = state.clone();
    match start_watcher(root.clone(), move |event| {
        let state = watch_state.clone();
        if state.is_indexing.load(Ordering::Relaxed) {
            return;
        }
        let changed_path = event
            .paths
            .first()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let root = state.project_root.read().clone();
        handle.spawn(async move {
            let analysis_event = analysis_event(
                AnalysisEventType::Analyzer,
                format!("File changed: {changed_path}"),
                Some(changed_path),
            );
            {
                let mut graph = state.graph.write();
                graph.events.push(analysis_event.clone());
            }
            update_status(&state, |status| {
                status.analyzer_status = AnalyzerStatus::Stale;
                status.message = Some("File changed. Re-indexing workspace.".into());
                status.progress = Some(0);
            });
            let _ = state
                .ws_tx
                .send(ServerMessage::AnalysisEvent(analysis_event));
            sleep(Duration::from_millis(250)).await;
            let changed_files = event
                .paths
                .iter()
                .map(|path| project_indexer::relative_to(&root, path))
                .collect::<Vec<_>>();
            index_and_patch(state, root, changed_files).await;
        });
    }) {
        Ok(watcher) => {
            *state.watcher.write() = Some(watcher);
            info!(project_root = %root.display(), "file watcher installed");
        }
        Err(error) => {
            warn!(project_root = %root.display(), ?error, "failed to install file watcher")
        }
    }
}

async fn ws_handler(
    State(state): State<AppStateHandle>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| websocket(socket, state))
}

async fn websocket(socket: WebSocket, state: AppStateHandle) {
    info!("websocket connected");
    let (mut sender, mut receiver) = socket.split();
    let initial = ServerMessage::GraphSnapshot(state.graph.read().clone());
    if let Ok(text) = serde_json::to_string(&initial) {
        let _ = sender.send(Message::Text(text.into())).await;
    }

    let mut rx = state.ws_tx.subscribe();
    let forward = tokio::spawn(async move {
        while let Ok(message) = rx.recv().await {
            match serde_json::to_string(&message) {
                Ok(text) => {
                    if sender.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(error) => warn!(?error, "failed to serialize websocket message"),
            }
        }
    });

    while let Some(message) = receiver.next().await {
        if matches!(message, Ok(Message::Close(_)) | Err(_)) {
            break;
        }
    }
    forward.abort();
    info!("websocket disconnected");
}

async fn index_and_publish(state: AppStateHandle, project_root: PathBuf) {
    if state.is_indexing.swap(true, Ordering::SeqCst) {
        info!(project_root = %project_root.display(), "indexing already in progress, skipping");
        return;
    }
    info!(project_root = %project_root.display(), "indexing start");
    update_status(&state, |status| {
        status.app_state = AppState::Indexing;
        status.analyzer_status = AnalyzerStatus::Starting;
        status.project_name = project_root
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string);
        status.project_path = Some(project_root.display().to_string());
        status.message = Some("Indexing workspace".into());
        status.progress = Some(5);
    });

    let index = match index_project(&project_root) {
        Ok(index) => index,
        Err(error) => {
            error!(?error, "failed to index project");
            let mut status = state.status.read().clone();
            status.app_state = AppState::Error;
            status.analyzer_status = AnalyzerStatus::Error;
            status.message = Some("No Cargo.toml found in project root.".into());
            status.progress = None;
            *state.status.write() = status.clone();
            state.graph.write().status = status.clone();
            let _ = state.ws_tx.send(ServerMessage::AnalyzerStatus(status));
            state.is_indexing.store(false, Ordering::SeqCst);
            return;
        }
    };

    update_status(&state, |status| {
        status.analyzer_status = AnalyzerStatus::Indexing;
        status.message = Some("Building fallback graph".into());
        status.progress = Some(25);
    });

    let fallback_status = state.status.read().clone();
    let mut snapshot = build_fallback_graph(&index, fallback_status);
    publish_snapshot(&state, snapshot.clone());

    update_status(&state, |status| {
        status.message = Some("Starting rust-analyzer".into());
        status.progress = Some(40);
    });

    match state.analyzer.subscribe_notifications().await {
        Ok(rx) => spawn_diagnostics_listener(state.clone(), rx),
        Err(error) => {
            warn!(?error, "rust-analyzer unavailable, using fallback graph");
            publish_analyzer_fallback(
                &state,
                snapshot,
                "rust-analyzer is unavailable. Using syntax graph fallback.",
            );
            info!(
                nodes = state.graph.read().nodes.len(),
                edges = state.graph.read().edges.len(),
                files = state.graph.read().files.len(),
                "indexing finish"
            );
            state.is_indexing.store(false, Ordering::SeqCst);
            return;
        }
    }

    {
        update_status(&state, |status| {
            status.message = Some("Reading document symbols".into());
            status.progress = Some(55);
        });
        for (idx, file) in index.files.iter().enumerate() {
            match timeout(
                Duration::from_secs(3),
                state.analyzer.document_symbols(&file.absolute_path),
            )
            .await
            {
                Ok(Ok(symbols)) => enrich_file_symbols(&mut snapshot, file, &symbols),
                Ok(Err(error)) => {
                    warn!(file = %file.relative_path, ?error, "documentSymbol failed")
                }
                Err(_) => warn!(file = %file.relative_path, "documentSymbol timed out"),
            }
            let progress = 55 + ((idx as f32 / index.files.len().max(1) as f32) * 35.0) as u8;
            update_status(&state, |status| status.progress = Some(progress.min(90)));
        }
        update_status(&state, |status| {
            status.message = Some("Resolving semantic call graph".into());
            status.progress = Some(92);
        });
        enrich_semantic_call_edges(&mut snapshot, &project_root, &state.analyzer).await;
        snapshot.status = ready_status(&state, "Ready");
        publish_snapshot(&state, snapshot);
    }

    info!(
        nodes = state.graph.read().nodes.len(),
        edges = state.graph.read().edges.len(),
        files = state.graph.read().files.len(),
        "indexing finish"
    );
    state.is_indexing.store(false, Ordering::SeqCst);
}

async fn index_and_patch(state: AppStateHandle, project_root: PathBuf, changed_files: Vec<String>) {
    if state.is_indexing.swap(true, Ordering::SeqCst) {
        return;
    }
    update_status(&state, |status| {
        status.analyzer_status = AnalyzerStatus::Indexing;
        status.message = Some("Updating changed files".into());
        status.progress = Some(20);
    });

    let old_snapshot = state.graph.read().clone();
    let index = match index_project(&project_root) {
        Ok(index) => index,
        Err(error) => {
            warn!(?error, "incremental index failed; keeping current graph");
            state.is_indexing.store(false, Ordering::SeqCst);
            return;
        }
    };

    let mut snapshot = build_fallback_graph(&index, state.status.read().clone());
    if state.analyzer.subscribe_notifications().await.is_ok() {
        for file in &index.files {
            match timeout(
                Duration::from_secs(3),
                state.analyzer.document_symbols(&file.absolute_path),
            )
            .await
            {
                Ok(Ok(symbols)) => enrich_file_symbols(&mut snapshot, file, &symbols),
                Ok(Err(error)) => {
                    warn!(file = %file.relative_path, ?error, "documentSymbol failed during patch")
                }
                Err(_) => {
                    warn!(file = %file.relative_path, "documentSymbol timed out during patch")
                }
            }
        }
        enrich_semantic_call_edges(&mut snapshot, &project_root, &state.analyzer).await;
    }
    snapshot.status = ready_status(&state, "Ready");
    let diagnostics = state
        .diagnostics_by_file
        .read()
        .values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    apply_diagnostics_to_files(&mut snapshot, &diagnostics);
    let patch = diff_snapshots(&old_snapshot, &snapshot, changed_files, diagnostics);
    *state.graph.write() = snapshot;
    let _ = state.ws_tx.send(ServerMessage::GraphPatch(patch));
    state.is_indexing.store(false, Ordering::SeqCst);
}

fn diff_snapshots(
    old: &GraphSnapshot,
    new: &GraphSnapshot,
    changed_files: Vec<String>,
    diagnostics: Vec<DiagnosticRecord>,
) -> GraphPatch {
    let old_nodes = old
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    let new_nodes = new
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    let old_edges = old
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<HashMap<_, _>>();
    let new_edges = new
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<HashMap<_, _>>();

    GraphPatch {
        added_nodes: new
            .nodes
            .iter()
            .filter(|node| !old_nodes.contains_key(node.id.as_str()))
            .cloned()
            .collect(),
        updated_nodes: new
            .nodes
            .iter()
            .filter(|node| {
                old_nodes.get(node.id.as_str()).is_some_and(|old| {
                    serde_json::to_value(old).ok() != serde_json::to_value(node).ok()
                })
            })
            .cloned()
            .collect(),
        removed_node_ids: old
            .nodes
            .iter()
            .filter(|node| !new_nodes.contains_key(node.id.as_str()))
            .map(|node| node.id.clone())
            .collect(),
        added_edges: new
            .edges
            .iter()
            .filter(|edge| !old_edges.contains_key(edge.id.as_str()))
            .cloned()
            .collect(),
        updated_edges: new
            .edges
            .iter()
            .filter(|edge| {
                old_edges.get(edge.id.as_str()).is_some_and(|old| {
                    serde_json::to_value(old).ok() != serde_json::to_value(edge).ok()
                })
            })
            .cloned()
            .collect(),
        removed_edge_ids: old
            .edges
            .iter()
            .filter(|edge| !new_edges.contains_key(edge.id.as_str()))
            .map(|edge| edge.id.clone())
            .collect(),
        diagnostics,
        changed_files,
    }
}

fn apply_diagnostics_to_files(snapshot: &mut GraphSnapshot, diagnostics: &[DiagnosticRecord]) {
    let mut by_file: HashMap<&str, u32> = HashMap::new();
    for diagnostic in diagnostics {
        *by_file.entry(diagnostic.file.as_str()).or_default() += 1;
    }
    for file in &mut snapshot.files {
        file.diagnostics_count = by_file.get(file.path.as_str()).copied().unwrap_or_default();
    }
}

fn publish_analyzer_fallback(
    state: &AppStateHandle,
    mut snapshot: GraphSnapshot,
    message: &'static str,
) {
    snapshot.status = fallback_status(state, message);
    snapshot
        .events
        .push(analysis_event(AnalysisEventType::Warning, message, None));
    publish_snapshot(state, snapshot);
}

fn spawn_diagnostics_listener(
    state: AppStateHandle,
    mut rx: broadcast::Receiver<ra_client::LspNotification>,
) {
    tokio::spawn(async move {
        while let Ok(notification) = rx.recv().await {
            if notification.method != "textDocument/publishDiagnostics" {
                continue;
            }
            let Ok(params) = serde_json::from_value::<ra_client::LspPublishDiagnosticsParams>(
                notification.params,
            ) else {
                continue;
            };
            apply_lsp_diagnostics(&state, params);
        }
    });
}

fn apply_lsp_diagnostics(state: &AppStateHandle, params: ra_client::LspPublishDiagnosticsParams) {
    let Some(path) = Url::parse(params.uri.as_str())
        .ok()
        .and_then(|uri| uri.to_file_path().ok())
    else {
        return;
    };
    let root = state.project_root.read().clone();
    let file = project_indexer::relative_to(&root, &path);
    let graph = state.graph.read().clone();
    let symbol_index = SymbolIndex::from_nodes(&graph.nodes);
    let diagnostics = params
        .diagnostics
        .into_iter()
        .enumerate()
        .map(|(idx, diagnostic)| diagnostic_from_lsp(&file, idx, diagnostic, &symbol_index))
        .collect::<Vec<_>>();

    state
        .diagnostics_by_file
        .write()
        .insert(file.clone(), diagnostics.clone());
    rebuild_diagnostics_by_node(state);
    update_project_file_diagnostics(state, &file, diagnostics.len() as u32);

    let _ = state.ws_tx.send(ServerMessage::GraphPatch(GraphPatch {
        diagnostics,
        changed_files: vec![file],
        ..GraphPatch::default()
    }));
}

fn diagnostic_from_lsp(
    file: &str,
    index: usize,
    diagnostic: ra_client::LspDiagnostic,
    symbol_index: &SymbolIndex,
) -> DiagnosticRecord {
    let range = graph_core::TextRange {
        start: graph_core::TextPosition {
            line: diagnostic.range.start.line,
            character: diagnostic.range.start.character,
        },
        end: graph_core::TextPosition {
            line: diagnostic.range.end.line,
            character: diagnostic.range.end.character,
        },
    };
    let related_node_ids = related_nodes_for_range(symbol_index, file, range);
    let code = diagnostic.code.map(|code| match code {
        ra_client::LspNumberOrString::Number(value) => value.to_string(),
        ra_client::LspNumberOrString::String(value) => value,
    });
    DiagnosticRecord {
        id: format!(
            "diagnostic:{file}:{}:{}:{index}",
            range.start.line, range.start.character
        ),
        language: LanguageId::Rust,
        file: file.to_string(),
        range: Some(range),
        severity: diagnostic_severity(diagnostic.severity),
        source: diagnostic.source,
        message: diagnostic.message,
        code,
        related_node_ids,
    }
}

fn diagnostic_severity(severity: Option<ra_client::LspDiagnosticSeverity>) -> DiagnosticSeverity {
    match severity {
        Some(ra_client::LspDiagnosticSeverity::ERROR) => DiagnosticSeverity::Error,
        Some(ra_client::LspDiagnosticSeverity::WARNING) => DiagnosticSeverity::Warning,
        Some(ra_client::LspDiagnosticSeverity::INFORMATION) => DiagnosticSeverity::Information,
        Some(ra_client::LspDiagnosticSeverity::HINT) => DiagnosticSeverity::Hint,
        _ => DiagnosticSeverity::Information,
    }
}

fn related_nodes_for_range(
    symbol_index: &SymbolIndex,
    file: &str,
    range: graph_core::TextRange,
) -> Vec<String> {
    symbol_index
        .find_by_file(file)
        .into_iter()
        .filter(|symbol| ranges_overlap(symbol.range, range))
        .map(|symbol| symbol.node_id.clone())
        .collect()
}

fn ranges_overlap(left: graph_core::TextRange, right: graph_core::TextRange) -> bool {
    position_le(left.start, right.end) && position_le(right.start, left.end)
}

fn position_le(left: graph_core::TextPosition, right: graph_core::TextPosition) -> bool {
    left.line < right.line || (left.line == right.line && left.character <= right.character)
}

fn rebuild_diagnostics_by_node(state: &AppStateHandle) {
    let mut by_node: HashMap<String, Vec<DiagnosticRecord>> = HashMap::new();
    for diagnostic in state.diagnostics_by_file.read().values().flatten() {
        for node_id in &diagnostic.related_node_ids {
            by_node
                .entry(node_id.clone())
                .or_default()
                .push(diagnostic.clone());
        }
    }
    *state.diagnostics_by_node.write() = by_node;
}

fn update_project_file_diagnostics(state: &AppStateHandle, file: &str, count: u32) {
    let mut graph = state.graph.write();
    if let Some(project_file) = graph
        .files
        .iter_mut()
        .find(|project_file| project_file.path == file)
    {
        project_file.diagnostics_count = count;
    }
}

async fn enrich_semantic_call_edges(
    snapshot: &mut GraphSnapshot,
    project_root: &Path,
    analyzer: &AnalyzerState,
) {
    let symbol_index = SymbolIndex::from_nodes(&snapshot.nodes);
    if symbol_index.symbols.is_empty() {
        return;
    }
    let callable_symbols = symbol_index
        .symbols
        .iter()
        .filter(|symbol| {
            symbol.language == LanguageId::Rust
                && matches!(
                    symbol.kind,
                    SymbolKindName::Function | SymbolKindName::Method
                )
        })
        .map(|symbol| {
            (
                symbol.node_id.clone(),
                project_root.join(&symbol.file),
                symbol.selection_range.start,
            )
        })
        .collect::<Vec<_>>();

    for (source_id, file, position) in callable_symbols {
        let items = match timeout(
            Duration::from_secs(2),
            analyzer.prepare_call_hierarchy(&file, position.line, position.character),
        )
        .await
        {
            Ok(Ok(items)) => items,
            Ok(Err(error)) => {
                warn!(?error, source = %source_id, "prepareCallHierarchy failed");
                continue;
            }
            Err(_) => {
                warn!(source = %source_id, "prepareCallHierarchy timed out");
                continue;
            }
        };
        for item in items {
            let outgoing =
                match timeout(Duration::from_secs(2), analyzer.outgoing_calls(&item)).await {
                    Ok(Ok(outgoing)) => outgoing,
                    Ok(Err(error)) => {
                        warn!(?error, source = %source_id, "outgoingCalls failed");
                        continue;
                    }
                    Err(_) => {
                        warn!(source = %source_id, "outgoingCalls timed out");
                        continue;
                    }
                };
            for call in outgoing {
                let Some(target_path) = Url::parse(call.to.uri.as_str())
                    .ok()
                    .and_then(|uri| uri.to_file_path().ok())
                else {
                    continue;
                };
                insert_semantic_call_edge(
                    snapshot,
                    &symbol_index,
                    &source_id,
                    &target_path,
                    call.to.selection_range.start.line,
                    call.to.selection_range.start.character,
                );
            }
        }
    }
}

fn insert_semantic_call_edge(
    snapshot: &mut GraphSnapshot,
    symbol_index: &SymbolIndex,
    source_id: &str,
    target_path: &Path,
    line: u32,
    character: u32,
) -> bool {
    let Some(target) = symbol_index.find_by_uri_path_position(target_path, line, character) else {
        return false;
    };
    push_unique_edge_with_confidence(
        &mut snapshot.edges,
        &HashSet::new(),
        EdgeType::Calls,
        source_id,
        &target.node_id,
        EdgeConfidence::Semantic,
    );
    true
}

fn graph_reference_records(
    incoming_edges: &[graph_core::GraphEdge],
    node_by_id: &HashMap<&str, &GraphNode>,
) -> Vec<ReferenceRecord> {
    incoming_edges
        .iter()
        .filter(|edge| {
            matches!(
                edge.edge_type,
                EdgeType::Calls
                    | EdgeType::EndpointHandler
                    | EdgeType::TypeReference
                    | EdgeType::Uses
                    | EdgeType::DataFlow
            )
        })
        .filter_map(|edge| node_by_id.get(edge.source.as_str()).copied())
        .filter_map(|node| reference_from_node(Some(node.clone())))
        .collect()
}

fn related_type_nodes(
    incoming_edges: &[graph_core::GraphEdge],
    outgoing_edges: &[graph_core::GraphEdge],
    node_by_id: &HashMap<&str, &GraphNode>,
) -> Vec<GraphNode> {
    let mut seen = HashSet::new();
    incoming_edges
        .iter()
        .chain(outgoing_edges.iter())
        .filter(|edge| {
            matches!(
                edge.edge_type,
                EdgeType::TypeReference | EdgeType::Implements
            )
        })
        .flat_map(|edge| [edge.source.as_str(), edge.target.as_str()])
        .filter_map(|id| node_by_id.get(id).copied())
        .filter(|node| {
            matches!(
                node.node_type,
                graph_core::NodeType::Struct
                    | graph_core::NodeType::Enum
                    | graph_core::NodeType::Trait
                    | graph_core::NodeType::Impl
                    | graph_core::NodeType::Interface
                    | graph_core::NodeType::TypeAlias
            )
        })
        .filter(|node| seen.insert(node.id.clone()))
        .cloned()
        .collect()
}

async fn resolve_rust_references(
    state: &AppStateHandle,
    graph: &GraphSnapshot,
    node: &GraphNode,
) -> Vec<ReferenceRecord> {
    if node.language.as_deref() != Some(LanguageId::Rust.as_str())
        || !matches!(
            node.node_type,
            graph_core::NodeType::Function | graph_core::NodeType::Method
        )
    {
        return Vec::new();
    }
    let Some(file) = node.file.as_ref() else {
        return Vec::new();
    };
    let Some(selection_range) = node.selection_range else {
        return Vec::new();
    };
    let project_root = state.project_root.read().clone();
    let absolute_file = project_root.join(file);
    let locations = match timeout(
        Duration::from_secs(4),
        state.analyzer.references(
            &absolute_file,
            selection_range.start.line,
            selection_range.start.character,
        ),
    )
    .await
    {
        Ok(Ok(locations)) => locations,
        Ok(Err(error)) => {
            warn!(?error, node = %node.id, "rust-analyzer references failed");
            return Vec::new();
        }
        Err(_) => {
            warn!(node = %node.id, "rust-analyzer references timed out");
            return Vec::new();
        }
    };

    references_from_locations(graph, &project_root, locations)
}

fn references_from_locations(
    graph: &GraphSnapshot,
    project_root: &Path,
    locations: Vec<ra_client::LspLocation>,
) -> Vec<ReferenceRecord> {
    let symbol_index = SymbolIndex::from_nodes(&graph.nodes);
    let node_by_id = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    locations
        .into_iter()
        .filter_map(|location| {
            reference_from_location(project_root, &symbol_index, &node_by_id, location)
        })
        .collect()
}

fn reference_from_location(
    project_root: &Path,
    symbol_index: &SymbolIndex,
    node_by_id: &HashMap<&str, &GraphNode>,
    location: ra_client::LspLocation,
) -> Option<ReferenceRecord> {
    let path = Url::parse(location.uri.as_str())
        .ok()?
        .to_file_path()
        .ok()?;
    let file = project_indexer::relative_to(project_root, &path);
    let range = graph_core::TextRange {
        start: graph_core::TextPosition {
            line: location.range.start.line,
            character: location.range.start.character,
        },
        end: graph_core::TextPosition {
            line: location.range.end.line,
            character: location.range.end.character,
        },
    };
    let node = symbol_index
        .find_by_uri_path_position(&path, range.start.line, range.start.character)
        .and_then(|symbol| node_by_id.get(symbol.node_id.as_str()).copied())
        .cloned();
    Some(ReferenceRecord {
        node,
        location: SourceLocation {
            file,
            line: range.start.line + 1,
            character: range.start.character,
            range: Some(range),
        },
    })
}

fn reference_from_node(node: Option<GraphNode>) -> Option<ReferenceRecord> {
    let node = node?;
    let file = node.file.clone()?;
    let range = node.range;
    Some(ReferenceRecord {
        location: SourceLocation {
            file,
            line: node
                .line
                .unwrap_or_else(|| range.map(|range| range.start.line + 1).unwrap_or_default()),
            character: node
                .selection_range
                .map(|range| range.start.character)
                .unwrap_or_default(),
            range,
        },
        node: Some(node),
    })
}

fn dedupe_references(references: &mut Vec<ReferenceRecord>) {
    let mut seen = HashSet::new();
    references.retain(|reference| {
        seen.insert((
            reference.location.file.clone(),
            reference.location.line,
            reference.location.character,
            reference.node.as_ref().map(|node| node.id.clone()),
        ))
    });
}

fn update_status<F>(state: &AppStateHandle, mut update: F)
where
    F: FnMut(&mut AppStatus),
{
    let mut status = state.status.read().clone();
    update(&mut status);
    status.last_updated = Some(timestamp());
    *state.status.write() = status.clone();
    state.graph.write().status = status.clone();
    let _ = state.ws_tx.send(ServerMessage::AnalyzerStatus(status));
}

fn publish_snapshot(state: &AppStateHandle, mut snapshot: GraphSnapshot) {
    snapshot.status.last_updated = Some(timestamp());
    *state.status.write() = snapshot.status.clone();
    *state.graph.write() = snapshot.clone();
    let _ = state.ws_tx.send(ServerMessage::GraphSnapshot(snapshot));
}

fn ready_status(state: &AppStateHandle, message: &str) -> AppStatus {
    let mut status = state.status.read().clone();
    status.app_state = AppState::Normal;
    status.analyzer_status = AnalyzerStatus::Ready;
    status.message = Some(message.into());
    status.progress = Some(100);
    status.last_updated = Some(timestamp());
    *state.status.write() = status.clone();
    let _ = state
        .ws_tx
        .send(ServerMessage::AnalyzerStatus(status.clone()));
    status
}

fn fallback_status(state: &AppStateHandle, message: &str) -> AppStatus {
    let mut status = state.status.read().clone();
    status.app_state = AppState::Normal;
    status.analyzer_status = AnalyzerStatus::Fallback;
    status.message = Some(message.into());
    status.progress = Some(100);
    status.last_updated = Some(timestamp());
    *state.status.write() = status.clone();
    let _ = state
        .ws_tx
        .send(ServerMessage::AnalyzerStatus(status.clone()));
    status
}

fn analysis_event(
    event_type: AnalysisEventType,
    message: impl Into<String>,
    file: Option<String>,
) -> AnalysisEvent {
    AnalysisEvent {
        id: Uuid::new_v4().to_string(),
        event_type,
        message: message.into(),
        timestamp: timestamp(),
        file,
    }
}

fn timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{secs}")
}

fn score_node(node: &GraphNode, query: &str) -> Option<u8> {
    if query.is_empty() {
        return Some(3);
    }
    let fields = [
        node.label.to_lowercase(),
        node.file.clone().unwrap_or_default().to_lowercase(),
        node.module.clone().unwrap_or_default().to_lowercase(),
        node.crate_name.clone().unwrap_or_default().to_lowercase(),
        format!("{:?}", node.node_type).to_lowercase(),
    ];
    if fields.iter().any(|field| field == query) {
        Some(0)
    } else if fields.iter().any(|field| field.starts_with(query)) {
        Some(1)
    } else if fields.iter().any(|field| field.contains(query)) {
        Some(2)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_core::{EdgeConfidence, LspPosition, LspRange, Visibility};

    fn test_node(label: &str, file: Option<&str>, module: Option<&str>) -> GraphNode {
        let range = LspRange {
            start: LspPosition {
                line: 0,
                character: 0,
            },
            end: LspPosition {
                line: 0,
                character: label.len() as u32,
            },
        };
        GraphNode {
            id: format!("fn:{}@1", label),
            language: Some("rust".into()),
            node_type: graph_core::NodeType::Function,
            label: label.into(),
            file: file.map(str::to_string),
            module: module.map(str::to_string),
            crate_name: Some("demo".into()),
            line: Some(1),
            visibility: Some(Visibility::Pub),
            is_async: None,
            is_unsafe: None,
            is_generic: None,
            signature: None,
            description: None,
            pinned: None,
            bookmarked: None,
            connections: None,
            range: Some(range),
            selection_range: Some(range),
            x: 0.0,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
        }
    }

    #[test]
    fn search_scoring_prefers_exact_then_prefix_then_contains() {
        let exact = test_node("main", Some("src/main.rs"), Some("app"));
        let prefix = test_node("main_handler", Some("src/routes.rs"), Some("app"));
        let contains = test_node("domain_main", Some("src/domain.rs"), Some("app"));
        let miss = test_node("health", Some("src/health.rs"), Some("app"));

        assert_eq!(score_node(&exact, "main"), Some(0));
        assert_eq!(score_node(&prefix, "main"), Some(1));
        assert_eq!(score_node(&contains, "main"), Some(2));
        assert_eq!(score_node(&miss, "main"), None);
    }

    #[test]
    fn node_details_shape_can_hold_confidence_edges() {
        let node = test_node("main", Some("src/main.rs"), Some("app"));
        let edge = graph_core::GraphEdge {
            id: "Calls:a->b".into(),
            source: "a".into(),
            target: "b".into(),
            edge_type: EdgeType::Calls,
            confidence: EdgeConfidence::Semantic,
        };
        let response = NodeDetailsResponse {
            node,
            incoming_edges: vec![edge.clone()],
            outgoing_edges: vec![edge],
            callers: Vec::new(),
            callees: Vec::new(),
            references: vec![ReferenceRecord {
                node: None,
                location: SourceLocation {
                    file: "src/main.rs".into(),
                    line: 1,
                    character: 0,
                    range: None,
                },
            }],
            related_types: Vec::new(),
            diagnostics: Vec::new(),
        };
        assert_eq!(
            response.incoming_edges[0].confidence,
            EdgeConfidence::Semantic
        );
        assert_eq!(response.references[0].location.file, "src/main.rs");
    }

    #[test]
    fn semantic_call_edge_insertion_resolves_target_from_symbol_index() {
        let source = test_node("main", Some("src/main.rs"), Some("app"));
        let mut target = test_node("helper", Some("src/main.rs"), Some("app"));
        let target_range = LspRange {
            start: LspPosition {
                line: 4,
                character: 0,
            },
            end: LspPosition {
                line: 4,
                character: 6,
            },
        };
        target.line = Some(5);
        target.range = Some(target_range);
        target.selection_range = Some(target_range);
        let mut snapshot = GraphSnapshot {
            nodes: vec![source.clone(), target.clone()],
            edges: vec![graph_core::GraphEdge {
                id: graph_core::edge_id(EdgeType::Calls, &source.id, &target.id),
                source: source.id.clone(),
                target: target.id.clone(),
                edge_type: EdgeType::Calls,
                confidence: EdgeConfidence::SyntaxFallback,
            }],
            files: Vec::new(),
            events: Vec::new(),
            status: AppStatus::empty(),
        };
        let symbol_index = SymbolIndex::from_nodes(&snapshot.nodes);

        assert!(insert_semantic_call_edge(
            &mut snapshot,
            &symbol_index,
            &source.id,
            Path::new("/tmp/project/src/main.rs"),
            4,
            0,
        ));
        let edge = snapshot
            .edges
            .iter()
            .find(|edge| edge.source == source.id && edge.target == target.id)
            .unwrap();
        assert_eq!(edge.confidence, EdgeConfidence::Semantic);
    }

    #[test]
    fn reference_records_preserve_unresolved_source_locations() {
        let location = ReferenceRecord {
            node: None,
            location: SourceLocation {
                file: "src/lib.rs".into(),
                line: 7,
                character: 3,
                range: Some(LspRange {
                    start: LspPosition {
                        line: 6,
                        character: 3,
                    },
                    end: LspPosition {
                        line: 6,
                        character: 8,
                    },
                }),
            },
        };
        assert!(location.node.is_none());
        assert_eq!(location.location.line, 7);
        assert_eq!(location.location.range.unwrap().start.line, 6);
    }

    #[test]
    fn lsp_diagnostic_converts_and_associates_to_node() {
        let node = test_node("main", Some("src/main.rs"), Some("app"));
        let symbol_index = SymbolIndex::from_nodes(std::slice::from_ref(&node));
        let diagnostic: ra_client::LspDiagnostic = serde_json::from_value(serde_json::json!({
            "range": {
                "start": { "line": 0, "character": 1 },
                "end": { "line": 0, "character": 2 }
            },
            "severity": 1,
            "source": "rustc",
            "message": "broken"
        }))
        .unwrap();

        let record = diagnostic_from_lsp("src/main.rs", 0, diagnostic, &symbol_index);
        assert_eq!(record.severity, DiagnosticSeverity::Error);
        assert_eq!(record.source.as_deref(), Some("rustc"));
        assert_eq!(record.related_node_ids, vec![node.id]);
    }

    #[test]
    fn graph_patch_serializes_diagnostics_and_changed_files() {
        let patch = GraphPatch {
            diagnostics: vec![DiagnosticRecord {
                id: "diagnostic:src/main.rs:0:0:0".into(),
                language: LanguageId::Rust,
                file: "src/main.rs".into(),
                range: None,
                severity: DiagnosticSeverity::Warning,
                source: Some("rustc".into()),
                message: "careful".into(),
                code: None,
                related_node_ids: vec!["fn:main@1".into()],
            }],
            changed_files: vec!["src/main.rs".into()],
            ..GraphPatch::default()
        };
        let value = serde_json::to_value(&patch).unwrap();
        assert_eq!(value["changedFiles"][0], "src/main.rs");
        assert_eq!(value["diagnostics"][0]["message"], "careful");
    }
}
