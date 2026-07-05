use crate::routes::layout::{
    apply_layout_store_to_snapshot, clear_layout, layout_path, load_layout, save_layout,
    storage_dir_for_project, LayoutNode, LayoutStore,
};
use crate::routes::views::{load_views, save_views, SavedView, SavedViewsStore};
use crate::server::{analyzer_services_from_counts, AnalyzerFileCounts};
use crate::services::diagnostics::diagnostic_from_lsp;
use graph_core::{
    AnalyzerCapability, AnalyzerEngine, AnalyzerKind, AnalyzerProvider, AnalyzerServiceStatus,
    AnalyzerStatus, AppStatus, DiagnosticRecord, DiagnosticSeverity, EdgeConfidence, EdgeType,
    GraphNode, GraphPatch, GraphSnapshot, LanguageId, LspPosition, LspRange, PythonAnalyzerStatus,
    ReferenceRecord, SourceLocation, SourceReachability, SymbolIndex, TraceStepKind, Visibility,
};
use graph_query::trace::{build_node_trace, build_route_trace};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::qml_lsp::QmlAnalyzerStatus;
use crate::typescript_lsp::TypeScriptAnalyzerStatus;

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
        reachability: None,
        reachable_from: None,
        detached_reason: None,
        x: 0.0,
        y: 0.0,
        vx: 0.0,
        vy: 0.0,
    }
}

fn test_edge(
    edge_type: EdgeType,
    source: impl Into<String>,
    target: impl Into<String>,
    confidence: EdgeConfidence,
) -> graph_core::GraphEdge {
    let source = source.into();
    let target = target.into();
    graph_core::GraphEdge {
        id: graph_core::edge_id(edge_type, &source, &target),
        source,
        target,
        edge_type,
        confidence,
        label: None,
        description: None,
        data_flow_kind: None,
        evidence: None,
    }
}

fn assert_local_analyzer(service: &AnalyzerServiceStatus) {
    assert_eq!(service.provider, AnalyzerProvider::Local);
    assert!(!service.billable);
    assert_eq!(service.credits_used, None);
}

#[test]
fn analyzer_services_omit_optional_languages_without_files() {
    let services = analyzer_services_from_counts(
        AnalyzerStatus::Ready,
        Some("Ready".into()),
        Some(PythonAnalyzerStatus {
            mode: "auto".into(),
            status: "ty unavailable".into(),
            message: Some("missing ty".into()),
        }),
        Some(TypeScriptAnalyzerStatus {
            mode: "auto".into(),
            status: "language server unavailable".into(),
            message: Some("missing typescript-language-server".into()),
        }),
        Some(QmlAnalyzerStatus {
            mode: "auto".into(),
            status: "qmlls unavailable".into(),
            message: Some("missing qmlls".into()),
        }),
        AnalyzerFileCounts {
            rust: 1,
            ..AnalyzerFileCounts::default()
        },
        None,
    );

    assert!(services.iter().any(|service| service.id == "rust-analyzer"));
    assert!(!services
        .iter()
        .any(|service| service.kind == AnalyzerKind::Python));
    assert!(!services
        .iter()
        .any(|service| service.kind == AnalyzerKind::TypeScript));
    assert!(!services
        .iter()
        .any(|service| service.kind == AnalyzerKind::Qml));
}

#[test]
fn analyzer_services_include_rust_typescript_and_qml_records() {
    let services = analyzer_services_from_counts(
        AnalyzerStatus::Ready,
        Some("Ready".into()),
        None,
        None,
        None,
        AnalyzerFileCounts {
            rust: 2,
            typescript: 3,
            python: 0,
            qml: 4,
        },
        Some("now".into()),
    );

    assert!(services.iter().any(|service| {
        service.id == "rust-analyzer"
            && service.kind == AnalyzerKind::Rust
            && service.engine == AnalyzerEngine::RustAnalyzer
    }));
    assert!(services
        .iter()
        .any(|service| service.id == "typescript-parser"));
    assert!(services.iter().any(|service| service.id == "qml-parser"));
    assert!(services.iter().all(|service| {
        service.provider == AnalyzerProvider::Local
            && !service.billable
            && service.credits_used.is_none()
    }));
}

