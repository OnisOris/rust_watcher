import { useState, useCallback, useEffect, useMemo } from 'react'
import { TopToolbar } from './components/TopToolbar'
import { ProjectExplorer } from './components/ProjectExplorer'
import { LiveCodeGraph } from './components/LiveCodeGraph'
import { InspectorPanel } from './components/InspectorPanel'
import { AnalysisTimeline } from './components/AnalysisTimeline'
import { FilterBar } from './components/FilterBar'
import { SearchCommandPalette } from './components/SearchCommandPalette'
import { EmptyState } from './components/EmptyState'
import { DenseGraphSuggestion } from './components/DenseGraphSuggestion'
import { FocusBubbleControls } from './components/FocusBubbleControls'
import { useBackendGraph } from './api/useBackendGraph'
import type { GraphMode, GraphFilters, NodeType, EdgeType, ThemeMode, GraphNode, GraphEdge } from './types'

const ALL_NODE_TYPES = new Set<NodeType>(['File', 'Module', 'Struct', 'Enum', 'Trait', 'Impl', 'Function', 'Method', 'Component', 'Hook', 'Interface', 'TypeAlias', 'Endpoint', 'Macro', 'ExternalCrate'])
const ALL_EDGE_TYPES = new Set<EdgeType>(['Contains', 'Uses', 'Calls', 'Renders', 'ApiCall', 'Implements', 'TypeReference', 'DataFlow', 'ModDeclaration', 'ExternalDependency'])

const DEFAULT_FILTERS: GraphFilters = {
  nodeTypes: ALL_NODE_TYPES,
  edgeTypes: ALL_EDGE_TYPES,
  showTests: true,
  showExternal: true,
  onlyPublicAPI: false,
  depth: 'full',
  onlyCurrentFile: false,
}

type GraphLens = 'all' | 'architecture' | 'api'

function initialTheme(): ThemeMode {
  const stored = localStorage.getItem('rust-watcher-theme')
  return stored === 'dark' ? 'dark' : 'light'
}

