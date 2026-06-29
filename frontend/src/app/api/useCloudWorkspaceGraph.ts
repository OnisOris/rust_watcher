import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { filterApplicableAnalyzers } from './analyzerStatus'
import { cloudFetch } from './cloudAuth'
import type {
  AnalysisEvent,
  AnalyzerStatus,
  AppState,
  AppStatus,
  DiagnosticRecord,
  GraphEdge,
  GraphMode,
  GraphNode,
  GraphSnapshot,
  ProjectFile,
  SearchResult,
} from '../types'

interface CloudJob {
  jobId: string
  workspaceId?: string
  status: 'queued' | 'importing' | 'indexing' | 'analyzing' | 'completed' | 'failed' | 'cancelled'
  message?: string
  progress?: number
  creditsUsed?: number
}

type CloudEvent =
  | { type: 'jobStatus'; jobId?: string; workspaceId?: string; status?: CloudJob['status']; progress?: number; message?: string }
  | { type: 'snapshotReady'; jobId?: string; workspaceId?: string; status?: CloudJob['status']; progress?: number; message?: string }
  | { type: 'error'; jobId?: string; workspaceId?: string; message?: string }

function emptyStatus(): AppStatus {
  return {
    appState: 'empty',
    analyzerStatus: 'Starting',
    analyzers: [],
    pythonAnalyzer: null,
    projectName: null,
    projectPath: null,
    lastUpdated: null,
    message: 'Choose a project to analyze in cloud mode.',
    progress: null,
  }
}

function normalizeStatus(status: AppStatus): AppStatus {
  return {
    ...status,
    analyzers: filterApplicableAnalyzers(status.analyzers ?? []),
  }
}

function localSearch(nodes: GraphNode[], query: string): SearchResult[] {
  const text = query.trim().toLowerCase()
  if (!text) return []
  return nodes
    .filter(node => {
      return node.label.toLowerCase().includes(text)
        || node.file?.toLowerCase().includes(text)
        || node.description?.toLowerCase().includes(text)
    })
    .slice(0, 30)
    .map(node => ({ node, score: 1 }))
}

export function useCloudWorkspaceGraph(
  workspaceId: string | null,
  mode: GraphMode,
  options: { enabled?: boolean; sessionToken?: string | null } = {},
) {
  const enabled = options.enabled ?? true
  const sessionToken = options.sessionToken ?? null
  const [status, setStatus] = useState<AppStatus>(emptyStatus)
  const [nodes, setNodes] = useState<GraphNode[]>([])
  const [edges, setEdges] = useState<GraphEdge[]>([])
  const [files, setFiles] = useState<ProjectFile[]>([])
  const [events, setEvents] = useState<AnalysisEvent[]>([])
  const [diagnosticsByFile] = useState<Map<string, DiagnosticRecord[]>>(new Map())
  const [diagnosticsByNode] = useState<Map<string, DiagnosticRecord[]>>(new Map())
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null)
  const [message, setMessage] = useState<string | null>(null)
  const snapshotSeq = useRef(0)

  const applySnapshot = useCallback((snapshot: GraphSnapshot) => {
    const normalized = normalizeStatus(snapshot.status)
    setStatus(normalized)
    setNodes(snapshot.nodes)
    setEdges(snapshot.edges)
    setFiles(snapshot.files)
    setEvents(snapshot.events)
    setMessage(normalized.message)
  }, [])

  const refreshSnapshot = useCallback(async () => {
    if (!enabled || !workspaceId) return
    const seq = ++snapshotSeq.current
    const response = await cloudFetch(`/api/cloud/workspaces/${encodeURIComponent(workspaceId)}/snapshot?mode=${encodeURIComponent(mode)}`, {}, sessionToken)
    if (response.status === 202) {
      setStatus(current => ({ ...current, appState: 'indexing', analyzerStatus: 'Indexing', message: 'Cloud analysis is still running.' }))
      return
    }
    if (!response.ok) throw new Error(`Cloud snapshot failed with HTTP ${response.status}`)
    const snapshot = await response.json() as GraphSnapshot
    if (seq !== snapshotSeq.current) return
    applySnapshot(snapshot)
  }, [applySnapshot, enabled, mode, workspaceId, sessionToken])

  useEffect(() => {
    if (!enabled || !workspaceId) return
    void refreshSnapshot().catch(error => {
      setStatus(current => ({ ...current, appState: 'error', analyzerStatus: 'Error' }))
      setMessage(error instanceof Error ? error.message : 'Cloud snapshot failed.')
    })
  }, [enabled, refreshSnapshot, workspaceId])

  useEffect(() => {
    if (!enabled || !workspaceId) return
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    const socket = new WebSocket(`${protocol}//${window.location.host}/api/cloud/ws`)
    socket.onmessage = event => {
      const payload = JSON.parse(event.data) as CloudEvent
      if (payload.workspaceId && payload.workspaceId !== workspaceId) return
      if (payload.type === 'snapshotReady') {
        void refreshSnapshot()
      } else if (payload.type === 'jobStatus') {
        setStatus(current => ({
          ...current,
          appState: payload.status === 'completed' ? 'normal' : 'indexing',
          analyzerStatus: payload.status === 'failed' ? 'Error' : payload.status === 'completed' ? 'Ready' : 'Indexing',
          message: payload.message ?? current.message,
          progress: payload.progress == null ? current.progress : Math.round(payload.progress * 100),
        }))
      } else if (payload.type === 'error') {
        setStatus(current => ({ ...current, appState: 'error', analyzerStatus: 'Error', message: payload.message ?? 'Cloud analysis failed.' }))
      }
    }
    return () => socket.close()
  }, [enabled, refreshSnapshot, workspaceId])

  const selectedNode = useMemo(
    () => selectedNodeId ? nodes.find(node => node.id === selectedNodeId) ?? null : null,
    [nodes, selectedNodeId],
  )

  return {
    appState: status.appState as AppState,
    analyzerStatus: status.analyzerStatus as AnalyzerStatus,
    analyzers: status.analyzers,
    pythonAnalyzer: status.pythonAnalyzer,
    projectName: status.projectName,
    projectPath: status.projectPath,
    lastUpdated: status.lastUpdated,
    message: message ?? status.message,
    nodes,
    edges,
    files,
    events,
    diagnosticsByFile,
    diagnosticsByNode,
    selectedNode,
    selectedNodeId,
    setSelectedNodeId,
    openProject: async () => {},
    openInEditor: async () => {
      setMessage('Cloud mode is read-only; open files in your local checkout.')
    },
    saveNodeLayout: async () => {},
    requestFocusBubble: async () => ({ nodes, edges }),
    search: async (query: string) => localSearch(nodes, query),
    refreshSnapshot,
  }
}
