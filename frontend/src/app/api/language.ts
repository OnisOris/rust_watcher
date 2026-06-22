import type { GraphNode, LanguageFilter } from '../types'

export type RecognizedLanguage = 'rust' | 'typescript' | 'javascript' | 'python' | 'qml' | 'endpoints' | 'external' | 'unknown'

export function inferNodeLanguage(node: Pick<GraphNode, 'language' | 'file' | 'type' | 'crate' | 'id' | 'label'>): RecognizedLanguage {
  if (node.type === 'Endpoint') return 'endpoints'
  if (node.type === 'ExternalCrate' || node.crate === 'external') return 'external'
  const explicit = normalizeLanguage(node.language)
  if (explicit !== 'unknown') return explicit
  const source = `${node.file ?? ''} ${node.id ?? ''} ${node.label ?? ''}`.toLowerCase()
  if (source.includes('.rs')) return 'rust'
  if (source.includes('.py')) return 'python'
  if (source.includes('.qml')) return 'qml'
  if (source.includes('.ts') || source.includes('.tsx')) return 'typescript'
  if (source.includes('.js') || source.includes('.jsx')) return 'javascript'
  return 'unknown'
}

export function languageFilterKey(language: RecognizedLanguage): LanguageFilter | null {
  if (language === 'javascript') return 'typescript'
  if (language === 'rust' || language === 'typescript' || language === 'python' || language === 'qml' || language === 'external' || language === 'endpoints') return language
  return null
}

export function languageDisplay(language: RecognizedLanguage | string) {
  switch (normalizeLanguage(language)) {
    case 'rust': return 'Rust'
    case 'typescript': return 'TypeScript'
    case 'javascript': return 'JavaScript'
    case 'python': return 'Python'
    case 'qml': return 'QML'
    case 'endpoints': return 'API'
    case 'external': return 'External'
    default: return 'Unknown'
  }
}

export function languageIcon(language: RecognizedLanguage | string) {
  switch (normalizeLanguage(language)) {
    case 'rust': return 'Rs'
    case 'typescript': return 'TS'
    case 'javascript': return 'JS'
    case 'python': return 'Py'
    case 'qml': return 'QML'
    case 'endpoints': return 'API'
    case 'external': return 'Ext'
    default: return '?'
  }
}

export function languageColor(language: RecognizedLanguage | string) {
  switch (normalizeLanguage(language)) {
    case 'rust': return '#3B82F6'
    case 'typescript': return '#14B8A6'
    case 'javascript': return '#F59E0B'
    case 'python': return '#F97316'
    case 'qml': return '#8B5CF6'
    case 'endpoints': return '#E11D48'
    case 'external': return '#7D8795'
    default: return '#94A3B8'
  }
}

export function normalizeLanguage(language?: string | null): RecognizedLanguage {
  const value = (language ?? '').trim().toLowerCase()
  if (value === 'ts' || value === 'tsx') return 'typescript'
  if (value === 'js' || value === 'jsx') return 'javascript'
  if (value === 'api' || value === 'endpoint') return 'endpoints'
  if (value === 'rust' || value === 'typescript' || value === 'javascript' || value === 'python' || value === 'qml' || value === 'endpoints' || value === 'external') return value
  return 'unknown'
}
