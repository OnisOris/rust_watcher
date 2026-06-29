export const CLOUD_SESSION_STORAGE_KEY = 'rust-watcher-cloud-session'

export function cloudAuthHeaders(sessionToken: string | null): HeadersInit {
  return sessionToken ? { Authorization: `Bearer ${sessionToken}` } : {}
}

export async function cloudFetch(input: RequestInfo | URL, init: RequestInit = {}, sessionToken: string | null) {
  const headers = new Headers(init.headers)
  if (sessionToken) headers.set('Authorization', `Bearer ${sessionToken}`)
  return fetch(input, { ...init, headers })
}
