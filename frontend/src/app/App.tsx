import { useState, useCallback, useEffect, useMemo, useRef } from 'react'
import { TopToolbar } from './components/TopToolbar'
import { ProjectExplorer } from './components/ProjectExplorer'
import { LiveCodeGraph } from './components/LiveCodeGraph'
import { InspectorPanel } from './components/InspectorPanel'
import { AnalysisTimeline } from './components/AnalysisTimeline'
import { FilterBar } from './components/FilterBar'
import { SearchCommandPalette } from './components/SearchCommandPalette'
import { EmptyState } from './components/EmptyState'
import { DenseGraphSuggestion } from './components/DenseGraphSuggestion'
import { useBackendGraph } from './api/useBackendGraph'
import {
  applyCollapsedGroups,
  applyGraphFilters,
  applyGraphMode,
  buildNeighborhoodGraph,
  buildCollapsedGroupStats,
  buildRouteFlowGraph,
  bundleEdges,
} from './api/graphView'
import { applySavedViewState, normalizeSavedView, serializableFilters } from './api/savedViews'
import { deriveTraceHighlights, type TraceHighlights } from './api/trace'
import { DEFAULT_GRAPH_LAYOUT_SETTINGS } from './types'
import { formatUpdatedLabel } from './utils/time'
import type { GraphMode, GraphFilters, NodeType, EdgeType, ThemeMode, GraphNode, GraphEdge, GraphLayoutSettings, GraphLabelMode, GraphLayoutMode, LanguageFilter, SavedView, TraceExplanation } from './types'

const ALL_NODE_TYPES = new Set<NodeType>(['File', 'Module', 'Struct', 'Class', 'Object', 'Enum', 'Trait', 'Impl', 'Function', 'Method', 'Component', 'Hook', 'Interface', 'TypeAlias', 'Property', 'Signal', 'Handler', 'Endpoint', 'Macro', 'ExternalCrate'])
const ALL_EDGE_TYPES = new Set<EdgeType>(['Contains', 'Imports', 'Uses', 'Calls', 'Renders', 'ApiCall', 'EndpointHandler', 'Implements', 'TypeReference', 'DataFlow', 'ModDeclaration', 'ExternalDependency'])
const ALL_LANGUAGES = new Set<LanguageFilter>(['rust', 'typescript', 'python', 'qml', 'external', 'endpoints'])

const DEFAULT_FILTERS: GraphFilters = {
  nodeTypes: ALL_NODE_TYPES,
  edgeTypes: ALL_EDGE_TYPES,
  languages: ALL_LANGUAGES,
  edgeVisibility: 'Semantic',
  showTests: true,
  showExternal: true,
  showDetached: false,
  onlyPublicAPI: false,
  depth: 'full',
  onlyCurrentFile: false,
}

const DEFAULT_COLLAPSED_GROUPS = new Set(['module:detached-rust-files'])

type GraphLens = 'all' | 'architecture' | 'api' | 'route'

const DEFAULT_VIEWS: SavedView[] = [
  { id: 'default-full', name: 'Full graph', filters: {}, focusedNodeId: null, collapsedGroups: [] },
  { id: 'default-api', name: 'Frontend API bridge', filters: { languages: new Set<LanguageFilter>(['typescript', 'qml', 'endpoints', 'rust', 'python']) }, focusedNodeId: null, collapsedGroups: [] },
  { id: 'default-route', name: 'Route Flow', filters: { edgeVisibility: 'Essential' }, focusedNodeId: null, collapsedGroups: [] },
  { id: 'default-rust', name: 'Rust backend', filters: { languages: new Set<LanguageFilter>(['rust', 'endpoints']) }, focusedNodeId: null, collapsedGroups: [] },
  { id: 'default-python', name: 'Python backend', filters: { languages: new Set<LanguageFilter>(['python', 'endpoints']) }, focusedNodeId: null, collapsedGroups: [] },
  { id: 'default-qml', name: 'QML UI', filters: { languages: new Set<LanguageFilter>(['qml', 'endpoints']) }, focusedNodeId: null, collapsedGroups: [] },
  { id: 'default-diagnostics', name: 'Diagnostics', filters: {}, focusedNodeId: null, collapsedGroups: [] },
]

function isEditableTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false
  const tagName = target.tagName.toLowerCase()
  return target.isContentEditable || tagName === 'input' || tagName === 'textarea' || tagName === 'select'
}

