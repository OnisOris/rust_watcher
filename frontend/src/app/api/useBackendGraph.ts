import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { INITIAL_EDGES, INITIAL_NODES, PROJECT_FILES, ANALYSIS_EVENTS } from '../mockData'
import {
  applyDiagnosticCountsToFiles,
  applyDiagnosticsPatch,
  applyGraphPatchToEdges,
  applyGraphPatchToNodes,
  diagnosticsByNodeFromFileMap,
} from './graphPatch'
import { shouldRefreshSnapshotForPatch } from './backendMessages'
import { filterApplicableAnalyzers } from './analyzerStatus'
import type {
  AnalysisEvent,
  AnalyzerStatus,
  AppState,
  AppStatus,
  FocusResponse,
  GraphEdge,
  GraphMode,
  GraphNode,
  GraphPatch,
  GraphSnapshot,
  DiagnosticRecord,
  ProjectFile,
  SearchResult,
} from '../types'

const DEV_FALLBACK = import.meta.env.DEV
const ABSENT_RUST_ANALYZER_MESSAGE = /No Cargo\.toml found|rust-analyzer disabled|Rust syntax fallback active|rust-analyzer is unavailable/i
const LANGUAGE_ROOT_IDS = new Set(['backend:python', 'frontend:typescript', 'ui:qml'])

const MOCK_STATUS: AppStatus = {
  appState: 'normal',
  analyzerStatus: 'Ready',
  analyzers: [
    {
      id: 'rust-analyzer',
      kind: 'Rust',
      engine: 'RustAnalyzer',
      label: 'rust-analyzer',
      status: 'Ready',
      capabilities: ['Symbols', 'Diagnostics', 'References', 'CallHierarchy', 'SemanticCalls'],
      filesIndexed: 247,
      lastUpdated: null,
      provider: 'local',
      billable: false,
    },
  ],
  pythonAnalyzer: { mode: 'parser', status: 'parser only' },
  projectName: 'mock workspace',
  projectPath: null,
  lastUpdated: null,
  message: 'Using mock data because the backend is unavailable.',
  progress: null,
}

type ServerMessage =
  | { type: 'graph_snapshot'; payload: GraphSnapshot }
  | { type: 'graph_patch'; payload: GraphPatch }
  | { type: 'analyzer_status'; payload: AppStatus }
  | { type: 'analysis_event'; payload: AnalysisEvent }
  | { type: 'error'; payload: { message: string } }

interface DiagnosticsResponse {
  diagnosticsByFile: Record<string, DiagnosticRecord[]>
  diagnosticsByNode: Record<string, DiagnosticRecord[]>
  allDiagnostics: DiagnosticRecord[]
}

function diagnosticsMapFromRecord(record: Record<string, DiagnosticRecord[]>): Map<string, DiagnosticRecord[]> {
  return new Map(Object.entries(record))
}

async function responseError(response: Response, action: string) {
  const text = await response.text().catch(() => '')
  const body = text.trim().slice(0, 240)
  return new Error(body || `${action} failed with HTTP ${response.status}.`)
}

function actionableNetworkError(action: string, error: unknown) {
  if (error instanceof TypeError && /fetch|network|failed/i.test(error.message)) {
    return `${action}: backend is not reachable. Make sure web-server is still running and refresh the page.`
  }
  if (error instanceof Error && /NetworkError|Failed to fetch|Load failed/i.test(error.message)) {
    return `${action}: backend is not reachable. Make sure web-server is still running and refresh the page.`
  }
  return error instanceof Error ? error.message : `${action} failed.`
}

