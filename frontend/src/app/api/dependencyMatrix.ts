import type { EdgeType, GraphEdge, GraphNode } from '../types'
import { classifyNode } from './graphAggregation'
import type {
  DependencyMatrixBadge,
  DependencyMatrixCell,
  DependencyMatrixLevel,
  ProjectGroup,
  ProjectGroupKind,
} from '../views/architecture/architectureTypes'
import type { DependencyMatrixModel } from '../views/architecture/architectureTypes'

export interface DependencyMatrixOptions {
  level?: DependencyMatrixLevel
  includeTests?: boolean
  includeExternal?: boolean
  includeGenerated?: boolean
  includeTypeRefs?: boolean
  expandAllFiles?: boolean
}

interface GroupDraft extends ProjectGroup {
  files: Set<string>
}

interface MatrixGroupTarget {
  id: string
  label: string
  kind: ProjectGroupKind
  pathPrefix?: string
}

const AREA_ORDER: ProjectGroupKind[] = ['frontend', 'backend', 'shared', 'tests', 'external', 'generated', 'unknown']
const AREA_LABELS: Partial<Record<ProjectGroupKind, string>> = {
  frontend: 'Frontend',
  backend: 'Backend',
  shared: 'Shared',
  tests: 'Tests',
  external: 'External',
  generated: 'Generated',
  unknown: 'Other',
}

const STRUCTURAL_EDGE_TYPES = new Set<EdgeType>(['Contains', 'ModDeclaration'])
const FILE_LEVEL_LIMIT = 80

export function defaultDependencyMatrixLevel(nodes: GraphNode[]): DependencyMatrixLevel {
  const visibleFiles = uniqueProjectFiles(nodes)
  const topLevelGroups = new Set(nodes.map(node => classifyNode(node))).size
  if (topLevelGroups <= 3 && visibleFiles.size <= FILE_LEVEL_LIMIT) return 'file'
  if (topLevelGroups <= 4) return 'module'
  return 'area'
}

export function buildDependencyMatrixModel(
  nodes: GraphNode[],
  edges: GraphEdge[],
  options: DependencyMatrixOptions = {},
): DependencyMatrixModel {
  const level = options.level ?? defaultDependencyMatrixLevel(nodes)
  const includeTests = options.includeTests ?? true
  const includeExternal = options.includeExternal ?? true
  const includeGenerated = options.includeGenerated ?? true
  const includeTypeRefs = options.includeTypeRefs ?? true
  const nodeById = new Map(nodes.map(node => [node.id, node]))
  const nodeToGroup = new Map<string, string>()
  const groupsById = new Map<string, GroupDraft>()

  for (const node of nodes) {
    const kind = classifyNode(node)
    if (!includeTests && kind === 'tests') continue
    if (!includeExternal && kind === 'external') continue
    if (!includeGenerated && kind === 'generated') continue
    const target = groupTargetFor(node, kind, level)
    const group = ensureGroup(groupsById, target, node.language)
    addNodeStats(group, node)
    nodeToGroup.set(node.id, group.id)
  }

  const initialCells = collectCells(edges, nodeById, nodeToGroup, includeTypeRefs)
  const allowedGroupIds = selectVisibleGroups(groupsById, initialCells, level, options.expandAllFiles ?? false)
  const groups = finalizeGroups(groupsById, allowedGroupIds)
  const cells = finalizeCells(initialCells, groupsById, allowedGroupIds)

  const suggestedLevel = level === 'area' && groups.length <= 3 && cells.length <= 1 ? 'file' : undefined

  return {
    groups,
    cells,
    level,
    totalGroups: groupsById.size,
    truncated: allowedGroupIds.size < groupsById.size,
    suggestedLevel,
  }
}