#[test]
fn analyzer_services_include_python_ty_record_when_ready() {
    let services = analyzer_services_from_counts(
        AnalyzerStatus::Ready,
        None,
        Some(PythonAnalyzerStatus {
            mode: "auto".into(),
            status: "ty ready".into(),
            message: None,
        }),
        None,
        None,
        AnalyzerFileCounts {
            python: 5,
            ..AnalyzerFileCounts::default()
        },
        None,
    );
    let ty = services
        .iter()
        .find(|service| service.id == "python-ty")
        .expect("python ty analyzer record");

    assert_eq!(ty.status, AnalyzerStatus::Ready);
    assert!(ty.capabilities.contains(&AnalyzerCapability::Diagnostics));
    assert_eq!(ty.files_indexed, 5);
    assert_local_analyzer(ty);
}

#[test]
fn analyzer_services_report_python_parser_fallback() {
    let services = analyzer_services_from_counts(
        AnalyzerStatus::Ready,
        None,
        Some(PythonAnalyzerStatus {
            mode: "auto".into(),
            status: "ty unavailable".into(),
            message: Some("missing ty".into()),
        }),
        None,
        None,
        AnalyzerFileCounts {
            python: 7,
            ..AnalyzerFileCounts::default()
        },
        None,
    );

    assert!(services.iter().any(|service| {
        service.id == "python-ty" && service.status == AnalyzerStatus::Fallback
    }));
    assert!(services.iter().any(|service| {
        service.id == "python-parser"
            && service.status == AnalyzerStatus::Ready
            && service.capabilities == vec![AnalyzerCapability::Symbols]
    }));
}

#[test]
fn analyzer_services_report_typescript_language_server_when_ready() {
    let services = analyzer_services_from_counts(
        AnalyzerStatus::Ready,
        None,
        None,
        Some(TypeScriptAnalyzerStatus {
            mode: "auto".into(),
            status: "language server ready".into(),
            message: None,
        }),
        None,
        AnalyzerFileCounts {
            typescript: 9,
            ..AnalyzerFileCounts::default()
        },
        None,
    );
    let service = services
        .iter()
        .find(|service| service.id == "typescript-language-server")
        .expect("typescript language server analyzer record");

    assert_eq!(service.status, AnalyzerStatus::Ready);
    assert_eq!(service.engine, AnalyzerEngine::TypeScriptLanguageServer);
    assert!(service
        .capabilities
        .contains(&AnalyzerCapability::References));
    assert!(!services
        .iter()
        .any(|service| service.id == "typescript-parser"));
}

#[test]
fn analyzer_services_report_typescript_parser_fallback() {
    let services = analyzer_services_from_counts(
        AnalyzerStatus::Ready,
        None,
        None,
        Some(TypeScriptAnalyzerStatus {
            mode: "auto".into(),
            status: "language server unavailable".into(),
            message: Some("missing typescript-language-server".into()),
        }),
        None,
        AnalyzerFileCounts {
            typescript: 4,
            ..AnalyzerFileCounts::default()
        },
        None,
    );

    assert!(services.iter().any(|service| {
        service.id == "typescript-language-server" && service.status == AnalyzerStatus::Fallback
    }));
    assert!(services.iter().any(|service| {
        service.id == "typescript-parser"
            && service.status == AnalyzerStatus::Ready
            && service.message.as_deref().is_some_and(|message| {
                message.contains("pnpm add -D typescript typescript-language-server")
            })
    }));
}

#[test]
fn analyzer_services_report_qmlls_when_ready() {
    let services = analyzer_services_from_counts(
        AnalyzerStatus::Ready,
        None,
        None,
        None,
        Some(QmlAnalyzerStatus {
            mode: "auto".into(),
            status: "qmlls ready".into(),
            message: None,
        }),
        AnalyzerFileCounts {
            qml: 3,
            ..AnalyzerFileCounts::default()
        },
        None,
    );
    let service = services
        .iter()
        .find(|service| service.id == "qmlls")
        .expect("qmlls analyzer record");

    assert_eq!(service.status, AnalyzerStatus::Ready);
    assert_eq!(service.engine, AnalyzerEngine::QmlLanguageServer);
    assert!(service
        .capabilities
        .contains(&AnalyzerCapability::Diagnostics));
    assert!(!services.iter().any(|service| service.id == "qml-parser"));
}

