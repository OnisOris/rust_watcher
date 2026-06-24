import type { GraphEdge, GraphFilters, GraphMode, GraphNode, NodeType, ThemeMode } from '../types'
import { inferNodeLanguage, languageColor, languageIcon } from './language'

const NODE_COLORS: Record<NodeType, string> = {
  File: '#3B82F6',
  Module: '#8B5CF6',
  Struct: '#06B6D4',
  Class: '#0EA5E9',
  Object: '#38BDF8',
  Enum: '#F59E0B',
  Trait: '#10B981',
  Impl: '#6366F1',
  Function: '#EC4899',
  Method: '#F97316',
  Component: '#14B8A6',
  Hook: '#A855F7',
  Interface: '#22C55E',
  TypeAlias: '#84CC16',
  Property: '#FACC15',
  Signal: '#FB7185',
  Handler: '#F472B6',
  Endpoint: '#E11D48',
  Macro: '#EF4444',
  ExternalCrate: '#7D8795',
}

const EDGE_COLORS: Record<GraphEdge['type'], string> = {
  Contains: '#475569',
  Imports: '#64748B',
  Uses: '#4B5870',
  Calls: '#06B6D4',
  Renders: '#14B8A6',
  ApiCall: '#E11D48',
  EndpointHandler: '#F97316',
  Implements: '#10B981',
  TypeReference: '#3B82F6',
  DataFlow: '#8B5CF6',
  ModDeclaration: '#6366F1',
  ExternalDependency: '#64748B',
}

const NODE_SIZES: Record<NodeType, number> = {
  Module: 26,
  ExternalCrate: 18,
  File: 18,
  Struct: 18,
  Class: 18,
  Object: 17,
  Enum: 18,
  Trait: 18,
  Impl: 10,
  Function: 14,
  Method: 12,
  Component: 18,
  Hook: 13,
  Interface: 16,
  TypeAlias: 15,
  Property: 10,
  Signal: 11,
  Handler: 12,
  Endpoint: 16,
  Macro: 12,
}

const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5))

type ExportTheme = {
  bg: string
  bg2: string
  card: string
  border: string
  text: string
  textMuted: string
  grid: string
}

type SvgExportOptions = {
  nodes: GraphNode[]
  edges: GraphEdge[]
  filters: GraphFilters
  selectedNodeId: string | null
  graphMode: GraphMode
  theme: ThemeMode
}

function exportTheme(mode: ThemeMode): ExportTheme {
  if (mode === 'dark') {
    return {
      bg: '#0b1020',
      bg2: '#141b2f',
      card: '#101827',
      border: '#334155',
      text: '#e5edf8',
      textMuted: '#9fb0c3',
      grid: 'rgba(148,163,184,0.20)',
    }
  }
  return {
    bg: '#eef4fb',
    bg2: '#f8fbff',
    card: '#ffffff',
    border: '#b7c6d8',
    text: '#172033',
    textMuted: '#52647a',
    grid: 'rgba(30,64,112,0.14)',
  }
}

function nodeRadius(node: GraphNode) {
  return NODE_SIZES[node.type] ?? 14
}

function edgeColor(edge: GraphEdge) {
  return EDGE_COLORS[edge.type] ?? '#64748B'
}

function edgeWidth(edge: GraphEdge) {
  if (edge.type === 'DataFlow' || edge.type === 'ApiCall' || edge.type === 'EndpointHandler') return 2.4
  if (edge.type === 'Calls' || edge.type === 'Renders') return 1.7
  return 1.1
}

function escapeXml(value: string) {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&apos;')
}

function stablePoint(id: string, index: number, count: number) {
  let hash = 0
  for (let i = 0; i < id.length; i++) hash = (hash * 31 + id.charCodeAt(i)) | 0
  const angle = index * GOLDEN_ANGLE + (Math.abs(hash) % 97) / 97
  const radius = Math.max(120, Math.sqrt(Math.max(1, count)) * 52) * Math.sqrt((index + 1) / Math.max(1, count))
  return { x: Math.cos(angle) * radius, y: Math.sin(angle) * radius }
}

