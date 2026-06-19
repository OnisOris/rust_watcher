import { useCallback, useEffect, useMemo, useState } from 'react'
import { INITIAL_EDGES, INITIAL_NODES, PROJECT_FILES, ANALYSIS_EVENTS } from '../mockData'
import {
  applyDiagnosticsCountsToFiles,
  applyDiagnosticsPatch,
  applyGraphPatchToEdges,
  applyGraphPatchToNodes,
} from './patchHelpers'
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

const MOCK_STATUS: AppStatus = {
  appState: 'normal',
  analyzerStatus: 'Ready',
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

export function useBackendGraph(mode: GraphMode) {
  const [appState, setAppState] = useState<AppState>('empty')
  const [analyzerStatus, setAnalyzerStatus] = useState<AnalyzerStatus>('Starting')
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

  const applyStatus = useCallback((status: AppStatus) => {
    setAppState(status.appState)
    setAnalyzerStatus(status.analyzerStatus)
    setProjectName(status.projectName)
    setProjectPath(status.projectPath)
    setLastUpdated(status.lastUpdated)
    setMessage(status.message)
  }, [])

  const applySnapshot = useCallback((snapshot: GraphSnapshot) => {
    setNodes(snapshot.nodes)
    setEdges(snapshot.edges)
    setFiles(snapshot.files)
    setEvents(snapshot.events)
    applyStatus(snapshot.status)
  }, [applyStatus])

  const applyPatch = useCallback((patch: GraphPatch) => {
    setNodes(prev => applyGraphPatchToNodes(prev, patch))
    setEdges(prev => applyGraphPatchToEdges(prev, patch))
    if (patch.diagnostics?.length || patch.changedFiles?.length) {
      setDiagnosticsByFile(prev => {
        const next = applyDiagnosticsPatch(prev, patch)
        setDiagnosticsByNode(next.diagnosticsByNode)
        setFiles(files => applyDiagnosticsCountsToFiles(files, next.diagnosticsByFile, patch.changedFiles ?? []))
        return next.diagnosticsByFile
      })
    }
  }, [])

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
    setNodes(INITIAL_NODES)
    setEdges(INITIAL_EDGES)
    setFiles(PROJECT_FILES)
    setEvents(ANALYSIS_EVENTS)
    applyStatus(MOCK_STATUS)
  }, [applyStatus])

  const refreshSnapshot = useCallback(async (nextMode: GraphMode = mode) => {
    try {
      const response = await fetch(`/api/graph/snapshot?mode=${encodeURIComponent(nextMode)}`)
      if (!response.ok) throw new Error(`snapshot failed: ${response.status}`)
      applySnapshot(await response.json())
      setBackendAvailable(true)
    } catch {
      applyDevFallback()
    }
  }, [applyDevFallback, applySnapshot, mode])

  useEffect(() => {
    let cancelled = false

    async function boot() {
      try {
        const [statusResponse, snapshotResponse] = await Promise.all([
          fetch('/api/status'),
          fetch(`/api/graph/snapshot?mode=${encodeURIComponent(mode)}`),
        ])
        if (!statusResponse.ok || !snapshotResponse.ok) {
          throw new Error('backend unavailable')
        }
        if (cancelled) return
        applyStatus(await statusResponse.json())
        applySnapshot(await snapshotResponse.json())
        setBackendAvailable(true)
      } catch {
        if (!cancelled) applyDevFallback()
      }
    }

    boot()
    return () => { cancelled = true }
  }, [applyDevFallback, applySnapshot, applyStatus, mode])

  useEffect(() => {
    if (!backendAvailable) return
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    const socket = new WebSocket(`${protocol}//${window.location.host}/ws`)

    socket.onmessage = event => {
      const message = JSON.parse(event.data) as ServerMessage
      if (message.type === 'graph_snapshot') {
        applySnapshot(message.payload)
      } else if (message.type === 'graph_patch') {
        applyPatch(message.payload)
      } else if (message.type === 'analyzer_status') {
        applyStatus(message.payload)
      } else if (message.type === 'analysis_event') {
        setEvents(prev => [message.payload, ...prev].slice(0, 200))
      } else if (message.type === 'error') {
        setMessage(message.payload.message)
      }
    }
    socket.onerror = () => {
      if (DEV_FALLBACK) applyDevFallback()
    }
    return () => socket.close()
  }, [applyDevFallback, applyPatch, applySnapshot, applyStatus, backendAvailable])

  const openProject = useCallback(async (path?: string) => {
    try {
      setAppState('indexing')
      const response = await fetch('/api/project/open', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: path || undefined }),
      })
      if (!response.ok) throw new Error(await response.text())
      await refreshSnapshot(mode)
      setBackendAvailable(true)
    } catch (error) {
      setMessage(error instanceof Error ? error.message : 'Failed to open project.')
      applyDevFallback()
    }
  }, [applyDevFallback, mode, refreshSnapshot])

  const requestFocusBubble = useCallback(async (nodeId: string, depth: 1 | 2 | 3 | 'full' = 'full') => {
    const response = await fetch('/api/focus', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ nodeId, depth, mode }),
    })
    if (!response.ok) throw new Error(await response.text())
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
      if (!response.ok) throw new Error(await response.text())
      setBackendAvailable(true)
    } catch (error) {
      setMessage(error instanceof Error ? error.message : 'Failed to open editor.')
    }
  }, [])

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