function initialTheme(): ThemeMode {
  const stored = localStorage.getItem('rust-watcher-theme')
  return stored === 'dark' ? 'dark' : 'light'
}

export default function App() {
  const [mode, setMode] = useState<GraphMode>('Macro')
  const [theme, setTheme] = useState<ThemeMode>(initialTheme)
  const [filters, setFilters] = useState<GraphFilters>(DEFAULT_FILTERS)
  const [timelineCollapsed, setTimelineCollapsed] = useState(true)
  const [searchOpen, setSearchOpen] = useState(false)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [clarityOpen, setClarityOpen] = useState(false)
  const [graphLens, setGraphLens] = useState<GraphLens>('all')
  const [neighborhoodNodeId, setNeighborhoodNodeId] = useState<string | null>(null)
  const [layoutMode, setLayoutMode] = useState<GraphLayoutMode>('Force')
  const layoutModeTouchedRef = useRef(false)
  const [labelMode, setLabelMode] = useState<GraphLabelMode>('auto')
  const [layoutSettings, setLayoutSettings] = useState<GraphLayoutSettings>(DEFAULT_GRAPH_LAYOUT_SETTINGS)
  const [recenterKey, setRecenterKey] = useState(0)
  const [pinnedNodeIds, setPinnedNodeIds] = useState<Set<string>>(new Set())
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(DEFAULT_COLLAPSED_GROUPS)
  const [userSavedViews, setUserSavedViews] = useState<SavedView[]>([])
  const [traceHighlights, setTraceHighlights] = useState<TraceHighlights | null>(null)
  const {
    appState,
    analyzerStatus,
    analyzers,
    pythonAnalyzer,
    projectName,
    lastUpdated,
    message,
    nodes,
    edges,
    files,
    events,
    diagnosticsByFile,
    diagnosticsByNode,
    selectedNodeId,
    setSelectedNodeId,
    openProject,
    openInEditor,
    saveNodeLayout,
    search,
    refreshSnapshot,
  } = useBackendGraph(mode)
  const savedLayoutRef = useRef<Map<string, string>>(new Map())

  const graphNodes = useMemo(
    () => nodes.map(node => ({
      ...node,
      pinned: pinnedNodeIds.has(node.id) || node.pinned,
    })),
    [nodes, pinnedNodeIds],
  )
  useEffect(() => {
    if (layoutModeTouchedRef.current) return
    if (nodes.length > 100) setLayoutMode('SemanticZones')
  }, [nodes.length])
  const selectedNode = selectedNodeId ? graphNodes.find(n => n.id === selectedNodeId) ?? null : null
  const handleTraceLoaded = useCallback((trace: TraceExplanation) => {
    setTraceHighlights(deriveTraceHighlights(trace))
  }, [])
  const clearTraceHighlight = useCallback(() => setTraceHighlights(null), [])
  const layoutTuned = layoutSettings.spacing !== DEFAULT_GRAPH_LAYOUT_SETTINGS.spacing
    || layoutSettings.repulsion !== DEFAULT_GRAPH_LAYOUT_SETTINGS.repulsion
    || layoutSettings.linkLength !== DEFAULT_GRAPH_LAYOUT_SETTINGS.linkLength
    || layoutSettings.damping !== DEFAULT_GRAPH_LAYOUT_SETTINGS.damping
  const { nodes: visibleGraphNodes, edges: visibleGraphEdges } = useMemo(
    () => {
      const modeGraph = graphLens === 'route'
        ? { nodes: graphNodes, edges }
        : applyGraphMode({ nodes: graphNodes, edges }, mode)
      const lensGraph = graphLens === 'route'
        ? buildRouteFlowGraph(modeGraph)
        : applyGraphLens(modeGraph.nodes, modeGraph.edges, graphLens)
      const neighborhoodGraph = neighborhoodNodeId
        ? buildNeighborhoodGraph(lensGraph, neighborhoodNodeId)
        : lensGraph
      const collapsedStats = buildCollapsedGroupStats(neighborhoodGraph, collapsedGroups, diagnosticsByNode)
      const annotatedGraph = {
        nodes: neighborhoodGraph.nodes.map(node => {
          const stats = collapsedStats.get(node.id)
          if (!stats) return node
          const edgeTypes = [...new Set([...stats.incomingEdgeTypes, ...stats.outgoingEdgeTypes])].slice(0, 3).join(', ')
          return {
            ...node,
            connections: stats.hiddenNodeCount,
            description: `Collapsed: ${stats.hiddenNodeCount} hidden · ${stats.hiddenDiagnosticCount} diagnostics${edgeTypes ? ` · ${edgeTypes}` : ''}`,
          }
        }),
        edges: neighborhoodGraph.edges,
      }
      const filteredGraph = applyCollapsedGroups(applyGraphFilters(annotatedGraph, filters), collapsedGroups)
      return { nodes: filteredGraph.nodes, edges: bundleEdges(filteredGraph.edges) }
    },
    [graphNodes, edges, mode, graphLens, neighborhoodNodeId, filters, collapsedGroups, diagnosticsByNode],
  )
  const zeroEdgeHint = visibleGraphNodes.length > 0 && visibleGraphEdges.length === 0
    ? graphModeEmptyHint(mode)
    : null

  const handleLayoutModeChange = useCallback((next: GraphLayoutMode) => {
    layoutModeTouchedRef.current = true
    setLayoutMode(next)
  }, [])

  const togglePinNode = useCallback((id: string) => {
    const node = graphNodes.find(node => node.id === id)
    const nextPinned = !(node?.pinned ?? pinnedNodeIds.has(id))
    setPinnedNodeIds(prev => {
      const next = new Set(prev)
      next.has(id) ? next.delete(id) : next.add(id)
      return next
    })
    if (node) {
      saveNodeLayout({ ...node, pinned: nextPinned, vx: 0, vy: 0 })
    }
  }, [graphNodes, pinnedNodeIds, saveNodeLayout])

  const unpinAll = useCallback(() => {
    setPinnedNodeIds(new Set())
    graphNodes.filter(node => node.pinned).forEach(node => {
      saveNodeLayout({ ...node, pinned: false, vx: 0, vy: 0 })
    })
  }, [graphNodes, saveNodeLayout])

  const toggleCollapseGroup = useCallback((id: string) => {
    setCollapsedGroups(prev => {
      const next = new Set(prev)
      next.has(id) ? next.delete(id) : next.add(id)
      return next
    })
  }, [])

  const handleUpdateNodes = useCallback((updatedNodes: GraphNode[]) => {
    const changedPinned = updatedNodes.filter(node => node.pinned)
    if (!changedPinned.length) return
    setPinnedNodeIds(prev => {
      let changed = false
      const next = new Set(prev)
      changedPinned.forEach(node => {
        if (!next.has(node.id)) {
          next.add(node.id)
          changed = true
        }
      })
      if (!changed) return prev
      return next
    })
    for (const node of changedPinned) {
      const signature = `${Math.round(node.x * 10) / 10}:${Math.round(node.y * 10) / 10}:${node.pinned ? 1 : 0}`
      if (savedLayoutRef.current.get(node.id) === signature) continue
      savedLayoutRef.current.set(node.id, signature)
      saveNodeLayout({ ...node, vx: 0, vy: 0, pinned: true })
    }
  }, [saveNodeLayout])

  const applySavedView = useCallback((view: SavedView) => {
    const next = applySavedViewState(filters, view)
    setFilters(next.filters)
    setCollapsedGroups(next.collapsedGroups)
    setSelectedNodeId(next.focusedNodeId)
    setNeighborhoodNodeId(null)
  }, [filters, setSelectedNodeId])

  const loadSavedViews = useCallback(async () => {
    try {
      const response = await fetch('/api/views')
      if (!response.ok) return
      const payload = await response.json() as { views: SavedView[] }
      setUserSavedViews((payload.views ?? []).map(normalizeSavedView))
    } catch {
      setUserSavedViews([])
    }
  }, [])

  useEffect(() => {
    loadSavedViews()
  }, [loadSavedViews, projectName])

  const saveCurrentView = useCallback(async () => {
    const name = window.prompt('Save graph view as')
    if (!name?.trim()) return
    try {
      const response = await fetch('/api/views', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          name: name.trim(),
          filters: serializableFilters(filters),
          focusedNodeId: selectedNodeId,
          collapsedGroups: [...collapsedGroups],
          // TODO: per-view layout overrides can build on the separate global layout store.
        }),
      })
      if (response.ok) {
        await loadSavedViews()
      }
    } catch {
      // Saved views are convenience state; graph operation should keep going if storage fails.
    }
  }, [collapsedGroups, filters, loadSavedViews, selectedNodeId])

  // keyboard shortcuts
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault()
        setSearchOpen(prev => !prev)
        return
      }

      if (e.metaKey || e.ctrlKey || e.altKey || isEditableTarget(e.target)) {
        return
      }

      const depthByKey: Record<string, GraphFilters['depth']> = {
        '1': 1,
        '2': 2,
        '3': 3,
        '4': 'full',
      }
      const depth = depthByKey[e.key]
      if (depth) {
        e.preventDefault()
        setFilters(current => current.depth === depth ? current : { ...current, depth })
      }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [])

  useEffect(() => {
    document.documentElement.dataset.theme = theme
    document.documentElement.classList.toggle('dark', theme === 'dark')
    localStorage.setItem('rust-watcher-theme', theme)
  }, [theme])

  const handleOpenProject = useCallback((path?: string) => {
    openProject(path)
  }, [openProject])

  const handleSelectNode = useCallback((id: string | null) => {
    setSelectedNodeId(id)
  }, [setSelectedNodeId])

  // ── Empty state ──────────────────────────────────────────────────────────
  if (appState === 'empty') {
    return (
      <div className="w-full h-full" style={{ background: 'var(--cc-bg)' }}>
        <EmptyState onOpenProject={handleOpenProject} />
      </div>
    )
  }

  // ── Indexing state ───────────────────────────────────────────────────────
  if (appState === 'indexing') {
    return (
      <div
        className="w-full h-full flex flex-col items-center justify-center"
        style={{ background: 'var(--cc-bg)', fontFamily: 'Inter, sans-serif' }}
      >
        <IndexingScreen />
      </div>
    )
  }

  // ── Main / Normal state ──────────────────────────────────────────────────
  return (
    <div className="w-full h-full flex flex-col overflow-hidden" style={{ background: 'var(--cc-bg)', fontFamily: 'Inter, sans-serif' }}>
      {/* toolbar */}
      <TopToolbar
        appState={appState}
        analyzerStatus={analyzerStatus}
        analyzers={analyzers}
        message={message}
        projectName={projectName}
        lastUpdated={lastUpdated}
        filesCount={files.length}
        mode={mode}
        onModeChange={setMode}
        layoutMode={layoutMode}
        onLayoutModeChange={handleLayoutModeChange}
        onSearchOpen={() => setSearchOpen(true)}
        onSettingsOpen={() => setSettingsOpen(true)}
        onRecenter={() => setRecenterKey(key => key + 1)}
        onCollapse={() => setGraphLens(current => current === 'architecture' ? 'all' : 'architecture')}
        onThemeToggle={() => setTheme(current => current === 'light' ? 'dark' : 'light')}
        onClarityToggle={() => setClarityOpen(open => !open)}
        clarityOpen={clarityOpen}
        clarityActive={graphLens !== 'all' || layoutMode !== 'Force' || labelMode !== 'auto' || layoutTuned || filters.edgeVisibility !== 'Semantic'}
        theme={theme}
      />

      {/* main area */}
      <div className="flex flex-1 min-h-0">
        {/* left panel */}
        <ProjectExplorer
          files={files}
          nodes={graphNodes}
          projectName={projectName ?? undefined}
          selectedNodeId={selectedNodeId}
          diagnosticsByFile={diagnosticsByFile}
          onSelectNode={handleSelectNode}
          onFocusFile={() => {}}
        />

        {/* graph area */}
        <div className="relative flex-1 min-w-0 overflow-hidden">
          <LiveCodeGraph
            nodes={visibleGraphNodes}
            edges={visibleGraphEdges}
            filters={filters}
            selectedNodeId={selectedNodeId}
            recenterKey={recenterKey}
            theme={theme}
            layoutSettings={layoutSettings}
            layoutMode={layoutMode}
            labelMode={labelMode}
            diagnosticsByNode={diagnosticsByNode}
            highlightedTraceNodeIds={traceHighlights?.nodeIds}
            highlightedTraceEdgeIds={traceHighlights?.edgeIds}
            onSelectNode={handleSelectNode}
            onUpdateNodes={handleUpdateNodes}
          />

          {zeroEdgeHint && (
            <div
              className="absolute left-1/2 top-24 z-20 -translate-x-1/2 rounded-xl px-4 py-3"
              style={{ background: 'var(--cc-overlay)', border: '1px solid var(--cc-border)', boxShadow: 'var(--cc-shadow)', maxWidth: 420, backdropFilter: 'blur(12px)' }}
            >
              <div style={{ fontSize: 12, color: 'var(--cc-text)', fontWeight: 750 }}>{zeroEdgeHint.title}</div>
              <div style={{ fontSize: 11, color: 'var(--cc-text-subtle)', lineHeight: 1.45, marginTop: 4 }}>{zeroEdgeHint.body}</div>
            </div>
          )}

          {layoutMode === 'SemanticZones' && graphNodes.length > 100 && !layoutModeTouchedRef.current && (
            <div
              className="absolute left-1/2 top-24 z-20 -translate-x-1/2 rounded-xl px-4 py-2"
              style={{ background: 'var(--cc-overlay)', border: '1px solid var(--cc-border)', boxShadow: 'var(--cc-shadow)', maxWidth: 430, backdropFilter: 'blur(12px)' }}
            >
              <div style={{ fontSize: 12, color: 'var(--cc-text)', fontWeight: 750 }}>Large project detected. Showing semantic zones.</div>
              <div style={{ fontSize: 11, color: 'var(--cc-text-subtle)', marginTop: 3 }}>Switch to Force graph anytime from the layout selector.</div>
            </div>
          )}

          {/* floating filter bar */}
          <FilterBar
            filters={filters}
            onFiltersChange={setFilters}
            savedViews={[...DEFAULT_VIEWS, ...userSavedViews]}
            onApplyView={applySavedView}
            onSaveView={saveCurrentView}
            onUnpinAll={unpinAll}
          />

          <div
            className="absolute top-3 right-5 z-20 transition-all duration-200 ease-out"
            style={{
              opacity: clarityOpen ? 1 : 0,
              transform: clarityOpen ? 'translateY(0) scale(1)' : 'translateY(-8px) scale(0.98)',
              transformOrigin: 'top right',
              pointerEvents: clarityOpen ? 'auto' : 'none',
            }}
          >
            <DenseGraphSuggestion
              graphLens={graphLens}
              totalNodes={graphNodes.length}
              visibleNodes={visibleGraphNodes.length}
              totalEdges={edges.length}
              visibleEdges={visibleGraphEdges.length}
              labelMode={labelMode}
              layoutSettings={layoutSettings}
              onDismiss={() => setClarityOpen(false)}
              onLensChange={setGraphLens}
              onLabelModeChange={setLabelMode}
              onLayoutSettingsChange={setLayoutSettings}
              onResetLayoutSettings={() => setLayoutSettings(DEFAULT_GRAPH_LAYOUT_SETTINGS)}
            />
          </div>

          {/* graph metadata overlay */}
          <div
            className="absolute bottom-3 left-3 flex items-center gap-3"
            style={{ pointerEvents: 'none' }}
          >
            <div className="flex items-center gap-2 rounded-lg px-3 py-1.5" style={{ background: 'var(--cc-overlay)', border: '1px solid var(--cc-border)', backdropFilter: 'blur(8px)' }}>
              <span style={{ fontSize: 10, color: 'var(--cc-text-subtle)' }}>Visible {visibleGraphNodes.length} nodes</span>
              <span style={{ color: 'var(--cc-border)' }}>·</span>
              <span style={{ fontSize: 10, color: 'var(--cc-text-subtle)' }}>{visibleGraphEdges.length} edges</span>
              <span style={{ color: 'var(--cc-border)' }}>·</span>
              <span style={{ fontSize: 10, color: 'var(--cc-text-faint)' }}>Total {graphNodes.length}/{edges.length}</span>
              {graphLens !== 'all' && (
                <>
                  <span style={{ color: 'var(--cc-border)' }}>·</span>
                  <span style={{ fontSize: 10, color: '#06B6D4' }}>
                    {graphLens === 'architecture' ? 'Architecture' : graphLens === 'route' ? 'Route Flow' : 'API Bridge'}: {visibleGraphNodes.length}/{visibleGraphEdges.length}
                  </span>
                </>
              )}
              <span style={{ color: 'var(--cc-border)' }}>·</span>
              <span style={{ fontSize: 10, color: 'var(--cc-text-subtle)' }}>{graphModeLabel(mode)}</span>
            </div>
            <div className="flex items-center gap-1.5 rounded-lg px-3 py-1.5" style={{ background: 'var(--cc-overlay)', border: '1px solid var(--cc-border)', backdropFilter: 'blur(8px)' }}>
              <div style={{ width: 6, height: 6, borderRadius: '50%', background: analyzerStatus === 'Error' ? '#F87171' : analyzerStatus === 'Indexing' ? '#F59E0B' : '#34D399' }} />
              <span style={{ fontSize: 10, color: 'var(--cc-text-subtle)' }}>{message ?? 'Live'} · {formatUpdatedLabel(lastUpdated)}</span>
            </div>
          </div>
        </div>

        {/* right panel */}
        <InspectorPanel
          selectedNode={selectedNode}
          nodes={visibleGraphNodes}
          edges={visibleGraphEdges}
          projectName={projectName}
          analyzerStatus={analyzerStatus}
          analyzers={analyzers}
          pythonAnalyzer={pythonAnalyzer}
          appState={appState}
          filesCount={files.length}
          totalNodes={graphNodes.length}
          totalEdges={edges.length}
          visibleNodes={visibleGraphNodes.length}
          visibleEdges={visibleGraphEdges.length}
          message={message}
          onTogglePin={togglePinNode}
          onToggleCollapse={toggleCollapseGroup}
          collapsedGroups={collapsedGroups}
          onShowNeighborhood={id => setNeighborhoodNodeId(current => current === id ? null : id)}
          neighborhoodNodeId={neighborhoodNodeId}
          onSelectNode={handleSelectNode}
          onOpenInEditor={openInEditor}
          onTraceLoaded={handleTraceLoaded}
          onClearTraceHighlight={clearTraceHighlight}
        />
      </div>

      {/* analysis timeline */}
      <AnalysisTimeline
        events={events}
        diagnosticsByFile={diagnosticsByFile}
        collapsed={timelineCollapsed}
        onToggle={() => setTimelineCollapsed(c => !c)}
      />

      {/* search command palette */}
      <SearchCommandPalette
        nodes={nodes}
        search={search}
        open={searchOpen}
        onClose={() => setSearchOpen(false)}
        onSelectNode={(id) => { handleSelectNode(id); setSearchOpen(false) }}
      />

      {/* settings modal placeholder */}
      {settingsOpen && <SettingsModal onClose={() => setSettingsOpen(false)} />}
    </div>
  )
}

