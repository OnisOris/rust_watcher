import { useCallback, useEffect, useMemo, useState } from 'react'
import type { CSSProperties, ReactNode } from 'react'
import { Code2, CreditCard, FileArchive, FolderOpen, GitBranch, LogOut, PlugZap, UploadCloud, UserRound } from 'lucide-react'
import { cloudFetch } from '../api/cloudAuth'
import { CloudShellNav, type CloudShellTab } from './CloudShellNav'

interface CloudPortalProps {
  sessionToken: string
  username?: string | null
  activeTab: CloudShellTab
  graphEnabled?: boolean
  onTabChange: (tab: CloudShellTab) => void
  onWorkspaceReady: (workspaceId: string) => void
  onLogout: () => void
}

interface CloudStartResponse {
  workspaceId: string
  jobId: string
  status: string
}

interface CloudJob {
  jobId: string
  workspaceId?: string
  status: CloudJobStatus
  message?: string
  progress?: number
  creditsUsed?: number
  analysisMode?: 'full' | 'incremental' | 'fallback-full'
  fallbackReason?: string
}

type CloudJobStatus = 'creating' | 'queued' | 'importing' | 'indexing' | 'analyzing' | 'buildingGraph' | 'completed' | 'failed' | 'cancelled'

interface CloudUsage {
  creditsRemaining: number
  creditsUsed: number
  jobs: Array<{ jobId: string; workspaceId?: string; credits: number; reason: string }>
}

interface CloudWorkspace {
  workspaceId: string
  name: string
  source: { type: string; url?: string | null; ref?: string | null }
  status: string
  fileCount: number
  lastJobId?: string
  lastUpdated?: string
}

const steps = [
  'Importing repository',
  'Indexing files',
  'Running rust-analyzer',
  'Running ty',
  'Running qmlls',
  'Building graph',
  'Completed',
]

