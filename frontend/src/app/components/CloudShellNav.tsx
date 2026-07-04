import type { ReactNode } from 'react'
import { Boxes, Code2, FolderOpen, GitBranch, UserRound } from 'lucide-react'

export type CloudShellTab = 'workspaces' | 'new' | 'graph' | 'account' | 'ide'

interface CloudShellNavProps {
  activeTab: CloudShellTab
  username?: string | null
  graphEnabled?: boolean
  onNavigate: (tab: CloudShellTab) => void
}

const tabs: Array<{ id: CloudShellTab; label: string; icon: ReactNode; disabled?: boolean }> = [
  { id: 'workspaces', label: 'Workspaces', icon: <FolderOpen size={15} /> },
  { id: 'new', label: 'New analysis', icon: <GitBranch size={15} /> },
  { id: 'graph', label: 'Graph', icon: <Boxes size={15} /> },
  { id: 'ide', label: 'Browser IDE', icon: <Code2 size={15} /> },
  { id: 'account', label: 'Account', icon: <UserRound size={15} /> },
]

export function CloudShellNav({ activeTab, username, graphEnabled = false, onNavigate }: CloudShellNavProps) {
  return (
    <div
      className="flex items-center justify-between gap-4 px-5 shrink-0"
      style={{
        minHeight: 56,
        background: 'var(--cc-panel)',
        borderBottom: '1px solid var(--cc-border)',
        fontFamily: 'Inter, sans-serif',
      }}
    >
      <div className="flex items-center gap-3 min-w-0">
        <div
          className="flex items-center justify-center rounded-lg"
          style={{ width: 30, height: 30, background: '#06B6D4', color: '#fff', fontWeight: 800 }}
        >
          R
        </div>
        <div style={{ minWidth: 0 }}>
          <div style={{ fontSize: 13, fontWeight: 780, color: 'var(--cc-text)' }}>Rust Watcher Cloud</div>
          <div style={{ fontSize: 11, color: 'var(--cc-text-muted)' }}>{username ? `Signed in as ${username}` : 'Cloud workspace'}</div>
        </div>
      </div>

      <div className="flex items-center gap-1 rounded-xl p-1" style={{ background: 'var(--cc-surface)', border: '1px solid var(--cc-border)' }}>
        {tabs.map(tab => {
          const disabled = tab.disabled || ((tab.id === 'graph' || tab.id === 'ide') && !graphEnabled)
          const active = activeTab === tab.id
          return (
            <button
              key={tab.id}
              onClick={() => !disabled && onNavigate(tab.id)}
              disabled={disabled}
              className="flex items-center gap-1.5 rounded-lg"
              title={tab.label}
              style={{
                minHeight: 34,
                padding: '0 12px',
                border: active ? '1px solid rgba(14,165,233,0.35)' : '1px solid transparent',
                background: active ? 'var(--cc-selected-soft)' : 'transparent',
                color: disabled ? 'var(--cc-text-faint)' : active ? 'var(--cc-accent)' : 'var(--cc-text-subtle)',
                fontSize: 12,
                fontWeight: active ? 780 : 680,
                cursor: disabled ? 'not-allowed' : 'pointer',
                whiteSpace: 'nowrap',
              }}
            >
              {tab.icon}
              {tab.label}
            </button>
          )
        })}
      </div>
    </div>
  )
}
