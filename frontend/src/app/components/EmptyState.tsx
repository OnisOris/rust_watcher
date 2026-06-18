import { useEffect, useRef, useState } from 'react'
import { FolderOpen, Clock, Zap } from 'lucide-react'

interface EmptyStateProps {
  onOpenProject: (path?: string) => void
}

const RECENT_PROJECTS = [
  { name: 'axum-web-api', path: '~/projects/axum-web-api', ago: '2 hours ago' },
  { name: 'tokio-runtime', path: '~/code/tokio-runtime', ago: '3 days ago' },
  { name: 'serde-json', path: '~/oss/serde-json', ago: '1 week ago' },
]

export function EmptyState({ onOpenProject }: EmptyStateProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const [projectPath, setProjectPath] = useState('')

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')!
    const dpr = window.devicePixelRatio || 1

    const resize = () => {
      const rect = canvas.getBoundingClientRect()
      canvas.width = rect.width * dpr
      canvas.height = rect.height * dpr
      ctx.scale(dpr, dpr)
    }
    resize()

    const themeColor = (name: string, fallback: string) =>
      getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback
    const W = canvas.offsetWidth, H = canvas.offsetHeight
    const cx = W / 2, cy = H / 2

    // Abstract decorative graph
    const nodes = [
      { x: cx, y: cy, r: 10, color: '#06B6D4' },
      { x: cx - 90, y: cy - 55, r: 7, color: '#8B5CF6' },
      { x: cx + 100, y: cy - 45, r: 8, color: '#EC4899' },
      { x: cx - 120, y: cy + 60, r: 6, color: '#10B981' },
      { x: cx + 80, y: cy + 70, r: 7, color: '#F59E0B' },
      { x: cx - 40, y: cy - 100, r: 5, color: '#3B82F6' },
      { x: cx + 40, y: cy + 100, r: 6, color: '#6366F1' },
      { x: cx - 180, y: cy + 10, r: 5, color: '#7D8795' },
      { x: cx + 170, y: cy + 20, r: 5, color: '#7D8795' },
    ]

    const edges = [
      [0, 1], [0, 2], [0, 3], [0, 4], [1, 5], [1, 3], [2, 6], [3, 7], [4, 6],
    ]

    let t = 0
    let raf: number

    const draw = () => {
      const rect = canvas.getBoundingClientRect()
      const W2 = rect.width, H2 = rect.height
      ctx.clearRect(0, 0, W2, H2)

      // edges
      for (const [a, b] of edges) {
        const na = nodes[a], nb = nodes[b]
        ctx.save()
        ctx.strokeStyle = themeColor('--cc-border-strong', '#b7c6d8')
        ctx.lineWidth = 1.5
        ctx.globalAlpha = 0.6
        ctx.beginPath()
        ctx.moveTo(na.x, na.y)
        ctx.lineTo(nb.x, nb.y)
        ctx.stroke()
        ctx.restore()
      }

      // animated data flow dot on first edge
      const [ea, eb] = [nodes[0], nodes[1]]
      const progress = (t % 100) / 100
      ctx.save()
      ctx.fillStyle = '#06B6D4'
      ctx.globalAlpha = 0.8
      ctx.beginPath()
      ctx.arc(ea.x + (eb.x - ea.x) * progress, ea.y + (eb.y - ea.y) * progress, 3, 0, Math.PI * 2)
      ctx.fill()
      ctx.restore()

      // nodes
      for (const n of nodes) {
        const pulse = n === nodes[0] ? Math.sin(t * 0.04) * 3 : 0
        ctx.save()
        ctx.shadowBlur = 18 + pulse
        ctx.shadowColor = n.color
        ctx.fillStyle = themeColor('--cc-card', '#ffffff')
        ctx.strokeStyle = n.color
        ctx.lineWidth = 1.5
        ctx.globalAlpha = 0.85
        ctx.beginPath()
        ctx.arc(n.x, n.y, n.r + pulse * 0.3, 0, Math.PI * 2)
        ctx.fill()
        ctx.stroke()
        ctx.restore()
      }

      t++
      raf = requestAnimationFrame(draw)
    }

    raf = requestAnimationFrame(draw)
    return () => cancelAnimationFrame(raf)
  }, [])

  return (
    <div
      className="flex items-center justify-center w-full h-full relative overflow-hidden"
      style={{ background: 'var(--cc-bg)', fontFamily: 'Inter, sans-serif' }}
    >
      <canvas ref={canvasRef} className="absolute inset-0 w-full h-full" style={{ opacity: 0.4 }} />

      <div className="relative z-10 flex flex-col items-center text-center" style={{ maxWidth: 480 }}>
        {/* logo */}
        <div
          className="flex items-center justify-center rounded-2xl mb-6"
          style={{ width: 64, height: 64, background: 'linear-gradient(135deg, #06B6D420 0%, #7C3AED20 100%)', border: '1px solid var(--cc-border-strong)' }}
        >
          <div
            className="flex items-center justify-center rounded-xl"
            style={{ width: 48, height: 48, background: 'linear-gradient(135deg, #06B6D4 0%, #7C3AED 100%)' }}
          >
            <Zap size={24} color="#fff" />
          </div>
        </div>

        <h1 style={{ fontSize: 28, fontWeight: 700, color: 'var(--cc-text)', letterSpacing: '-0.03em', marginBottom: 8, lineHeight: 1.2 }}>
          Rust Code <span style={{ color: '#06B6D4' }}>Command Center</span>
        </h1>
        <p style={{ fontSize: 14, color: 'var(--cc-text-muted)', marginBottom: 32, lineHeight: 1.6 }}>
          Understand your Rust project as a living system.<br />
          Open a workspace to see its live code graph.
        </p>

        {/* CTA */}
        <button
          onClick={() => onOpenProject(projectPath.trim() || undefined)}
          className="flex items-center gap-2 rounded-xl transition-all mb-4"
          style={{
            padding: '12px 28px',
            background: 'linear-gradient(135deg, #06B6D4 0%, #7C3AED 100%)',
            color: '#fff',
            fontSize: 14,
            fontWeight: 600,
            border: 'none',
            cursor: 'pointer',
            boxShadow: '0 8px 24px rgba(6,182,212,0.25)',
            letterSpacing: '-0.01em',
          }}
        >
          <FolderOpen size={16} />
          Open Rust Workspace
        </button>

        <input
          value={projectPath}
          onChange={event => setProjectPath(event.target.value)}
          placeholder="/path/to/rust/project"
          style={{
            width: '100%',
            maxWidth: 420,
            background: 'var(--cc-surface)',
            border: '1px solid var(--cc-border)',
            borderRadius: 8,
            color: 'var(--cc-text)',
            fontFamily: 'JetBrains Mono, monospace',
            fontSize: 12,
            outline: 'none',
            padding: '9px 11px',
            marginBottom: 8,
          }}
        />
        <button
          onClick={() => onOpenProject()}
          style={{
            background: 'none',
            border: 'none',
            color: 'var(--cc-text-subtle)',
            cursor: 'pointer',
            fontSize: 11,
            marginBottom: 8,
          }}
        >
          Use project from server CLI
        </button>

        {/* recent projects */}
        <div className="w-full rounded-xl overflow-hidden" style={{ background: 'var(--cc-panel)', border: '1px solid var(--cc-border)', marginTop: 16 }}>
          <div className="px-4 py-2.5" style={{ borderBottom: '1px solid var(--cc-border)' }}>
            <p style={{ fontSize: 11, color: 'var(--cc-text-subtle)', fontWeight: 600, letterSpacing: '0.07em', textTransform: 'uppercase' }}>
              Recent Projects
            </p>
          </div>
          {RECENT_PROJECTS.map(p => (
            <button
              key={p.name}
              onClick={() => onOpenProject()}
              className="flex items-center gap-3 w-full px-4 py-3 transition-colors"
              style={{ borderBottom: '1px solid var(--cc-border)', background: 'none', cursor: 'pointer' }}
            >
              <div className="flex items-center justify-center rounded-lg shrink-0" style={{ width: 32, height: 32, background: 'var(--cc-elevated)' }}>
                <FolderOpen size={14} color="#8B5CF6" />
              </div>
              <div className="flex-1 text-left">
                <div style={{ fontSize: 13, color: 'var(--cc-text)', fontWeight: 500 }}>{p.name}</div>
                <div style={{ fontSize: 11, color: 'var(--cc-text-faint)', fontFamily: 'JetBrains Mono, monospace' }}>{p.path}</div>
              </div>
              <div className="flex items-center gap-1" style={{ color: 'var(--cc-text-faint)', fontSize: 11 }}>
                <Clock size={11} />
                {p.ago}
              </div>
            </button>
          ))}
        </div>

        {/* features */}
        <div className="grid grid-cols-3 gap-3 mt-6 w-full">
          {[
            { icon: '🗺', label: 'Live Code Graph', desc: 'Force-directed, always up to date' },
            { icon: '🔍', label: 'Focus Bubble', desc: 'Zoom into any symbol context' },
            { icon: '⚡', label: 'rust-analyzer', desc: 'Semantic analysis in real time' },
          ].map(f => (
            <div key={f.label} className="rounded-xl p-3 text-center" style={{ background: 'var(--cc-panel)', border: '1px solid var(--cc-border)' }}>
              <div style={{ fontSize: 22, marginBottom: 4 }}>{f.icon}</div>
              <div style={{ fontSize: 11, color: 'var(--cc-text)', fontWeight: 500, marginBottom: 2 }}>{f.label}</div>
              <div style={{ fontSize: 10, color: 'var(--cc-text-subtle)' }}>{f.desc}</div>
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}