export function CloudPortal({ sessionToken, username, activeTab, graphEnabled = false, onTabChange, onWorkspaceReady, onLogout }: CloudPortalProps) {
  const [githubUrl, setGithubUrl] = useState('')
  const [gitRef, setGitRef] = useState('')
  const [job, setJob] = useState<CloudJob | null>(null)
  const [usage, setUsage] = useState<CloudUsage | null>(null)
  const [workspaces, setWorkspaces] = useState<CloudWorkspace[]>([])
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const loadUsage = useCallback(async () => {
    const response = await cloudFetch('/api/cloud/usage', {}, sessionToken)
    if (response.ok) setUsage(await response.json())
  }, [sessionToken])

  const loadWorkspaces = useCallback(async () => {
    const response = await cloudFetch('/api/cloud/workspaces', {}, sessionToken)
    if (response.ok) {
      const payload = await response.json() as { workspaces: CloudWorkspace[] }
      setWorkspaces(payload.workspaces)
    }
  }, [sessionToken])

  useEffect(() => {
    void loadUsage()
    void loadWorkspaces()
  }, [loadUsage, loadWorkspaces])

  useEffect(() => {
    if (!job || job.status === 'completed' || job.status === 'failed' || job.status === 'cancelled') return
    const id = window.setInterval(async () => {
      const response = await cloudFetch(`/api/cloud/jobs/${encodeURIComponent(job.jobId)}`, {}, sessionToken)
      if (!response.ok) return
      const next = normalizeCloudJob(await response.json())
      setJob(next)
      if (next.status === 'completed' && next.workspaceId) {
        await loadUsage()
        await loadWorkspaces()
        onWorkspaceReady(next.workspaceId)
      }
    }, 1800)
    return () => window.clearInterval(id)
  }, [job, loadUsage, loadWorkspaces, onWorkspaceReady, sessionToken])

  const progress = Math.round((job?.progress ?? 0) * 100)
  const currentStep = useMemo(() => {
    if (!job) return 0
    if (job.status === 'queued') return 0
    if (job.status === 'importing') return 1
    if (job.status === 'indexing') return 2
    if (job.status === 'analyzing') return 4
    if (job.status === 'buildingGraph') return 6
    if (job.status === 'completed') return steps.length
    return 0
  }, [job])

  async function startGithub() {
    if (!githubUrl.trim()) return
    setBusy(true)
    setError(null)
    setJob({ jobId: 'creating', status: 'creating', message: 'Creating cloud job and importing repository...', progress: 0.02 })
    try {
      const response = await cloudFetch('/api/cloud/import/github', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ url: githubUrl.trim(), ref: gitRef.trim() || undefined }),
      }, sessionToken)
      if (!response.ok) throw new Error(await response.text())
      const payload = await response.json() as CloudStartResponse
      await loadWorkspaces()
      setJob(normalizeCloudJob({ jobId: payload.jobId, workspaceId: payload.workspaceId, status: payload.status, progress: 0.05, message: 'Queued for cloud analysis' }))
    } catch (error) {
      setJob(null)
      setError(error instanceof Error ? error.message : 'GitHub import failed.')
    } finally {
      setBusy(false)
    }
  }

  async function uploadZip(file: File | null) {
    if (!file) return
    setBusy(true)
    setError(null)
    setJob({ jobId: 'creating', status: 'creating', message: 'Uploading archive and creating cloud job...', progress: 0.02 })
    try {
      const form = new FormData()
      form.append('file', file)
      const response = await cloudFetch('/api/cloud/upload', { method: 'POST', body: form }, sessionToken)
      if (!response.ok) throw new Error(await response.text())
      const payload = await response.json() as CloudStartResponse
      await loadWorkspaces()
      setJob(normalizeCloudJob({ jobId: payload.jobId, workspaceId: payload.workspaceId, status: payload.status, progress: 0.05, message: 'Queued for cloud analysis' }))
    } catch (error) {
      setJob(null)
      setError(error instanceof Error ? error.message : 'ZIP upload failed.')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="w-full h-full flex flex-col overflow-hidden" style={{ background: 'var(--cc-bg)', color: 'var(--cc-text)', fontFamily: 'Inter, sans-serif' }}>
      <CloudShellNav activeTab={activeTab} username={username} graphEnabled={graphEnabled} onNavigate={onTabChange} />
      <div className="flex-1 overflow-auto">
        <div style={{ maxWidth: 1180, margin: '0 auto', padding: '28px 24px 48px' }}>
          <div className="flex items-start justify-between gap-4" style={{ marginBottom: 22 }}>
            <div>
              <h1 style={{ fontSize: 28, fontWeight: 780, margin: 0, letterSpacing: 0 }}>{titleForTab(activeTab)}</h1>
              <p style={{ marginTop: 8, color: 'var(--cc-text-muted)', fontSize: 14 }}>{subtitleForTab(activeTab)}</p>
            </div>
            <CreditsCard usage={usage} />
          </div>

          {error && (
            <div style={{ marginBottom: 16, padding: 12, borderRadius: 8, background: '#FEE2E2', color: '#991B1B', border: '1px solid #FCA5A5', fontSize: 13 }}>
              {error}
            </div>
          )}

          {activeTab === 'workspaces' && (
            <section style={panelStyle}>
              <div className="flex items-center justify-between gap-3" style={{ marginBottom: 14 }}>
                <div className="flex items-center gap-2">
                  <FolderOpen size={18} />
                  <h2 style={{ fontSize: 16, fontWeight: 760, margin: 0 }}>Your workspaces</h2>
                </div>
                <button onClick={() => onTabChange('new')} style={{ ...primaryButtonStyle, marginTop: 0 }}>
                  <UploadCloud size={15} />
                  New analysis
                </button>
              </div>

              {workspaces.length === 0 ? (
                <div style={{ minHeight: 150, display: 'grid', placeItems: 'center', color: 'var(--cc-text-muted)', border: '1px dashed var(--cc-border-strong)', borderRadius: 8 }}>
                  No workspaces yet.
                </div>
              ) : (
                <div className="grid gap-3" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(300px, 1fr))' }}>
                  {workspaces.map(workspace => (
                    <div key={workspace.workspaceId} style={{ border: '1px solid var(--cc-border)', borderRadius: 8, padding: 14, background: 'var(--cc-surface)' }}>
                      <div className="flex items-start justify-between gap-3">
                        <div style={{ minWidth: 0 }}>
                          <div style={{ fontSize: 14, fontWeight: 760, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{workspace.name}</div>
                          <div style={{ marginTop: 5, fontSize: 11, color: statusColor(workspace.status) }}>
                            {workspace.status} · {workspace.fileCount} files · {sourceLabel(workspace.source)}
                          </div>
                          {workspace.lastUpdated && (
                            <div style={{ marginTop: 5, fontSize: 11, color: 'var(--cc-text-faint)' }}>{formatTimestamp(workspace.lastUpdated)}</div>
                          )}
                        </div>
                        <button onClick={() => onWorkspaceReady(workspace.workspaceId)} style={smallButtonStyle}>Open graph</button>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </section>
          )}

          {activeTab === 'new' && (
            <>
              <div className="grid gap-4" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(260px, 1fr))' }}>
                <section style={panelStyle}>
                  <div className="flex items-center gap-2" style={{ marginBottom: 12 }}>
                    <GitBranch size={18} />
                    <h2 style={{ fontSize: 15, fontWeight: 740, margin: 0 }}>GitHub repository</h2>
                  </div>
                  <input value={githubUrl} onChange={event => setGithubUrl(event.target.value)} placeholder="https://github.com/owner/repo" style={inputStyle} />
                  <input value={gitRef} onChange={event => setGitRef(event.target.value)} placeholder="branch, tag, or commit (optional)" style={{ ...inputStyle, marginTop: 8 }} />
                  <button onClick={startGithub} disabled={busy || !githubUrl.trim()} style={primaryButtonStyle}>
                    <UploadCloud size={15} />
                    Analyze
                  </button>
                </section>

                <section style={panelStyle}>
                  <div className="flex items-center gap-2" style={{ marginBottom: 12 }}>
                    <FileArchive size={18} />
                    <h2 style={{ fontSize: 15, fontWeight: 740, margin: 0 }}>Upload ZIP</h2>
                  </div>
                  <label className="flex flex-col items-center justify-center" style={{ minHeight: 132, border: '1px dashed var(--cc-border-strong)', borderRadius: 8, cursor: 'pointer', color: 'var(--cc-text-muted)' }}>
                    <UploadCloud size={24} />
                    <span style={{ marginTop: 8, fontSize: 13 }}>Choose a .zip project archive</span>
                    <input type="file" accept=".zip,application/zip" hidden onChange={event => void uploadZip(event.target.files?.[0] ?? null)} />
                  </label>
                </section>

                <section style={panelStyle}>
                  <div className="flex items-center gap-2" style={{ marginBottom: 12 }}>
                    <PlugZap size={18} />
                    <h2 style={{ fontSize: 15, fontWeight: 740, margin: 0 }}>Connect local agent</h2>
                  </div>
                  <pre style={{ margin: 0, whiteSpace: 'pre-wrap', background: 'var(--cc-surface)', border: '1px solid var(--cc-border)', borderRadius: 8, padding: 12, fontSize: 11, color: 'var(--cc-text)' }}>
{`cargo run -p local-agent -- connect \\
  --project /path/to/project \\
  --server ${window.location.origin} \\
  --token ${sessionToken}`}
                  </pre>
                </section>
              </div>

              {job && (
                <section style={{ ...panelStyle, marginTop: 18 }}>
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <div style={{ fontSize: 13, fontWeight: 740 }}>{job.jobId === 'creating' ? 'Preparing analysis' : `Job ${job.jobId.slice(0, 8)}`}</div>
                      <div style={{ fontSize: 12, color: statusColor(job.status), marginTop: 4 }}>
                        {job.analysisMode === 'fallback-full' ? `Fallback full: ${job.fallbackReason ?? job.message ?? statusLabel(job.status)}` : job.message ?? statusLabel(job.status)}
                      </div>
                    </div>
                    {job.status === 'completed' && job.workspaceId && (
                      <button onClick={() => onWorkspaceReady(job.workspaceId!)} style={{ ...primaryButtonStyle, marginTop: 0 }}>Open graph</button>
                    )}
                  </div>
                  <div style={{ height: 8, background: 'var(--cc-surface)', borderRadius: 999, overflow: 'hidden', marginTop: 14 }}>
                    <div style={{ height: '100%', width: `${progress}%`, background: '#06B6D4' }} />
                  </div>
                  <div className="grid gap-2" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))', marginTop: 14 }}>
                    {steps.map((step, index) => (
                      <div key={step} style={{ fontSize: 11, color: index < currentStep ? 'var(--cc-text)' : 'var(--cc-text-faint)' }}>
                        {index < currentStep ? 'Done: ' : ''}{step}
                      </div>
                    ))}
                  </div>
                </section>
              )}
            </>
          )}

          {activeTab === 'account' && (
            <section style={panelStyle}>
              <div className="grid gap-3" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(240px, 1fr))' }}>
                <InfoTile icon={<UserRound size={18} />} label="Signed in" value={username ?? 'Cloud user'} />
                <InfoTile icon={<CreditCard size={18} />} label="Credits" value={`${usage?.creditsRemaining ?? 1000} remaining`} />
                <InfoTile icon={<PlugZap size={18} />} label="Agent token" value="Configured on server" />
              </div>
              <div style={{ marginTop: 16, paddingTop: 16, borderTop: '1px solid var(--cc-border)' }}>
                <button onClick={onLogout} style={{ ...smallButtonStyle, minHeight: 36, color: '#B91C1C' }}>
                  <LogOut size={14} />
                  Sign out
                </button>
              </div>
            </section>
          )}

          {activeTab === 'ide' && (
            <section style={panelStyle}>
              <div className="flex items-center gap-3">
                <Code2 size={22} color="#06B6D4" />
                <div>
                  <div style={{ fontSize: 15, fontWeight: 760 }}>Browser IDE is planned</div>
                  <div style={{ marginTop: 5, fontSize: 13, color: 'var(--cc-text-muted)' }}>
                    This tab is reserved for a future VS Code-compatible web workspace.
                  </div>
                </div>
              </div>
            </section>
          )}
        </div>
      </div>
    </div>
  )
}

function CreditsCard({ usage }: { usage: CloudUsage | null }) {
  return (
    <div style={{ minWidth: 190, background: 'var(--cc-panel)', border: '1px solid var(--cc-border)', borderRadius: 8, padding: 12 }}>
      <div style={{ fontSize: 11, color: 'var(--cc-text-subtle)', fontWeight: 700, textTransform: 'uppercase' }}>Credits</div>
      <div style={{ marginTop: 6, fontSize: 22, fontWeight: 760 }}>{usage?.creditsRemaining ?? 1000}</div>
      <div style={{ fontSize: 11, color: 'var(--cc-text-muted)' }}>{usage?.creditsUsed ?? 0} used</div>
    </div>
  )
}

function InfoTile({ icon, label, value }: { icon: ReactNode; label: string; value: string }) {
  return (
    <div style={{ border: '1px solid var(--cc-border)', borderRadius: 8, background: 'var(--cc-surface)', padding: 14 }}>
      <div className="flex items-center gap-2" style={{ color: 'var(--cc-text-muted)' }}>
        {icon}
        <span style={{ fontSize: 11, fontWeight: 760, textTransform: 'uppercase' }}>{label}</span>
      </div>
      <div style={{ marginTop: 8, fontSize: 15, fontWeight: 760, color: 'var(--cc-text)' }}>{value}</div>
    </div>
  )
}

function titleForTab(tab: CloudShellTab) {
  if (tab === 'workspaces') return 'Workspaces'
  if (tab === 'account') return 'Account settings'
  if (tab === 'ide') return 'Browser IDE'
  return 'Analyze your codebase'
}

function subtitleForTab(tab: CloudShellTab) {
  if (tab === 'workspaces') return 'Open previous cloud analyses or start a new one.'
  if (tab === 'account') return 'Manage session, credits, agent access, and cloud preferences.'
  if (tab === 'ide') return 'A browser workspace is planned for a future release.'
  return 'Upload a project, import a public GitHub repository, or connect a local agent. Analysis runs on the server.'
}

function normalizeCloudJob(payload: any): CloudJob {
  const status = normalizeStatus(payload.status)
  const progress = typeof payload.progress === 'number' ? payload.progress : progressForStatus(status)
  return {
    jobId: payload.jobId,
    workspaceId: payload.workspaceId,
    status,
    message: payload.message ?? statusLabel(status),
    progress,
    creditsUsed: payload.creditsUsed,
    analysisMode: payload.analysisMode,
    fallbackReason: payload.fallbackReason,
  }
}

function normalizeStatus(status: string): CloudJobStatus {
  switch (status) {
    case 'queued':
      return 'queued'
    case 'preparing':
    case 'importing':
      return 'importing'
    case 'indexing':
      return 'indexing'
    case 'runningAnalyzers':
    case 'analyzing':
      return 'analyzing'
    case 'buildingGraph':
      return 'buildingGraph'
    case 'completed':
      return 'completed'
    case 'failed':
      return 'failed'
    case 'cancelled':
      return 'cancelled'
    default:
      return 'queued'
  }
}

function progressForStatus(status: CloudJobStatus) {
  switch (status) {
    case 'creating':
      return 0.02
    case 'queued':
      return 0.05
    case 'importing':
      return 0.12
    case 'indexing':
      return 0.35
    case 'analyzing':
      return 0.65
    case 'buildingGraph':
      return 0.85
    case 'completed':
      return 1
    default:
      return 0
  }
}

function statusLabel(status: CloudJobStatus) {
  switch (status) {
    case 'creating':
      return 'Creating cloud job'
    case 'queued':
      return 'Queued for cloud analysis'
    case 'importing':
      return 'Importing repository'
    case 'indexing':
      return 'Indexing files'
    case 'analyzing':
      return 'Running analyzers'
    case 'buildingGraph':
      return 'Building graph'
    case 'completed':
      return 'Cloud analysis completed'
    case 'failed':
      return 'Cloud analysis failed'
    case 'cancelled':
      return 'Cloud analysis cancelled'
  }
}

function sourceLabel(source: CloudWorkspace['source']) {
  if (source.type === 'github') return 'GitHub'
  if (source.type === 'zip') return 'ZIP'
  if (source.type === 'agent') return 'Local agent'
  return 'Cloud'
}

function formatTimestamp(value: string) {
  const numeric = Number(value)
  const date = Number.isFinite(numeric)
    ? new Date(numeric < 10_000_000_000 ? numeric * 1000 : numeric)
    : new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return date.toLocaleString()
}

function statusColor(status: string) {
  if (status === 'failed') return '#B91C1C'
  if (status === 'completed' || status === 'ready') return '#047857'
  if (status === 'analyzing' || status === 'buildingGraph') return '#0369A1'
  return 'var(--cc-text-muted)'
}

const panelStyle: CSSProperties = {
  background: 'var(--cc-panel)',
  border: '1px solid var(--cc-border)',
  borderRadius: 8,
  padding: 16,
}

const inputStyle: CSSProperties = {
  width: '100%',
  background: 'var(--cc-surface)',
  border: '1px solid var(--cc-border)',
  borderRadius: 8,
  padding: '10px 11px',
  fontSize: 13,
  color: 'var(--cc-text)',
  outline: 'none',
}

const primaryButtonStyle: CSSProperties = {
  marginTop: 12,
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  gap: 8,
  minHeight: 36,
  padding: '0 14px',
  border: 'none',
  borderRadius: 8,
  background: '#06B6D4',
  color: '#fff',
  fontSize: 13,
  fontWeight: 720,
  cursor: 'pointer',
}

const smallButtonStyle: CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  gap: 7,
  minHeight: 30,
  padding: '0 10px',
  border: '1px solid var(--cc-border-strong)',
  borderRadius: 8,
  background: 'var(--cc-panel)',
  color: 'var(--cc-text)',
  fontSize: 12,
  fontWeight: 700,
  cursor: 'pointer',
  whiteSpace: 'nowrap',
}