function needsFreshLayout(nodes: GraphNode[]) {
  if (nodes.some(node => !Number.isFinite(node.x) || !Number.isFinite(node.y))) return true
  if (nodes.length < 3) return false
  const uniquePositions = new Set(nodes.map(node => `${Math.round(node.x)}:${Math.round(node.y)}`))
  return uniquePositions.size < Math.min(3, nodes.length)
}

function withExportLayout(nodes: GraphNode[]) {
  if (!needsFreshLayout(nodes)) return nodes
  return nodes.map((node, index) => {
    const point = stablePoint(node.id, index, nodes.length)
    return { ...node, x: point.x, y: point.y }
  })
}

function activeGraph(nodes: GraphNode[], edges: GraphEdge[], filters: GraphFilters) {
  const activeNodes = withExportLayout(nodes).filter(node => filters.nodeTypes.has(node.type))
  const activeIds = new Set(activeNodes.map(node => node.id))
  const activeEdges = edges.filter(edge => filters.edgeTypes.has(edge.type) && activeIds.has(edge.source) && activeIds.has(edge.target))
  return { activeNodes, activeEdges, activeIds }
}

function buildDegreeMap(nodes: GraphNode[], edges: GraphEdge[]) {
  const degree = new Map(nodes.map(node => [node.id, 0]))
  for (const edge of edges) {
    if (degree.has(edge.source)) degree.set(edge.source, (degree.get(edge.source) ?? 0) + (edge.bundledCount ?? 1))
    if (degree.has(edge.target)) degree.set(edge.target, (degree.get(edge.target) ?? 0) + (edge.bundledCount ?? 1))
  }
  return degree
}

function graphBounds(nodes: GraphNode[]) {
  if (nodes.length === 0) return { minX: -450, minY: -260, width: 900, height: 520 }
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity
  for (const node of nodes) {
    const size = nodeRadius(node)
    const labelPad = Math.min(170, Math.max(50, node.label.length * 7))
    minX = Math.min(minX, node.x - size - labelPad)
    maxX = Math.max(maxX, node.x + size + labelPad)
    minY = Math.min(minY, node.y - size - 32)
    maxY = Math.max(maxY, node.y + size + 48)
  }
  return { minX, minY, width: Math.max(1, maxX - minX), height: Math.max(1, maxY - minY) }
}

function shortPath(path: string) {
  const parts = path.split('/').filter(Boolean)
  if (parts.length <= 2) return path
  return `${parts[parts.length - 2]}/${parts[parts.length - 1]}`
}

function fitLabel(text: string, maxChars: number) {
  return text.length <= maxChars ? text : `${text.slice(0, Math.max(1, maxChars - 1))}…`
}

function shouldExportLabel(node: GraphNode, degree: number, selected: boolean, count: number) {
  if (selected || node.pinned) return true
  if (count <= 140) return true
  return degree >= 4 || node.type === 'Module' || node.type === 'File' || node.type === 'Endpoint' || node.type === 'Struct' || node.type === 'Trait'
}

function svgNodeShape(node: GraphNode, fill: string, stroke: string, selected: boolean) {
  const size = nodeRadius(node)
  const strokeWidth = selected ? 3 : 1.7
  if (node.type === 'File') {
    const w = size * 1.55
    const h = size * 1.95
    return `<rect x="${node.x - w / 2}" y="${node.y - h / 2}" width="${w}" height="${h}" rx="4" fill="${fill}" stroke="${stroke}" stroke-width="${strokeWidth}"/>`
  }
  if (node.type === 'Module' || node.type === 'Struct' || node.type === 'Class' || node.type === 'Object' || node.type === 'Interface' || node.type === 'TypeAlias') {
    return `<rect x="${node.x - size}" y="${node.y - size * 0.68}" width="${size * 2}" height="${size * 1.36}" rx="${node.type === 'Module' ? 8 : 5}" fill="${fill}" stroke="${stroke}" stroke-width="${strokeWidth}"/>`
  }
  if (node.type === 'Endpoint') {
    return `<rect x="${node.x - size * 1.35}" y="${node.y - size * 0.65}" width="${size * 2.7}" height="${size * 1.3}" rx="9" fill="${fill}" stroke="${stroke}" stroke-width="${strokeWidth}"/>`
  }
  if (node.type === 'Enum') {
    const points = `${node.x},${node.y - size} ${node.x + size * 0.76},${node.y} ${node.x},${node.y + size} ${node.x - size * 0.76},${node.y}`
    return `<polygon points="${points}" fill="${fill}" stroke="${stroke}" stroke-width="${strokeWidth}"/>`
  }
  return `<circle cx="${node.x}" cy="${node.y}" r="${size}" fill="${fill}" stroke="${stroke}" stroke-width="${strokeWidth}"/>`
}