function collectCells(
  edges: GraphEdge[],
  nodeById: Map<string, GraphNode>,
  nodeToGroup: Map<string, string>,
  includeTypeRefs: boolean,
) {
  const cells = new Map<string, DependencyMatrixCell>()

  for (const edge of edges) {
    if (STRUCTURAL_EDGE_TYPES.has(edge.type)) continue
    if (!includeTypeRefs && edge.type === 'TypeReference') continue
    const sourceGroupId = nodeToGroup.get(edge.source)
    const targetGroupId = nodeToGroup.get(edge.target)
    if (!sourceGroupId || !targetGroupId || sourceGroupId === targetGroupId) continue
    const source = nodeById.get(edge.source)
    const target = nodeById.get(edge.target)
    const key = `${sourceGroupId}->${targetGroupId}`
    const bucket = cells.get(key) ?? {
      sourceGroupId,
      targetGroupId,
      count: 0,
      edgeTypes: {},
      underlyingEdgeIds: [],
      files: [],
      examples: [],
      badges: [],
    }
    const underlyingEdgeIds = edge.bundledEdgeIds?.length ? edge.bundledEdgeIds : [edge.id]
    const count = Math.max(1, edge.bundledCount ?? underlyingEdgeIds.length)
    bucket.count += count
    bucket.underlyingEdgeIds.push(...underlyingEdgeIds)
    addEdgeTypeCounts(bucket.edgeTypes, edge)
    addUniqueFile(bucket.files, source?.file)
    addUniqueFile(bucket.files, target?.file)
    if (bucket.examples.length < 8) {
      bucket.examples.push({
        id: edge.id,
        sourceLabel: source?.label ?? edge.source,
        targetLabel: target?.label ?? edge.target,
        type: edge.type,
        sourceFile: normalizePath(source?.file),
        targetFile: normalizePath(target?.file),
      })
    }
    cells.set(key, bucket)
  }

  return cells
}

function finalizeCells(
  cellsByKey: Map<string, DependencyMatrixCell>,
  groupsById: Map<string, GroupDraft>,
  allowedGroupIds: Set<string>,
) {
  const cells = [...cellsByKey.values()]
    .filter(cell => allowedGroupIds.has(cell.sourceGroupId) && allowedGroupIds.has(cell.targetGroupId))
    .map(cell => ({
      ...cell,
      files: [...new Set(cell.files)].sort(),
      underlyingEdgeIds: [...new Set(cell.underlyingEdgeIds)],
      badges: badgesForCell(cell, cellsByKey, groupsById),
    }))

  for (const cell of cells) {
    const source = groupsById.get(cell.sourceGroupId)
    const target = groupsById.get(cell.targetGroupId)
    if (source) source.outgoingCount += cell.count
    if (target) target.incomingCount += cell.count
  }

  return cells.sort((a, b) => b.count - a.count || cellKey(a).localeCompare(cellKey(b)))
}

function badgesForCell(
  cell: DependencyMatrixCell,
  cellsByKey: Map<string, DependencyMatrixCell>,
  groupsById: Map<string, GroupDraft>,
): DependencyMatrixBadge[] {
  const badges = new Set<DependencyMatrixBadge>()
  const reverseKey = `${cell.targetGroupId}->${cell.sourceGroupId}`
  const source = groupsById.get(cell.sourceGroupId)
  const target = groupsById.get(cell.targetGroupId)
  if (cell.count >= 20) badges.add('strong')
  if (cellsByKey.has(reverseKey)) badges.add('cycle')
  if (isExternalCell(cell, source, target)) badges.add('external')
  if (isTypeOnlyCell(cell)) badges.add('type-only')
  if (groupArea(source) === 'backend' && groupArea(target) === 'frontend') badges.add('violation')
  if (isUnexpectedDependency(cell, source, target)) badges.add('unexpected')
  return [...badges]
}

function isExternalCell(cell: DependencyMatrixCell, source?: ProjectGroup, target?: ProjectGroup) {
  return groupArea(source) === 'external' || groupArea(target) === 'external' || Boolean(cell.edgeTypes.ExternalDependency)
}

function isTypeOnlyCell(cell: DependencyMatrixCell) {
  const types = Object.keys(cell.edgeTypes)
  return types.length > 0 && types.every(type => type === 'TypeReference')
}

function isUnexpectedDependency(cell: DependencyMatrixCell, source?: ProjectGroup, target?: ProjectGroup) {
  const edgeTypes = new Set(Object.keys(cell.edgeTypes))
  const isApiBoundary = edgeTypes.has('ApiCall') || edgeTypes.has('EndpointHandler')
  const sourceArea = groupArea(source)
  const targetArea = groupArea(target)
  if (sourceArea === 'backend' && targetArea === 'frontend') return true
  if (sourceArea === 'frontend' && targetArea === 'backend' && !isApiBoundary) return true
  if (looksLikeModel(source) && looksLikeRoute(target)) return true
  if (sourceArea === 'shared' && isFeatureSpecific(target)) return true
  return false
}

