import { describe, expect, it } from 'vitest'
import {
  diagnosticSeverityCountsForFile,
  diagnosticsToAnalysisEvents,
} from './diagnostics'
import type { DiagnosticRecord } from '../types'

function diagnostic(id: string, severity: DiagnosticRecord['severity'], file = 'src/main.rs'): DiagnosticRecord {
  return {
    id,
    language: 'rust',
    file,
    severity,
    message: `${severity} diagnostic`,
    relatedNodeIds: ['node'],
    range: {
      start: { line: 2, character: 4 },
      end: { line: 2, character: 8 },
    },
  }
}

describe('diagnostic helpers', () => {
  it('counts severities and reports the worst file severity', () => {
    const byFile = new Map([
      ['src/main.rs', [
        diagnostic('hint', 'Hint'),
        diagnostic('warning', 'Warning'),
      ]],
    ])

    const counts = diagnosticSeverityCountsForFile(byFile, 'src/main.rs')

    expect(counts.total).toBe(2)
    expect(counts.warning).toBe(1)
    expect(counts.hint).toBe(1)
    expect(counts.worst).toBe('Warning')
  })

  it('converts error and warning diagnostics into timeline events', () => {
    const byFile = new Map([
      ['src/main.rs', [
        diagnostic('error', 'Error'),
        diagnostic('info', 'Information'),
        diagnostic('warning', 'Warning', 'src/lib.rs'),
      ]],
    ])

    const events = diagnosticsToAnalysisEvents(byFile)

    expect(events.map(event => event.type)).toEqual(['error', 'warning'])
    expect(events[0].message).toContain('L3:5')
    expect(events.some(event => event.message.includes('Information'))).toBe(false)
  })
})
