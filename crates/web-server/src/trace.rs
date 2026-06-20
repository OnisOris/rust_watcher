use graph_core::{
    EdgeConfidence, EdgeType, GraphNode, GraphSnapshot, SourceReachability, TraceExplanation,
    TraceKind, TraceStep, TraceStepKind,
};
use std::collections::{HashMap, HashSet};

const MAX_TRACE_STEPS: usize = 20;

pub fn build_route_trace(graph: &GraphSnapshot, endpoint: &GraphNode) -> TraceExplanation {
    let route = graph_core::route_key_from_label(&endpoint.label);
    let mut builder = TraceBuilder::new(
        TraceKind::Route,
        endpoint.id.clone(),
        format!("Route trace: {}", endpoint.label),
        endpoint.id.clone(),
        route.as_ref().map(|route| route.key.clone()),
    );
    warn_if_unreachable(&mut builder, endpoint);
    let nodes = nodes_by_id(graph);
    let incoming = incoming_edges(graph, &endpoint.id);
    let outgoing = outgoing_edges(graph, &endpoint.id);
    let matching_active_endpoints =
        active_endpoint_count_for_route(graph, route.as_ref().map(|route| route.key.as_str()));
    if matching_active_endpoints > 1 {
        builder
            .trace
            .warnings
            .push("Multiple active endpoint implementations match this route.".to_string());
    }

    if endpoint.reachability == Some(SourceReachability::Detached) {
        builder.node_step(
            TraceStepKind::DetachedSource,
            endpoint,
            "Detached source",
            "Endpoint is not reachable from active program entrypoints",
        );
    }
    for edge in ranked_edges(
        incoming
            .into_iter()
            .filter(|edge| edge.edge_type == EdgeType::ApiCall),
    ) {
        if let Some(caller) = nodes.get(edge.source.as_str()) {
            builder.node_step(TraceStepKind::Caller, caller, "Caller", "Calls this route");
        }
        builder.edge_step(TraceStepKind::ApiRequest, edge, &nodes, "API request");
    }
    for edge in ranked_edges(
        incoming_edges(graph, &endpoint.id)
            .into_iter()
            .filter(|edge| edge.edge_type == EdgeType::DataFlow)
            .filter(|edge| edge.data_flow_kind == Some(graph_core::DataFlowKind::ApiRequest)),
    ) {
        builder.edge_step(TraceStepKind::ApiRequest, edge, &nodes, "Request data flow");
    }
    builder.node_step(
        TraceStepKind::Endpoint,
        endpoint,
        "Endpoint",
        "Route node matched by method and path",
    );
    for edge in ranked_edges(
        outgoing
            .into_iter()
            .filter(|edge| edge.edge_type == EdgeType::EndpointHandler),
    ) {
        builder.edge_step(
            TraceStepKind::EndpointHandler,
            edge,
            &nodes,
            "Endpoint handler",
        );
        if let Some(handler) = nodes.get(edge.target.as_str()) {
            builder.node_step(
                TraceStepKind::BackendHandler,
                handler,
                "Backend handler",
                "Function handling the route",
            );
            for call in ranked_edges(
                outgoing_edges(graph, &handler.id)
                    .into_iter()
                    .filter(|edge| edge.edge_type == EdgeType::Calls),
            ) {
                builder.edge_step(TraceStepKind::ServiceCall, call, &nodes, "Service call");
            }
            for flow in ranked_edges(graph.edges.iter().filter(|flow| {
                flow.edge_type == EdgeType::DataFlow
                    && (flow.source == handler.id
                        || flow.target == handler.id
                        || flow.target == endpoint.id)
                    && matches!(
                        flow.data_flow_kind,
                        Some(
                            graph_core::DataFlowKind::ReturnValue
                                | graph_core::DataFlowKind::ModelUse
                                | graph_core::DataFlowKind::ApiResponse
                        )
                    )
            })) {
                builder.edge_step(
                    trace_step_kind_for_edge(flow),
                    flow,
                    &nodes,
                    "Response data flow",
                );
            }
        }
    }
    builder.finish()
}