#[test]
fn analyzer_services_report_qml_parser_fallback() {
    let services = analyzer_services_from_counts(
        AnalyzerStatus::Ready,
        None,
        None,
        None,
        Some(QmlAnalyzerStatus {
            mode: "auto".into(),
            status: "qmlls unavailable".into(),
            message: Some("missing qmlls".into()),
        }),
        AnalyzerFileCounts {
            qml: 2,
            ..AnalyzerFileCounts::default()
        },
        None,
    );

    assert!(services
        .iter()
        .any(|service| service.id == "qmlls" && service.status == AnalyzerStatus::Fallback));
    assert!(services.iter().any(|service| {
        service.id == "qml-parser"
            && service.status == AnalyzerStatus::Fallback
            && service
                .message
                .as_deref()
                .is_some_and(|message| message.contains("qmlls not found"))
    }));
}

#[test]
fn readme_documents_analyzer_setup() {
    let readme =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../README.md"))
            .expect("workspace README");

    assert!(readme.contains("## Analyzer Setup"));
    assert!(readme.contains("rustup component add rust-analyzer"));
    assert!(readme.contains("uv tool install ty"));
    assert!(readme.contains("pnpm add -D typescript typescript-language-server"));
    assert!(readme.contains("--typescript-analyzer auto|parser|typescript-language-server"));
    assert!(readme.contains("--qml-analyzer auto|parser|qmlls"));
    assert!(readme.contains("--qmlls-no-cmake-calls"));
}

#[test]
fn frontend_readme_documents_local_typescript_semantic_setup() {
    let readme = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../frontend/README.md"),
    )
    .expect("frontend README");

    assert!(readme.contains("## Optional TypeScript semantic analysis"));
    assert!(readme.contains("pnpm add -D typescript typescript-language-server"));
    assert!(readme.contains("parser"));
    assert!(readme.contains("node_modules/.bin/typescript-language-server"));
}

fn trace_snapshot() -> GraphSnapshot {
    let mut caller = test_node("useUsers", Some("frontend/useUsers.ts"), Some("frontend"));
    caller.id = "caller".into();
    caller.language = Some("typescript".into());
    caller.node_type = graph_core::NodeType::Hook;
    caller.reachability = Some(SourceReachability::Active);
    let mut endpoint = test_node("GET /api/users", Some("src/main.rs"), Some("crate root"));
    endpoint.id = "endpoint".into();
    endpoint.node_type = graph_core::NodeType::Endpoint;
    endpoint.reachability = Some(SourceReachability::Active);
    let mut handler = test_node("users", Some("src/main.rs"), Some("crate root"));
    handler.id = "handler".into();
    handler.reachability = Some(SourceReachability::Active);
    handler.signature = Some("async fn users() -> Json<Vec<User>>".into());
    let mut service = test_node("list_users", Some("src/service.rs"), Some("service"));
    service.id = "service".into();
    service.reachability = Some(SourceReachability::Active);
    let mut model = test_node("User", Some("src/model.rs"), Some("model"));
    model.id = "model".into();
    model.node_type = graph_core::NodeType::Struct;
    model.reachability = Some(SourceReachability::Active);
    let mut detached = test_node("GET /api/scratch", Some("src/scratch.rs"), Some("scratch"));
    detached.id = "detached-endpoint".into();
    detached.node_type = graph_core::NodeType::Endpoint;
    detached.reachability = Some(SourceReachability::Detached);

    let mut request = test_edge(
        EdgeType::DataFlow,
        "caller",
        "endpoint",
        EdgeConfidence::Semantic,
    );
    request.id = "request".into();
    request.data_flow_kind = Some(graph_core::DataFlowKind::ApiRequest);
    request.evidence = Some("fetch('/api/users')".into());
    let mut response = test_edge(
        EdgeType::DataFlow,
        "handler",
        "endpoint",
        EdgeConfidence::Semantic,
    );
    response.id = "response".into();
    response.data_flow_kind = Some(graph_core::DataFlowKind::ApiResponse);
    response.evidence = Some("Json<Vec<User>>".into());
    let mut model_flow = test_edge(
        EdgeType::DataFlow,
        "service",
        "model",
        EdgeConfidence::Semantic,
    );
    model_flow.id = "model-flow".into();
    model_flow.data_flow_kind = Some(graph_core::DataFlowKind::ModelUse);

    GraphSnapshot {
        nodes: vec![caller, endpoint, handler, service, model, detached],
        edges: vec![
            test_edge(
                EdgeType::ApiCall,
                "caller",
                "endpoint",
                EdgeConfidence::Semantic,
            ),
            request,
            test_edge(
                EdgeType::EndpointHandler,
                "endpoint",
                "handler",
                EdgeConfidence::Exact,
            ),
            test_edge(
                EdgeType::Calls,
                "handler",
                "service",
                EdgeConfidence::Semantic,
            ),
            response,
            model_flow,
        ],
        files: Vec::new(),
        events: Vec::new(),
        status: AppStatus::empty(),
    }
}