export default function App() {
  const [mode, setMode] = useState<GraphMode>('Macro')
  const [theme, setTheme] = useState<ThemeMode>(initialTheme)
  const [focusBubbleNodeId, setFocusBubbleNodeId] = useState<string | null>(null)
  const [filters, setFilters] = useState<GraphFilters>(DEFAULT_FILTERS)
  const [timelineCollapsed, setTimelineCollapsed] = useState(false)
  const [searchOpen, setSearchOpen] = useState(false)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [clarityOpen, setClarityOpen] = useState(false)
  const [focusModeActive, setFocusModeActive] = useState(false)
  const [graphLens, setGraphLens] = useState<GraphLens>('all')
  const [recenterKey, setRecenterKey] = useState(0)
  const [pinnedNodeIds, setPinnedNodeIds] = useState<Set<string>>(new Set())
  const {
    appState,
    analyzerStatus,
    projectName,
    lastUpdated,
    message,
    nodes,
    edges,
    files,
    events,
    selectedNodeId,
    setSelectedNodeId,
    openProject,
    openInEditor,
    requestFocusBubble,
    search,
    refreshSnapshot,
  } = useBackendGraph(mode)

  const graphNodes = useMemo(
    () => nodes.map(node => ({
      ...node,
      pinned: pinnedNodeIds.has(node.id) || node.pinned,
    })),
    [nodes, pinnedNodeIds],
  )
  const selectedNode = selectedNodeId ? graphNodes.find(n => n.id === selectedNodeId) ?? null : null
  const { nodes: visibleGraphNodes, edges: visibleGraphEdges } = useMemo(
    () => applyGraphLens(graphNodes, edges, graphLens),
    [graphNodes, edges, graphLens],
  )
  const togglePinNode = useCallback((id: string) => {
    setPinnedNodeIds(prev => {
      const next = new Set(prev)
      next.has(id) ? next.delete(id) : next.add(id)
      return next
    })
  }, [])

  // keyboard shortcut ⌘K
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault()
        setSearchOpen(prev => !prev)
      }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [])

  useEffect(() => {
    refreshSnapshot(mode)
  }, [mode, refreshSnapshot])

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

  const handleDoubleClickNode = useCallback((id: string) => {
    setFocusBubbleNodeId(id)
    setFocusModeActive(true)
  }, [])

  const handleFocusBubble = useCallback((id: string) => {
    setFocusBubbleNodeId(id)
    setFocusModeActive(true)
    requestFocusBubble(id).catch(() => {})
  }, [requestFocusBubble])

  const handleCloseFocusBubble = useCallback(() => {
    setFocusBubbleNodeId(null)
    setFocusModeActive(false)
  }, [])

  const handleFocusModeToggle = useCallback(() => {
    if (focusModeActive) {
      handleCloseFocusBubble()
    } else if (selectedNodeId) {
      handleFocusBubble(selectedNodeId)
    }
  }, [focusModeActive, selectedNodeId, handleCloseFocusBubble, handleFocusBubble])

  const focusBubbleNode = focusBubbleNodeId ? graphNodes.find(n => n.id === focusBubbleNodeId) ?? null : null

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
        message={message}
        mode={mode}
        onModeChange={setMode}
        onSearchOpen={() => setSearchOpen(true)}
        onSettingsOpen={() => setSettingsOpen(true)}
        onRecenter={() => setRecenterKey(key => key + 1)}
        onCollapse={() => setGraphLens(current => current === 'architecture' ? 'all' : 'architecture')}
        onFocusMode={handleFocusModeToggle}
        onThemeToggle={() => setTheme(current => current === 'light' ? 'dark' : 'light')}
        onClarityToggle={() => setClarityOpen(open => !open)}
        focusModeActive={focusModeActive}
        clarityOpen={clarityOpen}
        clarityActive={graphLens !== 'all' || filters.depth !== 'full' || !filters.showExternal || !filters.showTests}
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
          onSelectNode={handleSelectNode}
          onFocusFile={() => {}}
        />

        {/* graph area */}
        <div className="relative flex-1 min-w-0 overflow-hidden">
          <LiveCodeGraph
            nodes={visibleGraphNodes}
            edges={visibleGraphEdges}
            mode={mode}
            filters={filters}
            selectedNodeId={selectedNodeId}
            focusBubbleNodeId={focusBubbleNodeId}
            recenterKey={recenterKey}
            theme={theme}
            onSelectNode={handleSelectNode}
            onDoubleClickNode={handleDoubleClickNode}
            onUpdateNodes={() => {}}
          />

          {/* floating filter bar */}
          <FilterBar filters={filters} onFiltersChange={setFilters} />

          {/* focus bubble controls */}
          {focusBubbleNodeId && focusBubbleNode && (
            <FocusBubbleControls
              nodeLabel={focusBubbleNode.label}
              onClose={handleCloseFocusBubble}
              onExpandDepth={() => setFilters(f => ({ ...f, depth: f.depth === 'full' ? 3 : (typeof f.depth === 'number' ? Math.min(f.depth + 1, 3) as 1 | 2 | 3 : 'full') }))}
              onCollapseNoise={() => setFilters(f => ({ ...f, depth: 1 }))}
              onShowCallers={() => {}}
              onShowCallees={() => {}}
              onShowDataFlow={() => {}}
            />
          )}

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
              depth={filters.depth}
              externalHidden={!filters.showExternal || !filters.nodeTypes.has('ExternalCrate')}
              testsHidden={!filters.showTests}
              canFocus={!!selectedNodeId}
              onDismiss={() => setClarityOpen(false)}
              onLensChange={setGraphLens}
              onHideExternal={() => {
                setFilters(f => {
                  const next = new Set(f.nodeTypes)
                  if (next.has('ExternalCrate')) {
                    next.delete('ExternalCrate')
                    return { ...f, nodeTypes: next, showExternal: false }
                  }
                  next.add('ExternalCrate')
                  return { ...f, nodeTypes: next, showExternal: true }
                })
              }}
              onHideTests={() => setFilters(f => ({ ...f, showTests: !f.showTests }))}
              onDepth2={() => setFilters(f => ({ ...f, depth: f.depth === 2 ? 'full' : 2 }))}
              onFocusBubble={() => {
                if (selectedNodeId) handleFocusBubble(selectedNodeId)
              }}
            />
          </div>

          {/* graph metadata overlay */}
          <div
            className="absolute bottom-3 left-3 flex items-center gap-3"
            style={{ pointerEvents: 'none' }}
          >
            <div className="flex items-center gap-2 rounded-lg px-3 py-1.5" style={{ background: 'var(--cc-overlay)', border: '1px solid var(--cc-border)', backdropFilter: 'blur(8px)' }}>
              <span style={{ fontSize: 10, color: 'var(--cc-text-subtle)' }}>{graphNodes.length} nodes</span>
              <span style={{ color: 'var(--cc-border)' }}>·</span>
              <span style={{ fontSize: 10, color: 'var(--cc-text-subtle)' }}>{edges.length} edges</span>
              {graphLens !== 'all' && (
                <>
                  <span style={{ color: 'var(--cc-border)' }}>·</span>
                  <span style={{ fontSize: 10, color: '#06B6D4' }}>
                    {graphLens === 'architecture' ? 'Architecture' : 'API Bridge'}: {visibleGraphNodes.length}/{visibleGraphEdges.length}
                  </span>
                </>
              )}
              <span style={{ color: 'var(--cc-border)' }}>·</span>
              <span style={{ fontSize: 10, color: 'var(--cc-text-subtle)' }}>{mode}</span>
            </div>
            <div className="flex items-center gap-1.5 rounded-lg px-3 py-1.5" style={{ background: 'var(--cc-overlay)', border: '1px solid var(--cc-border)', backdropFilter: 'blur(8px)' }}>
              <div style={{ width: 6, height: 6, borderRadius: '50%', background: analyzerStatus === 'Error' ? '#F87171' : analyzerStatus === 'Indexing' ? '#F59E0B' : '#34D399' }} />
              <span style={{ fontSize: 10, color: 'var(--cc-text-subtle)' }}>{message ?? 'Live'} · {lastUpdated ? `Updated ${lastUpdated}` : 'Waiting for backend'}</span>
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
          appState={appState}
          filesCount={files.length}
          message={message}
          onTogglePin={togglePinNode}
          onFocusBubble={handleFocusBubble}
          onSelectNode={handleSelectNode}
          onOpenInEditor={openInEditor}
        />
      </div>

      {/* analysis timeline */}
      <AnalysisTimeline
        events={events}
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
    keepEdgeTypes.add('ExternalDependency')
    keepEdgeTypes.add('Uses')
    keepEdgeTypes.add('ApiCall')
  } else {
    keepEdgeTypes.add('Contains')
    keepEdgeTypes.add('Calls')
    keepEdgeTypes.add('ApiCall')
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
        if (edge.type !== 'Contains' && edge.type !== 'Calls') continue
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
    { label: 'Discovering crates…', done: true },
    { label: 'Loading rust-analyzer…', done: true },
    { label: 'Indexing source files…', done: false, progress: 45 },
    { label: 'Building semantic graph…', done: false, pending: true },
    { label: 'Resolving trait bounds…', done: false, pending: true },
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

      <p style={{ fontSize: 11, color: 'var(--cc-text-faint)', marginTop: 16 }}>247 files · 4 crates · rust-analyzer 2024.12</p>
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