pub fn build_edge_trace(graph: &GraphSnapshot, edge: &graph_core::GraphEdge) -> TraceExplanation {
    let nodes = nodes_by_id(graph);
    let title = if edge.edge_type == EdgeType::DataFlow {
        format!(
            "Data flow trace: {}",
            edge.data_flow_kind
                .map(|kind| format!("{kind:?}"))
                .unwrap_or_else(|| "Unknown".to_string())
        )
    } else {
        format!("Edge trace: {:?}", edge.edge_type)
    };
    let mut builder = TraceBuilder::new(
        if edge.edge_type == EdgeType::DataFlow {
            TraceKind::DataFlow
        } else {
            TraceKind::NodeNeighborhood
        },
        edge.id.clone(),
        title,
        edge.source.clone(),
        None,
    );
    if let Some(source) = nodes.get(edge.source.as_str()) {
        warn_if_unreachable(&mut builder, source);
        builder.node_step(TraceStepKind::Caller, source, "Source", "Trace edge source");
    }
    builder.edge_step(
        trace_step_kind_for_edge(edge),
        edge,
        &nodes,
        "Selected edge",
    );
    if let Some(target) = nodes.get(edge.target.as_str()) {
        warn_if_unreachable(&mut builder, target);
        builder.node_step(
            TraceStepKind::Endpoint,
            target,
            "Target",
            "Trace edge target",
        );
    }
    let source_label = nodes
        .get(edge.source.as_str())
        .map(|node| node.label.as_str())
        .unwrap_or("unknown");
    let target_label = nodes
        .get(edge.target.as_str())
        .map(|node| node.label.as_str())
        .unwrap_or("unknown");
    if edge.edge_type == EdgeType::DataFlow {
        let kind = edge
            .data_flow_kind
            .map(|kind| format!("{kind:?}"))
            .unwrap_or_else(|| "Unknown".to_string());
        builder.trace.summary =
            format!("Data flows from {source_label} to {target_label} through {kind}.");
    }
    for related in ranked_edges(graph.edges.iter().filter(|candidate| {
        candidate.id != edge.id
            && (candidate.source == edge.source
                || candidate.target == edge.source
                || candidate.source == edge.target
                || candidate.target == edge.target)
            && matches!(
                candidate.edge_type,
                EdgeType::ApiCall
                    | EdgeType::EndpointHandler
                    | EdgeType::Calls
                    | EdgeType::DataFlow
            )
    })) {
        builder.edge_step(
            trace_step_kind_for_edge(related),
            related,
            &nodes,
            "Related edge",
        );
    }
    builder.finish()
}

pub fn build_node_trace(graph: &GraphSnapshot, node: &GraphNode) -> TraceExplanation {
    if node.node_type == graph_core::NodeType::Endpoint {
        return build_route_trace(graph, node);
    }
    let nodes = nodes_by_id(graph);
    let mut builder = TraceBuilder::new(
        TraceKind::NodeNeighborhood,
        node.id.clone(),
        format!("Node trace: {}", node.label),
        node.id.clone(),
        None,
    );
    warn_if_unreachable(&mut builder, node);
    builder.node_step(
        trace_step_kind_for_node(node),
        node,
        "Selected node",
        "Trace root node",
    );
    for edge in ranked_edges(graph.edges.iter().filter(|edge| {
        (edge.source == node.id || edge.target == node.id)
            && matches!(
                edge.edge_type,
                EdgeType::DataFlow
                    | EdgeType::Calls
                    | EdgeType::ApiCall
                    | EdgeType::EndpointHandler
            )
    })) {
        builder.edge_step(
            trace_step_kind_for_edge(edge),
            edge,
            &nodes,
            "Neighborhood edge",
        );
    }
    let relevant_links = builder
        .trace
        .steps
        .iter()
        .filter(|step| step.edge_id.is_some())
        .count();
    builder.trace.summary = format!(
        "Node {} has {relevant_links} relevant incoming/outgoing trace link{}.",
        node.label,
        if relevant_links == 1 { "" } else { "s" }
    );
    builder.finish()
}

struct TraceBuilder {
    trace: TraceExplanation,
    seen_steps: HashSet<String>,
}

impl TraceBuilder {
    fn new(
        kind: TraceKind,
        id_seed: String,
        title: String,
        root_node_id: String,
        route_key: Option<String>,
    ) -> Self {
        Self {
            trace: TraceExplanation {
                id: format!("trace:{kind:?}:{id_seed}"),
                kind,
                title,
                summary: String::new(),
                steps: Vec::new(),
                warnings: Vec::new(),
                root_node_id: Some(root_node_id),
                route_key,
                created_at: current_timestamp(),
            },
            seen_steps: HashSet::new(),
        }
    }