#[test]
fn route_trace_includes_api_endpoint_handler_and_response_steps() {
    let snapshot = trace_snapshot();
    let endpoint = snapshot
        .nodes
        .iter()
        .find(|node| node.id == "endpoint")
        .unwrap();
    let trace = build_route_trace(&snapshot, endpoint);
    let kinds = trace.steps.iter().map(|step| step.kind).collect::<Vec<_>>();
    assert!(kinds.contains(&TraceStepKind::ApiRequest));
    assert!(kinds.contains(&TraceStepKind::Endpoint));
    assert!(kinds.contains(&TraceStepKind::EndpointHandler));
    assert!(kinds.contains(&TraceStepKind::BackendHandler));
    assert!(kinds.contains(&TraceStepKind::ServiceCall));
    assert!(kinds.contains(&TraceStepKind::ApiResponse));
    assert_eq!(trace.route_key.as_deref(), Some("GET /api/users"));
}

#[test]
fn detached_selected_node_returns_trace_warning() {
    let snapshot = trace_snapshot();
    let detached = snapshot
        .nodes
        .iter()
        .find(|node| node.id == "detached-endpoint")
        .unwrap();
    let trace = build_node_trace(&snapshot, detached);
    assert!(trace
        .warnings
        .iter()
        .any(|warning| warning.contains("detached")));
    assert!(trace
        .steps
        .iter()
        .any(|step| step.kind == TraceStepKind::DetachedSource));
}

#[test]
fn route_trace_query_lookup_uses_method_and_path_key() {
    let snapshot = trace_snapshot();
    let key = graph_core::route_key("get", "/api/users").key;
    let endpoint = graph_query::find_active_endpoint_by_route_key(&snapshot, &key).unwrap();
    let trace = build_route_trace(&snapshot, endpoint);

    assert_eq!(trace.route_key.as_deref(), Some("GET /api/users"));
    assert!(trace.summary.contains("Route GET /api/users"));
}

#[test]
fn ambiguous_active_route_trace_emits_warning() {
    let mut snapshot = trace_snapshot();
    let mut duplicate = snapshot
        .nodes
        .iter()
        .find(|node| node.id == "endpoint")
        .unwrap()
        .clone();
    duplicate.id = "endpoint-duplicate".into();
    duplicate.file = Some("src/other.rs".into());
    snapshot.nodes.push(duplicate);

    let endpoint = snapshot
        .nodes
        .iter()
        .find(|node| node.id == "endpoint")
        .unwrap();
    let trace = build_route_trace(&snapshot, endpoint);

    assert!(trace
        .warnings
        .iter()
        .any(|warning| warning.contains("Multiple active endpoint")));
}

