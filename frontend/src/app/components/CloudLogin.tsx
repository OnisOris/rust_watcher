import { useState } from 'react'
import { LockKeyhole } from 'lucide-react'

interface CloudLoginProps {
  onLogin: (sessionToken: string) => void
}

export function CloudLogin({ onLogin }: CloudLoginProps) {
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  async function login() {
    if (!username.trim() || !password) return
    setBusy(true)
    setError(null)
    try {
      const response = await fetch('/api/cloud/auth/login', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username: username.trim(), password }),
      })
      if (!response.ok) throw new Error(await response.text())
      const payload = await response.json() as { sessionToken: string }
      onLogin(payload.sessionToken)
    } catch (error) {
      setError(error instanceof Error ? error.message : 'Login failed.')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="w-full h-full flex items-center justify-center" style={{ background: 'var(--cc-bg)', color: 'var(--cc-text)', fontFamily: 'Inter, sans-serif' }}>
      <div style={{ width: 360, background: 'var(--cc-panel)', border: '1px solid var(--cc-border)', borderRadius: 8, padding: 20 }}>
        <div className="flex items-center gap-2" style={{ marginBottom: 14 }}>
          <LockKeyhole size={18} />
          <h1 style={{ margin: 0, fontSize: 18, fontWeight: 760 }}>Cloud access</h1>
        </div>
        <div className="grid gap-2">
          <input
            value={username}
            onChange={event => setUsername(event.target.value)}
            onKeyDown={event => { if (event.key === 'Enter') void login() }}
            placeholder="Username"
            autoComplete="username"
            autoFocus
            style={inputStyle}
          />
          <input
            value={password}
            onChange={event => setPassword(event.target.value)}
            onKeyDown={event => { if (event.key === 'Enter') void login() }}
            placeholder="Password"
            type="password"
            autoComplete="current-password"
            style={inputStyle}
          />
        </div>
        {error && <div style={{ marginTop: 10, color: '#B91C1C', fontSize: 12 }}>{error}</div>}
        <button
          onClick={() => void login()}
          disabled={busy || !username.trim() || !password}
          style={{
            marginTop: 12,
            width: '100%',
            minHeight: 38,
            border: 'none',
            borderRadius: 8,
            background: '#06B6D4',
            color: '#fff',
            fontWeight: 740,
            cursor: busy ? 'wait' : 'pointer',
          }}
        >
          {busy ? 'Checking...' : 'Continue'}
        </button>
      </div>
    </div>
  )
}

const inputStyle = {
  width: '100%',
  background: 'var(--cc-surface)',
  border: '1px solid var(--cc-border)',
  borderRadius: 8,
  color: 'var(--cc-text)',
  outline: 'none',
  padding: '10px 11px',
  fontSize: 13,
}