    fn node_step(&mut self, kind: TraceStepKind, node: &GraphNode, title: &str, description: &str) {
        if !trace_node_allowed(node, self.trace.root_node_id.as_deref()) {
            return;
        }
        self.push_step(TraceStep {
            id: format!("node:{}:{kind:?}", node.id),
            kind,
            node_id: Some(node.id.clone()),
            edge_id: None,
            title: format!("{title}: {}", node.label),
            description: description.to_string(),
            language: node.language.clone(),
            file: node.file.clone(),
            line: node.line,
            confidence: None,
            evidence: node.signature.clone().or_else(|| node.description.clone()),
            reachability: node.reachability,
        });
    }

    fn edge_step(
        &mut self,
        kind: TraceStepKind,
        edge: &graph_core::GraphEdge,
        nodes: &HashMap<&str, &GraphNode>,
        title: &str,
    ) {
        let source = nodes.get(edge.source.as_str()).copied();
        let target = nodes.get(edge.target.as_str()).copied();
        if source.is_some_and(|node| !trace_node_allowed(node, self.trace.root_node_id.as_deref()))
            || target
                .is_some_and(|node| !trace_node_allowed(node, self.trace.root_node_id.as_deref()))
        {
            return;
        }
        let source_label = source.map(|node| node.label.as_str()).unwrap_or("unknown");
        let target_label = target.map(|node| node.label.as_str()).unwrap_or("unknown");
        self.push_step(TraceStep {
            id: format!("edge:{}:{kind:?}", edge.id),
            kind,
            node_id: target.map(|node| node.id.clone()),
            edge_id: Some(edge.id.clone()),
            title: format!("{title}: {source_label} -> {target_label}"),
            description: edge
                .label
                .clone()
                .unwrap_or_else(|| format!("{:?}", edge.edge_type)),
            language: target.and_then(|node| node.language.clone()),
            file: target.and_then(|node| node.file.clone()),
            line: target.and_then(|node| node.line),
            confidence: Some(edge.confidence),
            evidence: edge.evidence.clone(),
            reachability: target.and_then(|node| node.reachability),
        });
    }

    fn push_step(&mut self, step: TraceStep) {
        if self.trace.steps.len() >= MAX_TRACE_STEPS {
            if !self
                .trace
                .warnings
                .iter()
                .any(|warning| warning.contains("truncated"))
            {
                self.trace
                    .warnings
                    .push(format!("Trace truncated to {MAX_TRACE_STEPS} steps."));
            }
            return;
        }
        if self.seen_steps.insert(step.id.clone()) {
            self.trace.steps.push(step);
        }
    }

    fn finish(mut self) -> TraceExplanation {
        if self.trace.summary.is_empty() {
            self.trace.summary = match self.trace.kind {
                TraceKind::Route => route_trace_summary(&self.trace),
                TraceKind::DataFlow => format!(
                    "{} data-flow step{} generated from the current graph.",
                    self.trace.steps.len(),
                    if self.trace.steps.len() == 1 { "" } else { "s" }
                ),
                TraceKind::NodeNeighborhood => format!(
                    "{} neighborhood step{} generated from the current graph.",
                    self.trace.steps.len(),
                    if self.trace.steps.len() == 1 { "" } else { "s" }
                ),
            };
        }
        self.trace
    }
}

fn route_trace_summary(trace: &TraceExplanation) -> String {
    let route = trace.route_key.as_deref().unwrap_or("selected route");
    let frontend_callers = trace
        .steps
        .iter()
        .filter(|step| step.kind == TraceStepKind::Caller)
        .filter(|step| {
            matches!(
                step.language.as_deref(),
                Some("typescript" | "javascript" | "qml")
            )
        })
        .count();
    let backend_handlers = trace
        .steps
        .iter()
        .filter(|step| step.kind == TraceStepKind::BackendHandler)
        .count();
    format!(
        "Route {route} is called by {frontend_callers} frontend/QML node{} and handled by {backend_handlers} backend function{}.",
        if frontend_callers == 1 { "" } else { "s" },
        if backend_handlers == 1 { "" } else { "s" },
    )
}

fn active_endpoint_count_for_route(graph: &GraphSnapshot, route_key: Option<&str>) -> usize {
    let Some(route_key) = route_key else {
        return 0;
    };
    graph
        .nodes
        .iter()
        .filter(|node| node.node_type == graph_core::NodeType::Endpoint)
        .filter(|node| active_trace_node(node))
        .filter(|node| {
            graph_core::route_key_from_label(&node.label)
                .is_some_and(|route| route.key == route_key)
        })
        .count()
}