function graphModeEmptyHint(mode: GraphMode) {
  switch (mode) {
    case 'CallFlow':
      return { title: 'No call flow edges detected', body: 'This mode expects endpoint-to-handler or function-call edges. Try Semantic or All edges, or check route extraction.' }
    case 'DataFlow':
      return { title: 'No data flow detected', body: 'This mode shows request parameters, DTOs and response types. Try Semantic edges or inspect handler signatures.' }
    case 'Traits':
      return { title: 'No type or implementation relations found', body: 'This mode shows Rust traits/impls plus class and type-reference relationships in other languages. Try enabling external dependencies and semantic edges.' }
    default:
      return { title: 'No edges in this view', body: 'The current filters hide all relationships. Try Full depth, Semantic edges, or enable detached and external sources.' }
  }
}

function graphModeLabel(mode: GraphMode) {
  return mode === 'Traits' ? 'Types & Impl' : mode
}

function applyGraphLens(nodes: GraphNode[], edges: GraphEdge[], lens: GraphLens) {
  if (lens === 'all') return { nodes, edges }

  const byId = new Map(nodes.map(node => [node.id, node]))
  const keep = new Set<string>()
  const keepEdgeTypes = new Set<EdgeType>()

  const keepContainerChain = (node: GraphNode) => {
    if (node.file) {
      const fileNode = nodes.find(candidate => candidate.type === 'File' && candidate.file === node.file)
      if (fileNode) keep.add(fileNode.id)
    }
    if (node.crate) {
      nodes
        .filter(candidate => candidate.type === 'Module' && candidate.crate === node.crate)
        .forEach(candidate => keep.add(candidate.id))
    }
  }

  if (lens === 'architecture') {
    nodes
      .filter(node => node.type === 'Module' || node.type === 'File' || node.type === 'Endpoint')
      .forEach(node => keep.add(node.id))
    keepEdgeTypes.add('Contains')
    keepEdgeTypes.add('Imports')
    keepEdgeTypes.add('ExternalDependency')
    keepEdgeTypes.add('Uses')
    keepEdgeTypes.add('ApiCall')
    keepEdgeTypes.add('EndpointHandler')
  } else {
    keepEdgeTypes.add('Contains')
    keepEdgeTypes.add('Calls')
    keepEdgeTypes.add('ApiCall')
    keepEdgeTypes.add('EndpointHandler')
  }

  edges
    .filter(edge => edge.type === 'ApiCall')
    .forEach(edge => {
      const source = byId.get(edge.source)
      const target = byId.get(edge.target)
      if (!source || !target) return
      keep.add(source.id)
      keep.add(target.id)
      keepContainerChain(source)
      keepContainerChain(target)
    })

  if (lens === 'api') {
    let grew = true
    while (grew) {
      grew = false
      for (const edge of edges) {
        if (edge.type !== 'Contains' && edge.type !== 'Calls' && edge.type !== 'EndpointHandler') continue
        const sourceKept = keep.has(edge.source)
        const targetKept = keep.has(edge.target)
        if (edge.type === 'Contains' && targetKept && !sourceKept) {
          if (!sourceKept) {
            keep.add(edge.source)
            grew = true
          }
        }
        if (edge.type === 'Calls' && sourceKept && !targetKept && byId.get(edge.target)?.type === 'Endpoint') {
          keep.add(edge.target)
          grew = true
        }
        if (edge.type === 'EndpointHandler' && sourceKept && !targetKept) {
          keep.add(edge.target)
          grew = true
        }
      }
    }
  }

  const filteredNodes = nodes.filter(node => keep.has(node.id))
  const filteredNodeIds = new Set(filteredNodes.map(node => node.id))
  const filteredEdges = edges.filter(edge =>
    keepEdgeTypes.has(edge.type) && filteredNodeIds.has(edge.source) && filteredNodeIds.has(edge.target)
  )

  return { nodes: filteredNodes, edges: filteredEdges }
}