function looksLikeModel(group?: ProjectGroup) {
  const value = `${group?.id ?? ''}/${group?.pathPrefix ?? ''}`.toLowerCase()
  return value.includes('model')
}

function looksLikeRoute(group?: ProjectGroup) {
  const value = `${group?.id ?? ''}/${group?.pathPrefix ?? ''}`.toLowerCase()
  return value.includes('route') || value.includes('controller') || value.includes('handler')
}

function isFeatureSpecific(group?: ProjectGroup) {
  if (!group) return false
  const area = groupArea(group)
  if (area === 'shared' || area === 'external' || area === 'tests') return false
  return /\/(feature|features|pages|routes|screens|app)\//i.test(`/${group.id}/${group.pathPrefix ?? ''}/`)
}

function selectVisibleGroups(
  groupsById: Map<string, GroupDraft>,
  cells: Map<string, DependencyMatrixCell>,
  level: DependencyMatrixLevel,
  expandAllFiles: boolean,
) {
  const groupIds = new Set(groupsById.keys())
  if (level !== 'file' || expandAllFiles || groupIds.size <= FILE_LEVEL_LIMIT) return groupIds

  const degreeByGroup = new Map<string, number>()
  for (const groupId of groupIds) {
    degreeByGroup.set(groupId, groupsById.get(groupId)?.nodeIds.length ?? 0)
  }
  for (const cell of cells.values()) {
    degreeByGroup.set(cell.sourceGroupId, (degreeByGroup.get(cell.sourceGroupId) ?? 0) + cell.count)
    degreeByGroup.set(cell.targetGroupId, (degreeByGroup.get(cell.targetGroupId) ?? 0) + cell.count)
  }

  return new Set(
    [...groupIds]
      .sort((a, b) => (degreeByGroup.get(b) ?? 0) - (degreeByGroup.get(a) ?? 0) || a.localeCompare(b))
      .slice(0, FILE_LEVEL_LIMIT),
  )
}

function finalizeGroups(groupsById: Map<string, GroupDraft>, allowedGroupIds: Set<string>): ProjectGroup[] {
  return [...groupsById.values()]
    .filter(group => allowedGroupIds.has(group.id))
    .map(group => {
      const { files: _files, ...publicGroup } = group
      return publicGroup
    })
    .sort(compareGroups)
}

function groupTargetFor(node: GraphNode, kind: ProjectGroupKind, level: DependencyMatrixLevel): MatrixGroupTarget {
  const file = normalizePath(node.file)
  if (level === 'area') {
    return { id: kind, label: AREA_LABELS[kind] ?? 'Other', kind, pathPrefix: kind }
  }
  if (level === 'file') {
    const label = file || node.label
    return { id: `file:${label}`, label, kind: fileKind(kind), pathPrefix: file }
  }
  if (level === 'directory') {
    const directory = directoryFor(file, kind)
    return { id: `dir:${directory}`, label: directory, kind: 'directory', pathPrefix: directory }
  }
  const module = moduleFor(node, kind)
  return { id: `module:${module}`, label: module, kind: 'module', pathPrefix: module }
}

function fileKind(kind: ProjectGroupKind): ProjectGroupKind {
  if (kind === 'frontend' || kind === 'backend' || kind === 'shared' || kind === 'external' || kind === 'tests' || kind === 'generated') return kind
  return 'directory'
}

function directoryFor(file: string, kind: ProjectGroupKind) {
  if (!file) return AREA_LABELS[kind] ?? 'Other'
  const parts = file.split('/').filter(Boolean)
  if (parts.length <= 1) return parts[0] ?? (AREA_LABELS[kind] ?? 'Other')
  return parts.slice(0, -1).join('/')
}

function moduleFor(node: GraphNode, kind: ProjectGroupKind) {
  const file = normalizePath(node.file)
  const parts = file.split('/').filter(Boolean)
  if (parts[0] === 'frontend' && parts[1] === 'src' && parts[2]) return `frontend/${stripExtension(parts[2])}`
  if (parts[0] === 'qml' && parts[1]) return `qml/${stripExtension(parts[1])}`
  if (parts[0] === 'backend' && parts[1]) return `backend/${stripExtension(parts[1])}`
  if (parts[0] === 'src') return 'example/src'
  if (node.module) return sanitizeGroupId(node.module)
  if (parts.length > 1) return `${parts[0]}/${stripExtension(parts[1])}`
  return AREA_LABELS[kind] ?? 'Other'
}

