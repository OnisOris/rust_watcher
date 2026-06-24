import { exportRecordedCanvasAsSvg } from './canvasSvgRecorder'

const EXPORT_WIDTH = 1920
const EXPORT_HEIGHT = 1080
const MINIMAP_WIDTH = 160
const EXPORT_MARGIN = 16

function numericAttr(element: Element, name: string, fallback: number) {
  const raw = element.getAttribute(name)
  if (!raw) return fallback
  const parsed = Number.parseFloat(raw)
  return Number.isFinite(parsed) ? parsed : fallback
}

function firstPathPoint(path: string) {
  const match = path.match(/-?\d+(?:\.\d+)?(?:e[-+]?\d+)?/gi)
  if (!match || match.length < 2) return null
  const x = Number.parseFloat(match[0])
  const y = Number.parseFloat(match[1])
  return Number.isFinite(x) && Number.isFinite(y) ? { x, y } : null
}

function hasIdentityTransform(element: Element) {
  const transform = element.getAttribute('transform')?.trim()
  return !transform || transform === 'matrix(1 0 0 1 0 0)'
}

function isInsideMinimapOverlay(element: Element, sourceWidth: number, sourceHeight: number) {
  if (!hasIdentityTransform(element)) return false

  const minimapX = sourceWidth - MINIMAP_WIDTH - EXPORT_MARGIN
  const minimapY = sourceHeight - 100 - EXPORT_MARGIN
  let x: number | null = null
  let y: number | null = null

  if (element.tagName === 'rect') {
    x = numericAttr(element, 'x', Number.NaN)
    y = numericAttr(element, 'y', Number.NaN)
  } else if (element.tagName === 'path') {
    const point = firstPathPoint(element.getAttribute('d') ?? '')
    if (point) {
      x = point.x
      y = point.y
    }
  } else if (element.tagName === 'text') {
    x = numericAttr(element, 'x', Number.NaN)
    y = numericAttr(element, 'y', Number.NaN)
  }

  if (x === null || y === null || !Number.isFinite(x) || !Number.isFinite(y)) return false
  return x >= minimapX - 4 && x <= sourceWidth - 4 && y >= minimapY - 4 && y <= sourceHeight - 4
}

function backgroundColorFrom(svg: SVGSVGElement) {
  const gradient = svg.querySelector('radialGradient')
  const stops = gradient ? Array.from(gradient.querySelectorAll('stop')) : []
  return stops.at(-1)?.getAttribute('stop-color') ?? '#eef4fb'
}

function normalizeGraphSvg(svgText: string) {
  const doc = new DOMParser().parseFromString(svgText, 'image/svg+xml')
  const parseError = doc.querySelector('parsererror')
  const svg = doc.documentElement as unknown as SVGSVGElement
  if (parseError || svg.tagName.toLowerCase() !== 'svg') return svgText

  const sourceWidth = numericAttr(svg, 'width', EXPORT_WIDTH)
  const sourceHeight = numericAttr(svg, 'height', EXPORT_HEIGHT)
  const scale = Math.min(EXPORT_WIDTH / sourceWidth, EXPORT_HEIGHT / sourceHeight)
  const offsetX = (EXPORT_WIDTH - sourceWidth * scale) / 2
  const offsetY = (EXPORT_HEIGHT - sourceHeight * scale) / 2
  const background = backgroundColorFrom(svg)

  for (const child of Array.from(svg.children)) {
    if (child.tagName.toLowerCase() === 'defs') continue
    if (isInsideMinimapOverlay(child, sourceWidth, sourceHeight)) child.remove()
  }

  const group = doc.createElementNS('http://www.w3.org/2000/svg', 'g')
  group.setAttribute('transform', `translate(${Number(offsetX.toFixed(3))} ${Number(offsetY.toFixed(3))}) scale(${Number(scale.toFixed(6))})`)

  for (const child of Array.from(svg.children)) {
    if (child.tagName.toLowerCase() === 'defs') continue
    group.appendChild(child)
  }

  const backgroundRect = doc.createElementNS('http://www.w3.org/2000/svg', 'rect')
  backgroundRect.setAttribute('width', String(EXPORT_WIDTH))
  backgroundRect.setAttribute('height', String(EXPORT_HEIGHT))
  backgroundRect.setAttribute('fill', background)

  svg.setAttribute('width', String(EXPORT_WIDTH))
  svg.setAttribute('height', String(EXPORT_HEIGHT))
  svg.setAttribute('viewBox', `0 0 ${EXPORT_WIDTH} ${EXPORT_HEIGHT}`)
  svg.insertBefore(backgroundRect, svg.firstChild)
  svg.appendChild(group)

  return new XMLSerializer().serializeToString(doc)
}

export function exportGraphCanvasAsSvg(canvas: HTMLCanvasElement, filename: string) {
  const BlobCtor = window.Blob

  class ExportSvgBlob extends BlobCtor {
    constructor(parts?: BlobPart[], options?: BlobPropertyBag) {
      const nextParts = options?.type?.includes('image/svg+xml') && typeof parts?.[0] === 'string'
        ? [normalizeGraphSvg(parts[0])]
        : parts
      super(nextParts, options)
    }
  }

  try {
    Object.defineProperty(window, 'Blob', { configurable: true, writable: true, value: ExportSvgBlob })
    return exportRecordedCanvasAsSvg(canvas, filename)
  } finally {
    Object.defineProperty(window, 'Blob', { configurable: true, writable: true, value: BlobCtor })
  }
}