fn warn_if_unreachable(builder: &mut TraceBuilder, node: &GraphNode) {
    match node.reachability {
        Some(SourceReachability::Detached) => {
            builder.trace.warnings.push(
                "This node is detached and is not reachable from the active program entrypoints."
                    .to_string(),
            );
        }
        Some(SourceReachability::Generated) => {
            builder.trace.warnings.push(
                "This node is generated and excluded from active traces by default.".to_string(),
            );
        }
        _ => {}
    }
}

fn trace_node_allowed(node: &GraphNode, root_node_id: Option<&str>) -> bool {
    if root_node_id == Some(node.id.as_str()) {
        return true;
    }
    !matches!(
        node.reachability,
        Some(SourceReachability::Detached | SourceReachability::Generated)
    )
}

pub fn active_trace_node(node: &GraphNode) -> bool {
    !matches!(
        node.reachability,
        Some(SourceReachability::Detached | SourceReachability::Generated)
    )
}

fn nodes_by_id(graph: &GraphSnapshot) -> HashMap<&str, &GraphNode> {
    graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect()
}

fn incoming_edges<'a>(
    graph: &'a GraphSnapshot,
    node_id: &'a str,
) -> Vec<&'a graph_core::GraphEdge> {
    graph
        .edges
        .iter()
        .filter(|edge| edge.target == node_id)
        .collect()
}

fn outgoing_edges<'a>(
    graph: &'a GraphSnapshot,
    node_id: &'a str,
) -> Vec<&'a graph_core::GraphEdge> {
    graph
        .edges
        .iter()
        .filter(|edge| edge.source == node_id)
        .collect()
}

fn ranked_edges<'a, I>(edges: I) -> Vec<&'a graph_core::GraphEdge>
where
    I: Iterator<Item = &'a graph_core::GraphEdge>,
{
    let mut edges = edges.collect::<Vec<_>>();
    edges.sort_by(|left, right| {
        trace_edge_rank(right)
            .cmp(&trace_edge_rank(left))
            .then(left.id.cmp(&right.id))
    });
    edges
}

fn trace_edge_rank(edge: &graph_core::GraphEdge) -> u8 {
    let confidence = match edge.confidence {
        EdgeConfidence::Exact => 4,
        EdgeConfidence::Semantic => 3,
        EdgeConfidence::SyntaxFallback => 2,
        EdgeConfidence::Heuristic => 1,
    };
    let edge_type = match edge.edge_type {
        EdgeType::ApiCall | EdgeType::EndpointHandler => 4,
        EdgeType::DataFlow => 3,
        EdgeType::Calls => 2,
        _ => 1,
    };
    confidence + edge_type + u8::from(edge.evidence.is_some())
}

fn trace_step_kind_for_node(node: &GraphNode) -> TraceStepKind {
    if node.reachability == Some(SourceReachability::Detached) {
        return TraceStepKind::DetachedSource;
    }
    if node.reachability == Some(SourceReachability::External)
        || node.node_type == graph_core::NodeType::ExternalCrate
    {
        return TraceStepKind::ExternalDependency;
    }
    if node.node_type == graph_core::NodeType::Endpoint {
        TraceStepKind::Endpoint
    } else {
        TraceStepKind::Unknown
    }
}

fn trace_step_kind_for_edge(edge: &graph_core::GraphEdge) -> TraceStepKind {
    match edge.edge_type {
        EdgeType::ApiCall => TraceStepKind::ApiRequest,
        EdgeType::EndpointHandler => TraceStepKind::EndpointHandler,
        EdgeType::Calls => TraceStepKind::ServiceCall,
        EdgeType::ExternalDependency => TraceStepKind::ExternalDependency,
        EdgeType::DataFlow => match edge.data_flow_kind {
            Some(graph_core::DataFlowKind::ApiRequest) => TraceStepKind::ApiRequest,
            Some(graph_core::DataFlowKind::ApiResponse) => TraceStepKind::ApiResponse,
            Some(graph_core::DataFlowKind::ReturnValue) => TraceStepKind::ReturnValue,
            Some(graph_core::DataFlowKind::ModelUse) => TraceStepKind::ModelUse,
            Some(graph_core::DataFlowKind::StateUpdate) => TraceStepKind::StateUpdate,
            Some(graph_core::DataFlowKind::PropertyBinding) => TraceStepKind::PropertyBinding,
            _ => TraceStepKind::Unknown,
        },
        _ => TraceStepKind::Unknown,
    }
}

fn current_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