// ── Indexing screen ─────────────────────────────────────────────────────────
function IndexingScreen() {
  const steps = [
    { label: 'Reading Cargo.toml…', done: true },
    { label: 'Discovering project scopes…', done: true },
    { label: 'Loading rust-analyzer…', done: true },
    { label: 'Indexing source files…', done: false, progress: 45 },
    { label: 'Building semantic graph…', done: false, pending: true },
    { label: 'Resolving type relationships…', done: false, pending: true },
  ]

  return (
    <div className="flex flex-col items-center" style={{ maxWidth: 420, width: '100%' }}>
      {/* animated logo */}
      <div className="relative mb-8">
        <div
          className="flex items-center justify-center rounded-2xl animate-pulse"
          style={{ width: 56, height: 56, background: 'linear-gradient(135deg, #06B6D4 0%, #7C3AED 100%)' }}
        >
          <span style={{ fontSize: 26 }}>⚡</span>
        </div>
        <div className="absolute -inset-3 rounded-3xl animate-ping opacity-20" style={{ background: 'linear-gradient(135deg, #06B6D4, #7C3AED)' }} />
      </div>

      <h2 style={{ fontSize: 20, fontWeight: 700, color: 'var(--cc-text)', marginBottom: 4, letterSpacing: '-0.02em' }}>
        Indexing workspace
      </h2>
      <p style={{ fontSize: 13, color: 'var(--cc-text-muted)', marginBottom: 28 }}>
        Connecting rust-analyzer to axum-web-api…
      </p>

      {/* step list */}
      <div className="w-full rounded-xl overflow-hidden" style={{ background: 'var(--cc-panel)', border: '1px solid var(--cc-border)' }}>
        {steps.map((s, i) => (
          <div key={i} className="flex items-center gap-3 px-4 py-2.5" style={{ borderBottom: i < steps.length - 1 ? '1px solid var(--cc-border)' : 'none' }}>
            <div className="flex items-center justify-center shrink-0" style={{ width: 18, height: 18 }}>
              {s.done ? (
                <div className="w-4 h-4 rounded-full flex items-center justify-center" style={{ background: '#34D399' }}>
                  <span style={{ color: 'var(--cc-bg)', fontSize: 9, fontWeight: 700 }}>✓</span>
                </div>
              ) : s.pending ? (
                <div className="w-4 h-4 rounded-full" style={{ background: 'var(--cc-border)', border: '1px solid var(--cc-border-strong)' }} />
              ) : (
                <div className="w-4 h-4 rounded-full flex items-center justify-center animate-spin" style={{ border: '2px solid #06B6D420', borderTop: '2px solid #06B6D4' }} />
              )}
            </div>
            <div className="flex-1">
              <span style={{ fontSize: 12, color: s.done ? 'var(--cc-text-muted)' : s.pending ? 'var(--cc-text-faint)' : 'var(--cc-text)' }}>
                {s.label}
              </span>
              {s.progress !== undefined && (
                <div className="flex items-center gap-2 mt-1">
                  <div className="flex-1 h-1 rounded-full overflow-hidden" style={{ background: 'var(--cc-border)' }}>
                    <div className="h-full rounded-full" style={{ width: `${s.progress}%`, background: 'linear-gradient(90deg, #06B6D4, #7C3AED)' }} />
                  </div>
                  <span style={{ fontSize: 10, color: 'var(--cc-text-subtle)' }}>{s.progress}%</span>
                </div>
              )}
            </div>
          </div>
        ))}
      </div>

      <p style={{ fontSize: 11, color: 'var(--cc-text-faint)', marginTop: 16 }}>247 files · 4 scopes · semantic analyzers</p>
    </div>
  )
}