#[test]
fn trace_excludes_generated_neighbors_by_default() {
    let mut snapshot = trace_snapshot();
    let mut generated = test_node("generated", Some("target/out.rs"), Some("generated"));
    generated.id = "generated".into();
    generated.reachability = Some(SourceReachability::Generated);
    snapshot.nodes.push(generated);
    snapshot.edges.push(test_edge(
        EdgeType::Calls,
        "handler",
        "generated",
        EdgeConfidence::Semantic,
    ));
    let handler = snapshot
        .nodes
        .iter()
        .find(|node| node.id == "handler")
        .unwrap();

    let trace = build_node_trace(&snapshot, handler);

    assert!(!trace
        .steps
        .iter()
        .any(|step| step.node_id.as_deref() == Some("generated")));
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

fn temp_project_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("rust-watcher-{name}-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn missing_layout_file_is_not_an_error() {
    let root = temp_project_root("missing-layout");
    let layout = load_layout(&root).unwrap();
    assert!(layout.nodes.is_empty());
    let _ = std::fs::remove_dir_all(storage_dir_for_project(&root));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn corrupt_layout_file_is_renamed_and_ignored() {
    let root = temp_project_root("corrupt-layout");
    let path = layout_path(&root);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{not-json").unwrap();

    let layout = load_layout(&root).unwrap();

    assert!(layout.nodes.is_empty());
    assert!(!path.exists());
    let backups = std::fs::read_dir(storage_dir_for_project(&root))
        .unwrap()
        .filter_map(std::result::Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("layout.corrupt.")
        })
        .count();
    assert_eq!(backups, 1);
    let _ = std::fs::remove_dir_all(storage_dir_for_project(&root));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn layout_save_load_roundtrip_and_clear() {
    let root = temp_project_root("layout-roundtrip");
    let layout = LayoutStore {
        nodes: HashMap::from([(
            "node:a".into(),
            LayoutNode {
                node_id: "node:a".into(),
                x: 12.0,
                y: -8.0,
                vx: 0.5,
                vy: -0.25,
                pinned: Some(true),
                updated_at: "1".into(),
            },
        )]),
    };
    save_layout(&root, &layout).unwrap();
    let loaded = load_layout(&root).unwrap();
    assert_eq!(loaded.nodes["node:a"].x, 12.0);
    assert_eq!(loaded.nodes["node:a"].pinned, Some(true));
    clear_layout(&root).unwrap();
    assert!(load_layout(&root).unwrap().nodes.is_empty());
    let _ = std::fs::remove_dir_all(storage_dir_for_project(&root));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn layout_applies_to_snapshot_and_ignores_stale_nodes() {
    let mut node = test_node("main", Some("src/main.rs"), Some("app"));
    node.id = "node:live".into();
    let mut snapshot = GraphSnapshot {
        nodes: vec![node],
        edges: Vec::new(),
        files: Vec::new(),
        events: Vec::new(),
        status: AppStatus::empty(),
    };
    let layout = LayoutStore {
        nodes: HashMap::from([
            (
                "node:live".into(),
                LayoutNode {
                    node_id: "node:live".into(),
                    x: 42.0,
                    y: 9.0,
                    vx: 0.0,
                    vy: 0.0,
                    pinned: Some(true),
                    updated_at: "1".into(),
                },
            ),
            (
                "node:stale".into(),
                LayoutNode {
                    node_id: "node:stale".into(),
                    x: 1.0,
                    y: 1.0,
                    vx: 1.0,
                    vy: 1.0,
                    pinned: Some(true),
                    updated_at: "1".into(),
                },
            ),
        ]),
    };
    apply_layout_store_to_snapshot(&mut snapshot, &layout);
    assert_eq!(snapshot.nodes.len(), 1);
    assert_eq!(snapshot.nodes[0].x, 42.0);
    assert_eq!(snapshot.nodes[0].pinned, Some(true));
}

#[test]
fn saved_views_roundtrip() {
    let root = temp_project_root("views-roundtrip");
    let views = SavedViewsStore {
        views: vec![SavedView {
            id: "view:1".into(),
            name: "Backend".into(),
            filters: serde_json::json!({ "languages": ["rust"] }),
            focused_node_id: Some("node:a".into()),
            collapsed_groups: vec!["file:src/main.rs".into()],
            layout_overrides: serde_json::json!({}),
            created_at: "1".into(),
            updated_at: "2".into(),
        }],
    };
    save_views(&root, &views).unwrap();
    let loaded = load_views(&root).unwrap();
    assert_eq!(loaded.views.len(), 1);
    assert_eq!(loaded.views[0].name, "Backend");
    assert_eq!(loaded.views[0].collapsed_groups, vec!["file:src/main.rs"]);
    let _ = std::fs::remove_dir_all(storage_dir_for_project(&root));
    let _ = std::fs::remove_dir_all(root);
}