function ensureGroup(
  groupsById: Map<string, GroupDraft>,
  target: MatrixGroupTarget,
  language?: string,
) {
  const existing = groupsById.get(target.id)
  if (existing) return existing
  const group: GroupDraft = {
    id: target.id,
    label: target.label,
    kind: target.kind,
    pathPrefix: target.pathPrefix,
    language,
    nodeIds: [],
    fileCount: 0,
    symbolCount: 0,
    incomingCount: 0,
    outgoingCount: 0,
    files: new Set(),
  }
  groupsById.set(group.id, group)
  return group
}

function addNodeStats(group: GroupDraft, node: GraphNode) {
  group.nodeIds.push(node.id)
  const file = normalizePath(node.file)
  if (file) group.files.add(file)
  if (node.type === 'File') group.fileCount += 1
  else group.symbolCount += 1
}

function addEdgeTypeCounts(edgeTypes: Partial<Record<EdgeType, number>>, edge: GraphEdge) {
  if (edge.bundledTypes?.length) {
    for (const type of edge.bundledTypes) {
      edgeTypes[type] = (edgeTypes[type] ?? 0) + 1
    }
    return
  }
  edgeTypes[edge.type] = (edgeTypes[edge.type] ?? 0) + Math.max(1, edge.bundledCount ?? 1)
}

function compareGroups(a: ProjectGroup, b: ProjectGroup) {
  const orderA = AREA_ORDER.indexOf(groupArea(a))
  const orderB = AREA_ORDER.indexOf(groupArea(b))
  const normalizedA = orderA === -1 ? AREA_ORDER.length : orderA
  const normalizedB = orderB === -1 ? AREA_ORDER.length : orderB
  return normalizedA - normalizedB || b.outgoingCount + b.incomingCount - (a.outgoingCount + a.incomingCount) || a.label.localeCompare(b.label)
}

function groupArea(group?: ProjectGroup): ProjectGroupKind {
  if (!group) return 'unknown'
  if (AREA_ORDER.includes(group.kind)) return group.kind
  const value = `${group.id}/${group.label}/${group.pathPrefix ?? ''}`.toLowerCase()
  if (value.includes('/test/') || value.includes('/tests/') || value.endsWith('.test.ts') || value.endsWith('.spec.ts')) return 'tests'
  if (value.includes('/generated/') || value.includes('/mock')) return 'generated'
  if (value.includes('/external/') || value.includes('external') || value.includes('crate:')) return 'external'
  if (value.includes('/shared/') || value.includes('/common/') || value.includes('/protocol/') || value.includes('/types/')) return 'shared'
  if (value.includes('/frontend/') || value.includes('/qml/') || value.endsWith('.tsx') || value.endsWith('.ts') || value.endsWith('.qml')) return 'frontend'
  if (value.includes('/backend/') || value.includes('/crates/') || value.includes('/src/') || value.endsWith('.rs') || value.endsWith('.py')) return 'backend'
  return 'unknown'
}

function addUniqueFile(files: string[], file?: string | null) {
  const normalized = normalizePath(file)
  if (normalized && !files.includes(normalized)) files.push(normalized)
}

function uniqueProjectFiles(nodes: GraphNode[]) {
  const files = new Set<string>()
  for (const node of nodes) {
    const file = normalizePath(node.file)
    if (file) files.add(file)
  }
  return files
}

function cellKey(cell: DependencyMatrixCell) {
  return `${cell.sourceGroupId}->${cell.targetGroupId}`
}

function normalizePath(path?: string | null) {
  return (path ?? '').replaceAll('\\', '/').replace(/^\/+/, '')
}

function stripExtension(pathPart: string) {
  return pathPart.replace(/\.[^.]+$/, '')
}

function sanitizeGroupId(value: string) {
  return value.replaceAll('::', '/').replaceAll('.', '/').replace(/[^a-zA-Z0-9_/-]/g, '-')
}