// ── Settings modal ──────────────────────────────────────────────────────────
function SettingsModal({ onClose }: { onClose: () => void }) {
  const sections = [
    { label: 'Analysis', items: ['Auto-reanalyze on save', 'Include test modules', 'Resolve proc macros', 'Track data flow'] },
    { label: 'Graph Rendering', items: ['Enable glow effects', 'Animate data flow edges', 'Show edge labels', 'Enable minimap'] },
    { label: 'Performance', items: ['Max visible nodes: 150', 'Physics simulation FPS: 60', 'Edge bundling', 'LOD on pan/zoom'] },
  ]

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center" style={{ background: 'var(--cc-backdrop)', backdropFilter: 'blur(4px)' }} onClick={e => { if (e.target === e.currentTarget) onClose() }}>
      <div className="rounded-2xl overflow-hidden shadow-2xl" style={{ width: 540, background: 'var(--cc-panel)', border: '1px solid var(--cc-border)', fontFamily: 'Inter, sans-serif' }}>
        {/* header */}
        <div className="flex items-center gap-2 px-5 py-4" style={{ borderBottom: '1px solid var(--cc-border)' }}>
          <span style={{ fontSize: 14, fontWeight: 600, color: 'var(--cc-text)', flex: 1 }}>Settings</span>
          <button onClick={onClose} style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--cc-text-subtle)', fontSize: 18, display: 'flex', lineHeight: 1 }}>×</button>
        </div>

        <div className="p-5 space-y-5">
          {sections.map(sec => (
            <div key={sec.label}>
              <p style={{ fontSize: 11, color: 'var(--cc-text-faint)', fontWeight: 600, letterSpacing: '0.07em', textTransform: 'uppercase', marginBottom: 10 }}>{sec.label}</p>
              <div className="space-y-2">
                {sec.items.map(item => (
                  <div key={item} className="flex items-center justify-between rounded-lg px-3 py-2.5" style={{ background: 'var(--cc-card)', border: '1px solid var(--cc-border)' }}>
                    <span style={{ fontSize: 12, color: 'var(--cc-text-muted)' }}>{item}</span>
                    <div
                      className="rounded-full flex items-center"
                      style={{ width: 36, height: 20, background: '#06B6D4', padding: 2, cursor: 'pointer' }}
                    >
                      <div className="rounded-full ml-auto" style={{ width: 16, height: 16, background: '#fff' }} />
                    </div>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>

        <div className="flex justify-end gap-2 px-5 py-4" style={{ borderTop: '1px solid var(--cc-border)' }}>
          <button onClick={onClose} style={{ padding: '8px 16px', borderRadius: 8, background: 'var(--cc-elevated)', border: '1px solid var(--cc-border)', color: 'var(--cc-text-muted)', fontSize: 12, cursor: 'pointer' }}>Cancel</button>
          <button onClick={onClose} style={{ padding: '8px 16px', borderRadius: 8, background: 'linear-gradient(135deg, #06B6D4, #7C3AED)', border: 'none', color: '#fff', fontSize: 12, fontWeight: 600, cursor: 'pointer' }}>Save</button>
        </div>
      </div>
    </div>
  )
}
