import type { EdgeType, GraphEdge, GraphNode, SourceReachability } from '../types'
import type { AggregatedProjectEdge, ProjectGroup, ProjectGroupKind, ProjectMapModel } from '../views/architecture/architectureTypes'

type GroupByMode = 'top-level' | 'directory' | 'language' | 'module'

interface ProjectMapOptions {
  groupBy?: GroupByMode
  includeTests?: boolean
  includeExternal?: boolean
  includeGenerated?: boolean
}

const TOP_LEVEL_ORDER: ProjectGroupKind[] = ['frontend', 'backend', 'shared', 'tests', 'external', 'generated', 'unknown']

const GROUP_LABELS: Partial<Record<ProjectGroupKind, string>> = {
  frontend: 'Frontend',
  backend: 'Backend',
  shared: 'Shared',
  tests: 'Tests',
  external: 'External',
  generated: 'Generated',
  unknown: 'Other',
}

export function buildProjectMapModel(
  nodes: GraphNode[],
  edges: GraphEdge[],
  options: ProjectMapOptions = {},
): ProjectMapModel {
  const includeTests = options.includeTests ?? true
  const includeExternal = options.includeExternal ?? true
  const includeGenerated = options.includeGenerated ?? true
  const nodeToGroup = new Map<string, string>()
  const groupsById = new Map<string, ProjectGroup>()
  const childGroupsById = new Map<string, ProjectGroup>()

  for (const node of nodes) {
    const kind = classifyNode(node)
    if (!includeTests && kind === 'tests') continue
    if (!includeExternal && kind === 'external') continue
    if (!includeGenerated && kind === 'generated') continue

    const groupId = groupIdFor(node, kind, options.groupBy ?? 'top-level')
    const group = ensureGroup(groupsById, groupId, kind, topLevelLabel(groupId, kind), pathPrefixFor(node, groupId), node.language)
    addNodeStats(group, node)
    nodeToGroup.set(node.id, group.id)

    const childId = subgroupIdFor(node, kind)
    if (childId && childId !== group.id) {
      const child = ensureGroup(childGroupsById, childId, 'directory', childId.split('/').slice(-1)[0], childId, node.language)
      addNodeStats(child, node)
      const existingChildren = group.children ?? []
      if (!existingChildren.some(candidate => candidate.id === child.id)) {
        group.children = [...existingChildren, child]
      }
    }
  }

  const edgeBuckets = new Map<string, AggregatedProjectEdge>()
  for (const edge of edges) {
    if (edge.type === 'Contains' || edge.type === 'ModDeclaration') continue
    const sourceGroupId = nodeToGroup.get(edge.source)
    const targetGroupId = nodeToGroup.get(edge.target)
    if (!sourceGroupId || !targetGroupId || sourceGroupId === targetGroupId) continue
    const key = `${sourceGroupId}->${targetGroupId}`
    const bucket = edgeBuckets.get(key) ?? {
      id: key,
      sourceGroupId,
      targetGroupId,
      count: 0,
      edgeTypes: {},
      underlyingEdgeIds: [],
    }
    const underlyingEdgeIds = edge.bundledEdgeIds?.length ? edge.bundledEdgeIds : [edge.id]
    const count = edge.bundledCount ?? underlyingEdgeIds.length
    bucket.count += Math.max(1, count)
    bucket.underlyingEdgeIds.push(...underlyingEdgeIds)
    addEdgeTypeCounts(bucket.edgeTypes, edge)
    edgeBuckets.set(key, bucket)
  }

  for (const edge of edgeBuckets.values()) {
    groupsById.get(edge.sourceGroupId)!.outgoingCount += edge.count
    groupsById.get(edge.targetGroupId)!.incomingCount += edge.count
  }

  const groups = [...groupsById.values()]
    .map(group => ({
      ...group,
      children: group.children?.sort((a, b) => b.nodeIds.length - a.nodeIds.length || a.label.localeCompare(b.label)),
    }))
    .sort(compareGroups)

  return {
    groups,
    edges: [...edgeBuckets.values()].sort((a, b) => b.count - a.count || a.id.localeCompare(b.id)),
    nodeToGroup,
  }
}