function normalizeStatus(status: AppStatus): AppStatus {
  const analyzers = filterApplicableAnalyzers(status.analyzers ?? [])
  const hasRustAnalyzer = analyzers.some(analyzer => analyzer.kind === 'Rust')
  const rustFallbackWithoutRustFiles = !hasRustAnalyzer && status.message && ABSENT_RUST_ANALYZER_MESSAGE.test(status.message)
  const analyzerStatus: AnalyzerStatus = rustFallbackWithoutRustFiles && status.analyzerStatus === 'Fallback'
    ? 'Ready'
    : status.analyzerStatus
  const message = rustFallbackWithoutRustFiles
    ? (analyzers.length > 0 ? 'Language graph ready' : 'Detecting project languages')
    : status.message

  return {
    ...status,
    analyzerStatus,
    analyzers,
    message,
  }
}

function isWorkspaceNode(node: GraphNode) {
  return node.type === 'Module' && !node.file && node.id.startsWith('workspace:')
}

function isLanguageRootNode(node: GraphNode) {
  return node.type === 'Module' && LANGUAGE_ROOT_IDS.has(node.id)
}

function edgeExists(edges: GraphEdge[], source: string, target: string) {
  return edges.some(edge => edge.source === source && edge.target === target)
}

function shouldHideWorkspaceWrapper(workspace: GraphNode, languageRoots: GraphNode[], edges: GraphEdge[]) {
  if (languageRoots.length !== 1) return false
  const languageRootIds = new Set(languageRoots.map(root => root.id))
  const hasNonLanguageChild = edges.some(edge => edge.source === workspace.id && !languageRootIds.has(edge.target))
  const hasIncomingEdge = edges.some(edge => edge.target === workspace.id)
  return !hasNonLanguageChild && !hasIncomingEdge
}

function normalizeSnapshot(snapshot: GraphSnapshot): GraphSnapshot {
  const status = normalizeStatus(snapshot.status)
  const workspace = snapshot.nodes.find(isWorkspaceNode)
  const languageRoots = snapshot.nodes.filter(isLanguageRootNode)
  if (!workspace || languageRoots.length === 0) {
    return { ...snapshot, status }
  }

  if (shouldHideWorkspaceWrapper(workspace, languageRoots, snapshot.edges)) {
    const nodes = snapshot.nodes.filter(node => node.id !== workspace.id)
    const edges = snapshot.edges.filter(edge => edge.source !== workspace.id && edge.target !== workspace.id)
    return { ...snapshot, nodes, edges, status }
  }

  let changed = false
  const edges = [...snapshot.edges]
  for (const root of languageRoots) {
    if (root.id === workspace.id || edgeExists(edges, workspace.id, root.id)) continue
    edges.push({
      id: `Contains:${workspace.id}->${root.id}`,
      source: workspace.id,
      target: root.id,
      type: 'Contains',
      confidence: 'Exact',
    })
    changed = true
  }

  return changed ? { ...snapshot, edges, status } : { ...snapshot, status }
}