function renderGrid(minX: number, minY: number, width: number, height: number, color: string) {
  const spacing = 40
  const x0 = Math.floor(minX / spacing) * spacing
  const y0 = Math.floor(minY / spacing) * spacing
  const x1 = minX + width
  const y1 = minY + height
  const dots: string[] = []
  for (let x = x0; x <= x1; x += spacing) {
    for (let y = y0; y <= y1; y += spacing) {
      dots.push(`<circle cx="${x}" cy="${y}" r="1" fill="${color}"/>`)
    }
  }
  return dots.join('\n')
}

function renderEdge(edge: GraphEdge, nodeById: Map<string, GraphNode>) {
  const src = nodeById.get(edge.source)
  const tgt = nodeById.get(edge.target)
  if (!src || !tgt) return ''
  const dx = tgt.x - src.x
  const dy = tgt.y - src.y
  const dist = Math.sqrt(dx * dx + dy * dy) || 1
  const ux = dx / dist
  const uy = dy / dist
  const x1 = src.x + ux * (nodeRadius(src) + 3)
  const y1 = src.y + uy * (nodeRadius(src) + 3)
  const x2 = tgt.x - ux * (nodeRadius(tgt) + 6)
  const y2 = tgt.y - uy * (nodeRadius(tgt) + 6)
  const bundled = (edge.bundledCount ?? 1) > 1
    ? `<text x="${(src.x + tgt.x) / 2}" y="${(src.y + tgt.y) / 2 - 5}" class="edge-count">${edge.bundledCount}</text>`
    : ''
  return `
    <g>
      <line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${edgeColor(edge)}" stroke-width="${edgeWidth(edge)}" stroke-opacity="0.62" marker-end="url(#arrow)"/>
      ${bundled}
      <title>${escapeXml(edge.label ?? edge.type)}</title>
    </g>`
}

function renderNode(node: GraphNode, degree: number, selected: boolean, colors: ExportTheme, count: number) {
  const language = inferNodeLanguage(node)
  const languageBadgeColor = languageColor(language)
  const typeColor = NODE_COLORS[node.type] ?? '#7D8795'
  const color = language === 'external' ? typeColor : languageBadgeColor
  const size = nodeRadius(node)
  const icon = languageIcon(language)
  const badgeW = Math.max(18, icon.length * 6.5 + 9)
  const badgeX = node.x + size * 0.35
  const badgeY = node.y - size - 14
  const labelLines = [fitLabel(node.label, selected ? 34 : 24)]
  if ((selected || node.type === 'Endpoint' || node.type === 'File') && node.file && node.file !== node.label) {
    labelLines.push(fitLabel(shortPath(node.file), selected ? 34 : 24))
  }
  const label = shouldExportLabel(node, degree, selected, count)
    ? labelLines.map((line, index) => `<text x="${node.x}" y="${node.y + size + 17 + index * 13}" class="node-label ${selected ? 'selected' : ''}">${escapeXml(line)}</text>`).join('\n')
    : ''

  return `
    <g class="node ${selected ? 'selected' : ''}" opacity="${node.reachability === 'Detached' ? 0.62 : 1}">
      ${svgNodeShape(node, colors.card, selected ? colors.text : color, selected)}
      <circle cx="${node.x}" cy="${node.y}" r="${Math.max(3, size * 0.23)}" fill="${color}" opacity="0.82"/>
      <rect x="${badgeX}" y="${badgeY}" width="${badgeW}" height="16" rx="8" fill="${colors.card}" stroke="${color}" stroke-width="1.2"/>
      <text x="${badgeX + badgeW / 2}" y="${badgeY + 11}" class="language-badge" fill="${color}">${escapeXml(icon)}</text>
      ${node.pinned ? `<text x="${node.x}" y="${node.y + 4}" class="pin">P</text>` : ''}
      ${label}
      <title>${escapeXml([node.type, node.label, node.file].filter(Boolean).join(' · '))}</title>
    </g>`
}

