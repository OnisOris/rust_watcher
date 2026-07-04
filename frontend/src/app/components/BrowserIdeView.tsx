import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { ComponentProps, CSSProperties } from 'react'
import Editor from '@monaco-editor/react'
import { AlertTriangle, ArrowLeft, CheckCircle2, ChevronDown, ChevronRight, File, Folder, Play, RefreshCw, Save } from 'lucide-react'
import { analyzeWorkspace, loadWorkspaceFileContent, loadWorkspaceFiles, saveWorkspaceFileContent } from '../api/cloudIde'
import type { WorkspaceFileEntry } from '../api/cloudIde'
import { CloudShellNav, type CloudShellTab } from './CloudShellNav'

interface BrowserIdeViewProps {
  workspaceId: string
  sessionToken: string
  username?: string | null
  theme: 'light' | 'dark'
  initialPath?: string | null
  initialLine?: number | null
  onTabChange: (tab: CloudShellTab) => void
  onBackToGraph: () => void
}

interface FileTreeNode {
  id: string
  name: string
  path: string
  type: 'directory' | 'file'
  children: FileTreeNode[]
  file?: WorkspaceFileEntry
}

export function BrowserIdeView({ workspaceId, sessionToken, username, theme, initialPath, initialLine, onTabChange, onBackToGraph }: BrowserIdeViewProps) {
  const [files, setFiles] = useState<WorkspaceFileEntry[]>([])
  const [revisionId, setRevisionId] = useState<string | null>(null)
  const [selectedPath, setSelectedPath] = useState<string | null>(null)
  const [content, setContent] = useState('')
  const [savedContent, setSavedContent] = useState('')
  const [loadingFiles, setLoadingFiles] = useState(true)
  const [loadingContent, setLoadingContent] = useState(false)
  const [saving, setSaving] = useState(false)
  const [analyzing, setAnalyzing] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const loadSeq = useRef(0)
  const editorRef = useRef<Parameters<NonNullable<ComponentProps<typeof Editor>['onMount']>>[0] | null>(null)

  const dirty = content !== savedContent
  const selectedFile = selectedPath ? files.find(file => file.path === selectedPath) ?? null : null
  const fileTree = useMemo(() => buildFileTree(files), [files])
  const editorLanguage = selectedPath ? languageForPath(selectedPath) : 'plaintext'

  const loadFiles = useCallback(async (preferredPath?: string | null) => {
    setLoadingFiles(true)
    setError(null)
    try {
      const payload = await loadWorkspaceFiles(workspaceId, sessionToken)
      setFiles(payload.files)
      setRevisionId(payload.revisionId)
      setExpanded(defaultExpandedDirectories(payload.files))
      setSelectedPath(current => {
        const nextPreferred = preferredPath ?? current
        if (nextPreferred && payload.files.some(file => file.path === nextPreferred)) return nextPreferred
        return payload.files[0]?.path ?? null
      })
    } catch (error) {
      setError(error instanceof Error ? error.message : 'Workspace files failed to load.')
    } finally {
      setLoadingFiles(false)
    }
  }, [sessionToken, workspaceId])

  useEffect(() => {
    void loadFiles(initialPath)
  }, [initialPath, loadFiles])

  useEffect(() => {
    if (!selectedPath) {
      setContent('')
      setSavedContent('')
      return
    }
    const seq = ++loadSeq.current
    setLoadingContent(true)
    setError(null)
    void loadWorkspaceFileContent(workspaceId, selectedPath, sessionToken, revisionId ?? undefined)
      .then(payload => {
        if (seq !== loadSeq.current) return
        setRevisionId(payload.revisionId)
        setContent(payload.content)
        setSavedContent(payload.content)
        setMessage(`${payload.file.path} loaded`)
        if (initialLine) {
          window.setTimeout(() => {
            editorRef.current?.revealLineInCenter(initialLine)
            editorRef.current?.setPosition({ lineNumber: initialLine, column: 1 })
            editorRef.current?.focus()
          }, 0)
        }
      })
      .catch(error => {
        if (seq !== loadSeq.current) return
        setError(error instanceof Error ? error.message : 'File failed to load.')
      })
      .finally(() => {
        if (seq === loadSeq.current) setLoadingContent(false)
      })
  }, [initialLine, revisionId, selectedPath, sessionToken, workspaceId])

  const selectFile = useCallback((path: string) => {
    if (path === selectedPath) return
    if (dirty && !window.confirm('Discard unsaved changes?')) return
    setSelectedPath(path)
  }, [dirty, selectedPath])

  const saveFile = useCallback(async () => {
    if (!selectedPath || !revisionId || saving || !dirty) return
    setSaving(true)
    setError(null)
    try {
      const response = await saveWorkspaceFileContent(workspaceId, selectedPath, content, revisionId, sessionToken)
      setRevisionId(response.revisionId)
      setSavedContent(content)
      setFiles(current => current.map(file => file.path === selectedPath ? response.file : file))
      setMessage(`Saved ${selectedPath}`)
    } catch (error) {
      setError(error instanceof Error ? error.message : 'File failed to save.')
    } finally {
      setSaving(false)
    }
  }, [content, dirty, revisionId, saving, selectedPath, sessionToken, workspaceId])

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== 's') return
      if (!selectedPath) return
      event.preventDefault()
      void saveFile()
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [saveFile, selectedPath])

  async function startAnalysis() {
    setAnalyzing(true)
    setError(null)
    try {
      const response = await analyzeWorkspace(workspaceId, sessionToken)
      setMessage(response.message ?? `Analysis queued: ${response.jobId.slice(0, 8)}`)
    } catch (error) {
      setError(error instanceof Error ? error.message : 'Analysis failed to start.')
    } finally {
      setAnalyzing(false)
    }
  }

  function toggleDirectory(path: string) {
    setExpanded(current => {
      const next = new Set(current)
      next.has(path) ? next.delete(path) : next.add(path)
      return next
    })
  }

  return (
    <div className="w-full h-full flex flex-col overflow-hidden" style={{ background: 'var(--cc-bg)', color: 'var(--cc-text)', fontFamily: 'Inter, sans-serif' }}>
      <CloudShellNav activeTab="ide" username={username} graphEnabled onNavigate={onTabChange} />

      <div className="flex-1 min-h-0 flex overflow-hidden">
        <aside
          className="shrink-0 flex flex-col min-h-0"
          style={{ width: 320, background: 'var(--cc-panel)', borderRight: '1px solid var(--cc-border)' }}
        >
          <div className="shrink-0 px-3 py-2" style={{ borderBottom: '1px solid var(--cc-border)' }}>
            <div className="flex items-center justify-between gap-2">
              <div className="min-w-0">
                <div style={{ fontSize: 12, fontWeight: 780, color: 'var(--cc-text)', textTransform: 'uppercase' }}>
                  Workspace files
                </div>
                <div style={{ marginTop: 2, fontSize: 10, color: 'var(--cc-text-subtle)' }}>
                  {files.length} files · {revisionId ? revisionId.slice(0, 8) : 'no revision'}
                </div>
              </div>
              <button
                onClick={() => void loadFiles(selectedPath)}
                disabled={loadingFiles}
                title="Refresh files"
                className="flex items-center justify-center"
                style={iconButtonStyle}
              >
                <RefreshCw size={14} />
              </button>
            </div>
          </div>

          <div className="flex-1 min-h-0 overflow-auto py-2" style={{ scrollbarWidth: 'thin', scrollbarColor: 'var(--cc-border) transparent' }}>
            {loadingFiles ? (
              <div style={{ padding: 14, fontSize: 12, color: 'var(--cc-text-muted)' }}>Loading files...</div>
            ) : fileTree.length === 0 ? (
              <div style={{ padding: 14, fontSize: 12, color: 'var(--cc-text-muted)' }}>No editable files found.</div>
            ) : (
              fileTree.map(node => (
                <TreeNodeRow
                  key={node.id}
                  node={node}
                  depth={0}
                  expanded={expanded}
                  selectedPath={selectedPath}
                  onToggle={toggleDirectory}
                  onSelect={selectFile}
                />
              ))
            )}
          </div>
        </aside>

        <main className="flex-1 min-w-0 flex flex-col overflow-hidden">
          <div className="shrink-0 flex items-center justify-between gap-3 px-4 py-2" style={{ minHeight: 48, background: 'var(--cc-panel)', borderBottom: '1px solid var(--cc-border)' }}>
            <div className="min-w-0 flex items-center gap-3">
              <button
                onClick={onBackToGraph}
                title="Back to graph"
                className="flex items-center gap-1.5 shrink-0"
                style={secondaryButtonStyle}
              >
                <ArrowLeft size={14} />
                Graph
              </button>
              <div className="min-w-0">
              <div style={{ fontSize: 13, fontWeight: 760, color: 'var(--cc-text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                {selectedPath ?? 'No file selected'}{dirty ? ' *' : ''}
              </div>
              <div style={{ marginTop: 2, fontSize: 10, color: 'var(--cc-text-subtle)' }}>
                {selectedFile ? `${formatBytes(selectedFile.sizeBytes)} · ${selectedFile.language ?? editorLanguage}` : 'Choose a file'}
              </div>
              </div>
            </div>
            <div className="flex items-center gap-2 shrink-0">
              <button
                onClick={() => void saveFile()}
                disabled={!dirty || saving || !selectedPath}
                className="flex items-center gap-1.5"
                style={buttonStyle(!dirty || saving || !selectedPath)}
              >
                <Save size={14} />
                {saving ? 'Saving' : 'Save'}
              </button>
              <button
                onClick={() => void startAnalysis()}
                disabled={analyzing || dirty || !revisionId}
                className="flex items-center gap-1.5"
                style={buttonStyle(analyzing || dirty || !revisionId)}
              >
                <Play size={14} />
                {analyzing ? 'Queued' : 'Analyze'}
              </button>
            </div>
          </div>

          {(error || message) && (
            <div
              className="shrink-0 flex items-center gap-2 px-4 py-2"
              style={{
                minHeight: 36,
                background: error ? 'rgba(220,38,38,0.08)' : 'rgba(14,165,233,0.08)',
                borderBottom: '1px solid var(--cc-border)',
                color: error ? '#DC2626' : 'var(--cc-text-muted)',
                fontSize: 12,
              }}
            >
              {error ? <AlertTriangle size={14} /> : <CheckCircle2 size={14} />}
              <span style={{ minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{error ?? message}</span>
            </div>
          )}

          <div className="flex-1 min-h-0 relative" style={{ background: theme === 'dark' ? '#1e1e1e' : '#ffffff' }}>
            {selectedPath ? (
              <Editor
                key={selectedPath}
                value={content}
                language={editorLanguage}
                theme={theme === 'dark' ? 'vs-dark' : 'light'}
                loading={<div style={{ padding: 16, color: 'var(--cc-text-muted)', fontSize: 12 }}>Loading editor...</div>}
                options={{
                  automaticLayout: true,
                  fontSize: 13,
                  fontFamily: 'JetBrains Mono, SFMono-Regular, Consolas, monospace',
                  minimap: { enabled: true },
                  scrollBeyondLastLine: false,
                  wordWrap: 'off',
                  tabSize: 2,
                  renderWhitespace: 'selection',
                  fixedOverflowWidgets: true,
                }}
                onChange={value => setContent(value ?? '')}
                onMount={editor => {
                  editorRef.current = editor
                  if (initialLine) {
                    editor.revealLineInCenter(initialLine)
                    editor.setPosition({ lineNumber: initialLine, column: 1 })
                  }
                }}
              />
            ) : (
              <div className="w-full h-full flex items-center justify-center" style={{ color: 'var(--cc-text-muted)', fontSize: 13 }}>
                Select a file from the workspace.
              </div>
            )}
            {loadingContent && (
              <div
                className="absolute inset-x-0 top-0"
                style={{ height: 2, background: 'linear-gradient(90deg, #0EA5E9, #22C55E)' }}
              />
            )}
          </div>

          <div className="shrink-0 flex items-center justify-between gap-3 px-3" style={{ minHeight: 28, background: 'var(--cc-panel)', borderTop: '1px solid var(--cc-border)', color: 'var(--cc-text-subtle)', fontSize: 11 }}>
            <span style={{ minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{workspaceId}</span>
            <span className="shrink-0">{dirty ? 'Unsaved changes' : 'Saved'}</span>
          </div>
        </main>
      </div>
    </div>
  )
}

function TreeNodeRow({
  node,
  depth,
  expanded,
  selectedPath,
  onToggle,
  onSelect,
}: {
  node: FileTreeNode
  depth: number
  expanded: Set<string>
  selectedPath: string | null
  onToggle: (path: string) => void
  onSelect: (path: string) => void
}) {
  const isDirectory = node.type === 'directory'
  const open = expanded.has(node.path)
  const selected = node.path === selectedPath
  return (
    <div>
      <button
        onClick={() => isDirectory ? onToggle(node.path) : onSelect(node.path)}
        className="w-full flex items-center gap-1.5"
        title={node.path}
        style={{
          minHeight: 26,
          padding: `0 8px 0 ${10 + depth * 14}px`,
          background: selected ? 'var(--cc-selected-soft)' : 'transparent',
          color: selected ? 'var(--cc-accent)' : isDirectory ? 'var(--cc-text-muted)' : 'var(--cc-text)',
          cursor: 'pointer',
          borderLeft: selected ? '2px solid var(--cc-accent)' : '2px solid transparent',
          fontSize: 12,
        }}
      >
        <span className="shrink-0" style={{ width: 13 }}>
          {isDirectory ? (open ? <ChevronDown size={12} /> : <ChevronRight size={12} />) : null}
        </span>
        {isDirectory ? <Folder size={13} color="var(--cc-module)" /> : <File size={13} color="var(--cc-file)" />}
        <span style={{ minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{node.name}</span>
        {isDirectory && <span style={{ marginLeft: 'auto', color: 'var(--cc-text-faint)', fontSize: 10 }}>{node.children.length}</span>}
      </button>
      {isDirectory && open && node.children.map(child => (
        <TreeNodeRow
          key={child.id}
          node={child}
          depth={depth + 1}
          expanded={expanded}
          selectedPath={selectedPath}
          onToggle={onToggle}
          onSelect={onSelect}
        />
      ))}
    </div>
  )
}

function buildFileTree(files: WorkspaceFileEntry[]) {
  const root: FileTreeNode[] = []
  const directories = new Map<string, FileTreeNode>()
  const sorted = [...files].sort((left, right) => left.path.localeCompare(right.path))
  for (const file of sorted) {
    const parts = file.path.split('/').filter(Boolean)
    let children = root
    let directoryPath = ''
    parts.forEach((part, index) => {
      const path = directoryPath ? `${directoryPath}/${part}` : part
      const isFile = index === parts.length - 1
      if (isFile) {
        children.push({ id: `file:${file.path}`, name: part, path: file.path, type: 'file', children: [], file })
        return
      }
      let directory = directories.get(path)
      if (!directory) {
        directory = { id: `dir:${path}`, name: part, path, type: 'directory', children: [] }
        directories.set(path, directory)
        children.push(directory)
      }
      directoryPath = path
      children = directory.children
    })
  }
  sortTree(root)
  return root
}

function sortTree(nodes: FileTreeNode[]) {
  nodes.sort((left, right) => {
    if (left.type !== right.type) return left.type === 'directory' ? -1 : 1
    return left.name.localeCompare(right.name)
  })
  nodes.forEach(node => sortTree(node.children))
}

function defaultExpandedDirectories(files: WorkspaceFileEntry[]) {
  const expanded = new Set<string>()
  files.slice(0, 80).forEach(file => {
    const parts = file.path.split('/').filter(Boolean)
    let current = ''
    parts.slice(0, -1).forEach(part => {
      current = current ? `${current}/${part}` : part
      expanded.add(current)
    })
  })
  return expanded
}

function languageForPath(path: string) {
  const extension = path.split('.').pop()?.toLowerCase()
  switch (extension) {
    case 'rs':
      return 'rust'
    case 'ts':
      return 'typescript'
    case 'tsx':
      return 'typescript'
    case 'js':
    case 'jsx':
      return 'javascript'
    case 'py':
      return 'python'
    case 'json':
      return 'json'
    case 'toml':
      return 'toml'
    case 'md':
      return 'markdown'
    case 'css':
      return 'css'
    case 'html':
      return 'html'
    case 'yaml':
    case 'yml':
      return 'yaml'
    default:
      return 'plaintext'
  }
}

function formatBytes(value: number) {
  if (value < 1024) return `${value} B`
  if (value < 1024 * 1024) return `${Math.round(value / 1024)} KB`
  return `${(value / (1024 * 1024)).toFixed(1)} MB`
}

function buttonStyle(disabled: boolean): CSSProperties {
  return {
    minHeight: 32,
    padding: '0 11px',
    borderRadius: 8,
    border: '1px solid var(--cc-border)',
    background: disabled ? 'var(--cc-surface)' : 'var(--cc-accent)',
    color: disabled ? 'var(--cc-text-faint)' : '#fff',
    fontSize: 12,
    fontWeight: 740,
    cursor: disabled ? 'not-allowed' : 'pointer',
  }
}

const iconButtonStyle: CSSProperties = {
  width: 30,
  height: 30,
  borderRadius: 8,
  border: '1px solid var(--cc-border)',
  background: 'var(--cc-surface)',
  color: 'var(--cc-text-muted)',
  cursor: 'pointer',
}

const secondaryButtonStyle: CSSProperties = {
  minHeight: 32,
  padding: '0 10px',
  borderRadius: 8,
  border: '1px solid var(--cc-border-strong)',
  background: 'var(--cc-surface)',
  color: 'var(--cc-text)',
  fontSize: 12,
  fontWeight: 740,
  cursor: 'pointer',
}