export function useBackendGraph(mode: GraphMode, options: { enabled?: boolean } = {}) {
  const enabled = options.enabled ?? true
  const [appState, setAppState] = useState<AppState>('empty')
  const [analyzerStatus, setAnalyzerStatus] = useState<AnalyzerStatus>('Starting')
  const [analyzers, setAnalyzers] = useState<AppStatus['analyzers']>([])
  const [pythonAnalyzer, setPythonAnalyzer] = useState<AppStatus['pythonAnalyzer']>(null)
  const [projectName, setProjectName] = useState<string | null>(null)
  const [projectPath, setProjectPath] = useState<string | null>(null)
  const [lastUpdated, setLastUpdated] = useState<string | null>(null)
  const [message, setMessage] = useState<string | null>(null)
  const [nodes, setNodes] = useState<GraphNode[]>([])
  const [edges, setEdges] = useState<GraphEdge[]>([])
  const [files, setFiles] = useState<ProjectFile[]>([])
  const [events, setEvents] = useState<AnalysisEvent[]>([])
  const [diagnosticsByFile, setDiagnosticsByFile] = useState<Map<string, DiagnosticRecord[]>>(new Map())
  const [diagnosticsByNode, setDiagnosticsByNode] = useState<Map<string, DiagnosticRecord[]>>(new Map())
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null)
  const [backendAvailable, setBackendAvailable] = useState(true)
  const snapshotRequestSeq = useRef(0)
  const localNodeCountRef = useRef(0)
  const applyPatchRef = useRef<(patch: GraphPatch) => boolean>(() => false)
  const applySnapshotRef = useRef<(snapshot: GraphSnapshot) => void>(() => {})
  const applyStatusRef = useRef<(status: AppStatus) => void>(() => {})
  const refreshSnapshotRef = useRef<() => Promise<void>>(async () => {})
  const applyDevFallbackRef = useRef<() => void>(() => {})

  useEffect(() => {
    localNodeCountRef.current = nodes.length
  }, [nodes.length])

  const applyStatus = useCallback((status: AppStatus) => {
    const normalized = normalizeStatus(status)
    setAppState(normalized.appState)
    setAnalyzerStatus(normalized.analyzerStatus)
    setAnalyzers(normalized.analyzers ?? [])
    setPythonAnalyzer(normalized.pythonAnalyzer ?? null)
    setProjectName(normalized.projectName)
    setProjectPath(normalized.projectPath)
    setLastUpdated(normalized.lastUpdated)
    setMessage(normalized.message)
  }, [])

  const applySnapshot = useCallback((snapshot: GraphSnapshot) => {
    const normalized = normalizeSnapshot(snapshot)
    localNodeCountRef.current = normalized.nodes.length
    setNodes(normalized.nodes)
    setEdges(normalized.edges)
    setFiles(normalized.files)
    setEvents(normalized.events)
    applyStatus(normalized.status)
  }, [applyStatus])

  useEffect(() => {
    applySnapshotRef.current = applySnapshot
  }, [applySnapshot])

  const applyDiagnosticsSnapshot = useCallback((diagnosticsByFileRecord: Record<string, DiagnosticRecord[]>) => {
    const nextByFile = diagnosticsMapFromRecord(diagnosticsByFileRecord)
    setDiagnosticsByFile(nextByFile)
    setDiagnosticsByNode(diagnosticsByNodeFromFileMap(nextByFile))
    setFiles(files => applyDiagnosticCountsToFiles(files, nextByFile, files.map(file => file.path)))
  }, [])

  const refreshDiagnostics = useCallback(async () => {
    const response = await fetch('/api/diagnostics')
    if (!response.ok) throw await responseError(response, 'Loading diagnostics')
    const diagnostics = await response.json() as DiagnosticsResponse
    applyDiagnosticsSnapshot(diagnostics.diagnosticsByFile)
  }, [applyDiagnosticsSnapshot])

  const applyPatch = useCallback((patch: GraphPatch) => {
    const touchesGraph = patch.fullRebuild
      || patch.addedNodes.length > 0
      || patch.updatedNodes.length > 0
      || patch.removedNodeIds.length > 0
      || patch.addedEdges.length > 0
      || patch.updatedEdges.length > 0
      || patch.removedEdgeIds.length > 0
    const needsSnapshot = touchesGraph || shouldRefreshSnapshotForPatch(patch, localNodeCountRef.current)
    if (!needsSnapshot) {
      setNodes(prev => {
        const next = applyGraphPatchToNodes(prev, patch)
        localNodeCountRef.current = next.length
        return next
      })
      setEdges(prev => applyGraphPatchToEdges(prev, patch))
    }
    if (patch.diagnostics?.length || patch.changedFiles?.length) {
      setDiagnosticsByFile(prev => {
        const next = applyDiagnosticsPatch(prev, patch)
        setDiagnosticsByNode(next.diagnosticsByNode)
        setFiles(files => applyDiagnosticCountsToFiles(files, next.diagnosticsByFile, patch.changedFiles ?? []))
        return next.diagnosticsByFile
      })
    }
    return needsSnapshot
  }, [])

  useEffect(() => {
    applyPatchRef.current = applyPatch
  }, [applyPatch])

  const applyDevFallback = useCallback(() => {
    setBackendAvailable(false)
    if (!DEV_FALLBACK) {
      applyStatus({
        ...MOCK_STATUS,
        appState: 'error',
        analyzerStatus: 'Error',
        message: 'Backend is unavailable.',
      })
      return
    }
    localNodeCountRef.current = INITIAL_NODES.length
    setNodes(INITIAL_NODES)
    setEdges(INITIAL_EDGES)
    setFiles(PROJECT_FILES)
    setEvents(ANALYSIS_EVENTS)
    setDiagnosticsByFile(new Map())
    setDiagnosticsByNode(new Map())
    applyStatus(MOCK_STATUS)
  }, [applyStatus])

  useEffect(() => {
    applyStatusRef.current = applyStatus
    applyDevFallbackRef.current = applyDevFallback
  }, [applyDevFallback, applyStatus])

  const refreshSnapshot = useCallback(async (nextMode: GraphMode = mode) => {
    if (!enabled) return
    const requestSeq = ++snapshotRequestSeq.current
    try {
      const response = await fetch(`/api/graph/snapshot?mode=${encodeURIComponent(nextMode)}`)
      if (!response.ok) throw await responseError(response, 'Loading graph snapshot')
      const snapshot = await response.json()
      if (requestSeq !== snapshotRequestSeq.current) return
      applySnapshot(snapshot)
      await refreshDiagnostics().catch(() => undefined)
      setBackendAvailable(true)
    } catch (error) {
      setMessage(actionableNetworkError('Loading graph snapshot', error))
      applyDevFallback()
    }
  }, [applyDevFallback, applySnapshot, refreshDiagnostics, mode, enabled])

  useEffect(() => {
    refreshSnapshotRef.current = async () => {
      await refreshSnapshot(mode)
    }
  }, [mode, refreshSnapshot])

  useEffect(() => {
    if (!enabled) return
    let cancelled = false

    async function boot() {
      try {
        const statusResponse = await fetch('/api/status')
        if (!statusResponse.ok) {
          throw new Error('backend unavailable')
        }
        if (cancelled) return
        applyStatus(await statusResponse.json())
        await refreshSnapshot()
      } catch (error) {
        if (!cancelled) {
          setMessage(actionableNetworkError('Connecting to backend', error))
          applyDevFallback()
        }
      }
    }

    boot()
    return () => { cancelled = true }
  }, [applyDevFallback, applyStatus, refreshSnapshot, enabled])

  useEffect(() => {
    if (!enabled) return
    if (backendAvailable) void refreshSnapshot(mode)
  }, [backendAvailable, mode, refreshSnapshot, enabled])

  useEffect(() => {
    if (!enabled) return
    if (!backendAvailable) return
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    const socket = new WebSocket(`${protocol}//${window.location.host}/ws`)

    socket.onmessage = event => {
      const message = JSON.parse(event.data) as ServerMessage
      if (message.type === 'graph_snapshot') {
        applySnapshotRef.current(message.payload)
      } else if (message.type === 'graph_patch') {
        if (message.payload.fullRebuild) {
          void refreshSnapshotRef.current()
        } else {
          const needsSnapshot = applyPatchRef.current(message.payload)
          if (needsSnapshot) void refreshSnapshotRef.current()
        }
      } else if (message.type === 'analyzer_status') {
        applyStatusRef.current(message.payload)
      } else if (message.type === 'analysis_event') {
        setEvents(prev => [message.payload, ...prev].slice(0, 200))
      } else if (message.type === 'error') {
        setMessage(message.payload.message)
      }
    }
    socket.onerror = () => {
      if (DEV_FALLBACK) applyDevFallbackRef.current()
    }
    return () => socket.close()
  }, [backendAvailable, enabled])

  const openProject = useCallback(async (path?: string) => {
    if (!enabled) return
    try {
      setAppState('indexing')
      setDiagnosticsByFile(new Map())
      setDiagnosticsByNode(new Map())
      const response = await fetch('/api/project/open', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: path || undefined }),
      })
      if (!response.ok) throw await responseError(response, 'Opening project')
      await refreshSnapshot(mode)
      setBackendAvailable(true)
    } catch (error) {
      setMessage(actionableNetworkError('Opening project', error))
      applyDevFallback()
    }
  }, [applyDevFallback, mode, refreshSnapshot, enabled])

  const requestFocusBubble = useCallback(async (nodeId: string, depth: 1 | 2 | 3 | 'full' = 'full') => {
    const response = await fetch('/api/focus', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ nodeId, depth, mode }),
    })
    if (!response.ok) throw await responseError(response, 'Focusing graph')
    return await response.json() as FocusResponse
  }, [mode])

  const openInEditor = useCallback(async (node: GraphNode) => {
    if (!node.file) {
      setMessage('Selected node has no source file.')
      return
    }
    try {
      const response = await fetch('/api/editor/open', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ file: node.file, line: node.line ?? undefined, column: 1 }),
      })
      if (!response.ok) throw await responseError(response, 'Opening editor')
      setBackendAvailable(true)
    } catch (error) {
      setMessage(actionableNetworkError('Opening editor', error))
    }
  }, [])

  const saveNodeLayout = useCallback(async (node: GraphNode) => {
    if (!enabled) return
    if (!backendAvailable) return
    try {
      const response = await fetch('/api/layout/node', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          node: {
            nodeId: node.id,
            x: node.x,
            y: node.y,
            vx: node.vx ?? 0,
            pinned: node.pinned ?? false,
          },
        }),
      })
      if (!response.ok) throw await responseError(response, 'Saving layout')
      setBackendAvailable(true)
    } catch (error) {
      setMessage(actionableNetworkError('Saving layout', error))
    }
  }, [backendAvailable, enabled])

  const search = useCallback(async (query: string): Promise<SearchResult[]> => {
    if (!backendAvailable) return localSearch(nodes, query)
    try {
      const response = await fetch(`/api/search?q=${encodeURIComponent(query)}`)
      if (!response.ok) throw new Error(`search failed: ${response.status}`)
      const data = await response.json()
      return data.results
    } catch {
      return localSearch(nodes, query)
    }
  }, [backendAvailable, nodes])

  const selectedNode = useMemo(
    () => selectedNodeId ? nodes.find(node => node.id === selectedNodeId) ?? null : null,
    [nodes, selectedNodeId],
  )

  return {
    appState,
    analyzerStatus,
    analyzers,
    pythonAnalyzer,
    projectName,
    projectPath,
    lastUpdated,
    message,
    nodes,
    edges,
    files,
    events,
    diagnosticsByFile,
    diagnosticsByNode,
    selectedNode,
    selectedNodeId,
    setSelectedNodeId,
    openProject,
    openInEditor,
    saveNodeLayout,
    requestFocusBubble,
    search,
    refreshSnapshot,
  }
}

function localSearch(nodes: GraphNode[], query: string): SearchResult[] {
  const q = query.toLowerCase()
  return nodes
    .filter(node =>
      !q ||
      node.label.toLowerCase().includes(q) ||
      node.file?.toLowerCase().includes(q) ||
      node.module?.toLowerCase().includes(q) ||
      node.crate?.toLowerCase().includes(q) ||
      node.type.toLowerCase().includes(q)
    )
    .slice(0, 30)
    .map(node => ({
      id: node.id,
      label: node.label,
      type: node.type,
      file: node.file ?? null,
      module: node.module ?? null,
      crate: node.crate ?? null,
      line: node.line ?? null,
    }))
}