export function classifyNode(node: GraphNode): ProjectGroupKind {
  const file = normalizePath(node.file)
  const moduleName = (node.module ?? '').toLowerCase()
  const label = node.label.toLowerCase()
  const reachability: SourceReachability | undefined = node.reachability

  if (node.type === 'ExternalCrate' || reachability === 'External') return 'external'
  if (
    file.includes('/test/')
    || file.includes('/tests/')
    || file.endsWith('.test.ts')
    || file.endsWith('.spec.ts')
    || file.endsWith('_test.rs')
    || label.includes('test')
  ) return 'tests'
  if (
    file.includes('/generated/')
    || file.includes('/mocks/')
    || file.includes('/mock')
    || reachability === 'Generated'
  ) return 'generated'
  if (
    file.includes('/shared/')
    || file.startsWith('shared/')
    || file.includes('/common/')
    || file.startsWith('common/')
    || file.includes('/protocol/')
    || file.startsWith('protocol/')
    || file.includes('/types/')
    || moduleName.includes('shared')
    || moduleName.includes('protocol')
  ) return 'shared'
  if (
    file.startsWith('frontend/')
    || file.includes('/frontend/')
    || file.includes('/src/app/')
    || file.endsWith('.tsx')
    || file.endsWith('.ts')
    || node.language === 'typescript'
    || node.language === 'qml'
  ) return 'frontend'
  if (
    file.startsWith('backend/')
    || file.includes('/backend/')
    || file.startsWith('crates/')
    || file.endsWith('.rs')
    || file.endsWith('.py')
    || node.language === 'rust'
    || node.language === 'python'
  ) return 'backend'
  return 'unknown'
}

function groupIdFor(node: GraphNode, kind: ProjectGroupKind, groupBy: GroupByMode) {
  if (groupBy === 'language') return node.language ? `language:${node.language}` : kind
  if (groupBy === 'module' && node.module) return sanitizeGroupId(node.module)
  if (groupBy === 'directory') return directoryGroupId(node, kind)
  return kind
}

function directoryGroupId(node: GraphNode, kind: ProjectGroupKind) {
  const file = normalizePath(node.file)
  if (!file) return kind
  const parts = file.split('/').filter(Boolean)
  if (parts.length <= 1) return kind
  if (parts[0] === 'frontend' || parts[0] === 'backend' || parts[0] === 'crates') return parts.slice(0, 2).join('/')
  return `${kind}/${parts[0]}`
}

function subgroupIdFor(node: GraphNode, kind: ProjectGroupKind) {
  const file = normalizePath(node.file)
  if (!file) return null
  const parts = file.split('/').filter(Boolean)
  if (!parts.length) return null
  if (kind === 'frontend') {
    const index = Math.max(parts.indexOf('src'), parts.indexOf('app'))
    const section = parts[index + 1] ?? parts[1]
    return section ? `frontend/${section}` : null
  }
  if (kind === 'backend') {
    if (parts[0] === 'crates' && parts[1]) return `backend/${parts[1]}`
    const section = parts.includes('src') ? parts[parts.indexOf('src') + 1] : parts[1]
    return section ? `backend/${section}` : null
  }
  if (kind === 'shared') return `shared/${parts[parts.length > 1 ? parts.length - 2 : 0]}`
  if (kind === 'tests') return file.includes('/integration') ? 'tests/integration' : 'tests/unit'
  if (kind === 'external') return `external/${node.crate ?? node.label.split('::')[0] ?? 'packages'}`
  return null
}

function ensureGroup(
  groupsById: Map<string, ProjectGroup>,
  id: string,
  kind: ProjectGroupKind,
  label: string,
  pathPrefix?: string,
  language?: string,
) {
  const existing = groupsById.get(id)
  if (existing) return existing
  const group: ProjectGroup = {
    id,
    label,
    kind,
    pathPrefix,
    language,
    nodeIds: [],
    fileCount: 0,
    symbolCount: 0,
    incomingCount: 0,
    outgoingCount: 0,
  }
  groupsById.set(id, group)
  return group
}

function addNodeStats(group: ProjectGroup, node: GraphNode) {
  group.nodeIds.push(node.id)
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

function topLevelLabel(groupId: string, kind: ProjectGroupKind) {
  if (groupId.startsWith('language:')) return groupId.replace('language:', '')
  return GROUP_LABELS[kind] ?? groupId.split('/').slice(-1)[0] ?? groupId
}

function pathPrefixFor(node: GraphNode, fallback: string) {
  const file = normalizePath(node.file)
  if (!file) return fallback
  const parts = file.split('/').filter(Boolean)
  return parts.slice(0, Math.min(2, parts.length)).join('/')
}

function compareGroups(a: ProjectGroup, b: ProjectGroup) {
  const ai = TOP_LEVEL_ORDER.indexOf(a.kind)
  const bi = TOP_LEVEL_ORDER.indexOf(b.kind)
  const orderA = ai === -1 ? TOP_LEVEL_ORDER.length : ai
  const orderB = bi === -1 ? TOP_LEVEL_ORDER.length : bi
  return orderA - orderB || b.nodeIds.length - a.nodeIds.length || a.label.localeCompare(b.label)
}

function normalizePath(path?: string | null) {
  return (path ?? '').replaceAll('\\', '/').replace(/^\/+/, '')
}

function sanitizeGroupId(value: string) {
  return value.replaceAll('::', '/').replaceAll('.', '/').replace(/[^a-zA-Z0-9_/-]/g, '-')
}
