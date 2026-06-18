# Rust Code Command Center

Local browser app for exploring Rust projects as a live code graph.

## Setup

Build the frontend first:

```bash
cd frontend
pnpm install
pnpm build
```

Build the Rust workspace:

```bash
cargo build
```

## Run

Production-style local run:

```bash
cargo run -p web-server -- serve --project /path/to/rust/project --open
```

If `--project` is omitted, the server indexes the current working directory.
The default host is `127.0.0.1`, and `--port 0` binds a free local port.

Useful options:

```bash
cargo run -p web-server -- serve \
  --project /path/to/rust/project \
  --host 127.0.0.1 \
  --port 0 \
  --frontend-dist frontend/dist \
  --rust-analyzer rust-analyzer \
  --open
```

## Frontend Development

Run the backend on the Vite proxy port:

```bash
cargo run -p web-server -- serve --project /path/to/rust/project --port 34127
```

Then run Vite:

```bash
cd frontend
pnpm dev
```

Override the backend proxy target with `VITE_BACKEND_URL` when needed.

## API

- `GET /api/health`
- `GET /api/status`
- `GET /api/graph/snapshot?mode=Macro`
- `GET /api/node/:id`
- `GET /api/search?q=query`
- `POST /api/focus`
- `POST /api/project/open`
- `GET /ws`

The app never exposes file mutation, deletion, shell execution, or write APIs.
