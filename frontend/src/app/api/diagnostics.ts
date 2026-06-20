import type { AnalysisEvent, DiagnosticRecord, DiagnosticSeverity } from '../types'

export interface DiagnosticSeverityCounts {
  error: number
  warning: number
  information: number
  hint: number
  total: number
  worst: DiagnosticSeverity | null
}

export function diagnosticSeverityCountsForFile(
  diagnosticsByFile: Map<string, DiagnosticRecord[]>,
  file: string,
): DiagnosticSeverityCounts {
  return diagnosticSeverityCounts(diagnosticsByFile.get(file) ?? [])
}

export function diagnosticSeverityCounts(diagnostics: DiagnosticRecord[]): DiagnosticSeverityCounts {
  const counts: DiagnosticSeverityCounts = {
    error: 0,
    warning: 0,
    information: 0,
    hint: 0,
    total: diagnostics.length,
    worst: null,
  }
  for (const diagnostic of diagnostics) {
    switch (diagnostic.severity) {
      case 'Error':
        counts.error += 1
        break
      case 'Warning':
        counts.warning += 1
        break
      case 'Information':
        counts.information += 1
        break
      case 'Hint':
        counts.hint += 1
        break
    }
  }
  counts.worst = counts.error > 0
    ? 'Error'
    : counts.warning > 0
      ? 'Warning'
      : counts.information > 0
        ? 'Information'
        : counts.hint > 0
          ? 'Hint'
          : null
  return counts
}

export function diagnosticColor(severity: DiagnosticSeverity | null) {
  switch (severity) {
    case 'Error':
      return '#DC2626'
    case 'Warning':
      return '#D97706'
    case 'Information':
      return '#0284C7'
    case 'Hint':
      return '#64748B'
    default:
      return 'var(--cc-text-faint)'
  }
}

export function diagnosticsToAnalysisEvents(
  diagnosticsByFile: Map<string, DiagnosticRecord[]>,
): AnalysisEvent[] {
  return [...diagnosticsByFile.values()]
    .flat()
    .filter(diagnostic => diagnostic.severity === 'Error' || diagnostic.severity === 'Warning')
    .map(diagnostic => ({
      id: `diagnostic-event:${diagnostic.id}`,
      type: diagnostic.severity === 'Error' ? 'error' : 'warning',
      file: diagnostic.file,
      message: diagnostic.range
        ? `${diagnostic.message} · L${diagnostic.range.start.line + 1}:${diagnostic.range.start.character + 1}`
        : diagnostic.message,
      timestamp: diagnostic.source ?? 'diagnostic',
    }))
}