function sanitizeFilePart(value: string) {
  return value.toLowerCase().replace(/[^a-z0-9_-]+/g, '-').replace(/^-+|-+$/g, '') || 'graph'
}

export function createGraphSvg({ nodes, edges, filters, selectedNodeId, graphMode, theme }: SvgExportOptions) {
  const colors = exportTheme(theme)
  const { activeNodes, activeEdges } = activeGraph(nodes, edges, filters)
  const nodeById = new Map(activeNodes.map(node => [node.id, node]))
  const degree = buildDegreeMap(activeNodes, activeEdges)
  const bounds = graphBounds(activeNodes)
  const padding = 96
  const viewX = bounds.minX - padding
  const viewY = bounds.minY - padding
  const viewW = bounds.width + padding * 2
  const viewH = bounds.height + padding * 2
  const width = Math.round(Math.min(2200, Math.max(900, viewW)))
  const height = Math.round(Math.min(1800, Math.max(520, viewH)))
  const generatedAt = new Date().toISOString()

  return `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="${viewX} ${viewY} ${viewW} ${viewH}" role="img" aria-label="Rust watcher ${escapeXml(graphMode)} graph export">
  <defs>
    <radialGradient id="graph-bg" cx="50%" cy="50%" r="70%">
      <stop offset="0%" stop-color="${colors.bg2}"/>
      <stop offset="100%" stop-color="${colors.bg}"/>
    </radialGradient>
    <marker id="arrow" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M 0 0 L 10 5 L 0 10 z" fill="context-stroke" fill-opacity="0.8"/>
    </marker>
    <style>
      .node-label { font: 520 10px Inter, Arial, sans-serif; text-anchor: middle; fill: ${colors.textMuted}; paint-order: stroke; stroke: ${colors.card}; stroke-width: 3px; stroke-linejoin: round; }
      .node-label.selected { font-weight: 700; font-size: 12px; fill: ${colors.text}; }
      .language-badge { font: 800 8px Inter, Arial, sans-serif; text-anchor: middle; }
      .pin { font: 700 10px Arial, sans-serif; text-anchor: middle; fill: #F59E0B; }
      .edge-count { font: 700 10px Inter, Arial, sans-serif; text-anchor: middle; fill: ${colors.textMuted}; paint-order: stroke; stroke: ${colors.card}; stroke-width: 4px; }
      .meta { font: 600 11px Inter, Arial, sans-serif; fill: ${colors.textMuted}; }
      .title { font: 800 14px Inter, Arial, sans-serif; fill: ${colors.text}; }
    </style>
  </defs>
  <rect x="${viewX}" y="${viewY}" width="${viewW}" height="${viewH}" fill="url(#graph-bg)"/>
  <g opacity="0.85">
    ${renderGrid(viewX, viewY, viewW, viewH, colors.grid)}
  </g>
  <text x="${viewX + 28}" y="${viewY + 38}" class="title">Rust watcher · ${escapeXml(graphMode)} graph</text>
  <text x="${viewX + 28}" y="${viewY + 58}" class="meta">${activeNodes.length} nodes · ${activeEdges.length} edges · exported ${escapeXml(generatedAt)}</text>
  <g class="edges">
    ${activeEdges.map(edge => renderEdge(edge, nodeById)).join('\n')}
  </g>
  <g class="nodes">
    ${activeNodes.map(node => renderNode(node, degree.get(node.id) ?? 0, node.id === selectedNodeId, colors, activeNodes.length)).join('\n')}
  </g>
</svg>`
}

export function downloadGraphSvg(options: SvgExportOptions) {
  const svg = createGraphSvg(options)
  const blob = new Blob([svg], { type: 'image/svg+xml;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = `rust-watcher-${sanitizeFilePart(options.graphMode)}-graph.svg`
  document.body.appendChild(link)
  link.click()
  link.remove()
  window.setTimeout(() => URL.revokeObjectURL(url), 1000)
}
