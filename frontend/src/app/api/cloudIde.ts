import { cloudFetch } from './cloudAuth'

export interface WorkspaceFileEntry {
  path: string
  contentHash: string
  sizeBytes: number
  language?: string
}

export interface WorkspaceFilesResponse {
  workspaceId: string
  revisionId: string
  files: WorkspaceFileEntry[]
}

export interface WorkspaceFileContentResponse {
  workspaceId: string
  revisionId: string
  file: WorkspaceFileEntry
  content: string
}

export interface SaveWorkspaceFileResponse {
  workspaceId: string
  revisionId: string
  file: WorkspaceFileEntry
  filesCount: number
  totalBytes: number
}

export interface WorkspaceRevisionFileDiffEntry {
  path: string
  oldSizeBytes?: number
  newSizeBytes?: number
  oldContentHash?: string
  newContentHash?: string
}

export interface WorkspaceRevisionDiffResponse {
  workspaceId: string
  baseRevisionId: string
  headRevisionId: string
  addedFiles: WorkspaceRevisionFileDiffEntry[]
  removedFiles: WorkspaceRevisionFileDiffEntry[]
  modifiedFiles: WorkspaceRevisionFileDiffEntry[]
  unchangedCount: number
}

export interface CloudAnalyzeResponse {
  jobId: string
  workspaceId?: string
  status: string
  message?: string
  progress?: number
}

export class CloudIdeHttpError extends Error {
  status: number

  constructor(message: string, status: number) {
    super(message)
    this.name = 'CloudIdeHttpError'
    this.status = status
  }
}

async function responseError(response: Response, fallback: string) {
  const message = await response.text()
  return new CloudIdeHttpError(message.trim() || `${fallback} failed with HTTP ${response.status}`, response.status)
}

export async function loadWorkspaceFiles(workspaceId: string, sessionToken: string) {
  const response = await cloudFetch(
    `/api/cloud/workspaces/${encodeURIComponent(workspaceId)}/files`,
    {},
    sessionToken,
  )
  if (!response.ok) throw await responseError(response, 'Loading workspace files')
  return await response.json() as WorkspaceFilesResponse
}

export async function loadWorkspaceFileContent(
  workspaceId: string,
  path: string,
  sessionToken: string,
  revisionId?: string,
) {
  const params = new URLSearchParams({ path })
  if (revisionId) params.set('revisionId', revisionId)
  const response = await cloudFetch(
    `/api/cloud/workspaces/${encodeURIComponent(workspaceId)}/files/content?${params}`,
    {},
    sessionToken,
  )
  if (!response.ok) throw await responseError(response, 'Loading file')
  return await response.json() as WorkspaceFileContentResponse
}

export async function saveWorkspaceFileContent(
  workspaceId: string,
  path: string,
  content: string,
  baseRevision: string,
  sessionToken: string,
) {
  const response = await cloudFetch(
    `/api/cloud/workspaces/${encodeURIComponent(workspaceId)}/files/content`,
    {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path, content, baseRevision }),
    },
    sessionToken,
  )
  if (!response.ok) throw await responseError(response, 'Saving file')
  return await response.json() as SaveWorkspaceFileResponse
}

export async function loadWorkspaceRevisionDiff(
  workspaceId: string,
  baseRevisionId: string,
  sessionToken: string,
  headRevisionId?: string,
) {
  const params = new URLSearchParams({ baseRevisionId })
  if (headRevisionId) params.set('headRevisionId', headRevisionId)
  const response = await cloudFetch(
    `/api/cloud/workspaces/${encodeURIComponent(workspaceId)}/diff?${params}`,
    {},
    sessionToken,
  )
  if (!response.ok) throw await responseError(response, 'Loading workspace changes')
  return await response.json() as WorkspaceRevisionDiffResponse
}

export async function analyzeWorkspace(workspaceId: string, sessionToken: string) {
  const response = await cloudFetch(
    `/api/cloud/workspaces/${encodeURIComponent(workspaceId)}/analyze`,
    { method: 'POST' },
    sessionToken,
  )
  if (!response.ok) throw await responseError(response, 'Starting analysis')
  return await response.json() as CloudAnalyzeResponse
}
