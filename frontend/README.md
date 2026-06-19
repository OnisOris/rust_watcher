# rust-watcher frontend

React + TypeScript + Vite frontend for the rust_watcher live code graph.

## Setup

```bash
pnpm install
```

## Development

```bash
pnpm dev
```

The frontend expects the backend to provide the HTTP API under `/api` and the live graph WebSocket at `/ws`.

## Build

```bash
pnpm build
```

## Tests

```bash
pnpm test:run
```
