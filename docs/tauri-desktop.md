# Tauri Desktop Integration

SilverBond can optionally be packaged as a native desktop application using Tauri 2. The Tauri shell is pure packaging — it does not fork runtime semantics.

## Architecture

The Tauri app (`src-tauri/`) wraps the same Rust backend that runs in standalone mode:

```
┌─────────────────────────────────┐
│         Tauri Window            │
│  ┌───────────────────────────┐  │
│  │   WebView (localhost:N)   │  │
│  │   Same Svelte frontend   │  │
│  └────────────┬──────────────┘  │
│               │ HTTP + SSE      │
│  ┌────────────┴──────────────┐  │
│  │   Rust Backend (in-proc)  │  │
│  │   Same Axum server        │  │
│  └───────────────────────────┘  │
└─────────────────────────────────┘
```

## Key Differences from Standalone

| Aspect | Standalone | Tauri |
|--------|-----------|-------|
| Port | Fixed `127.0.0.1:3333` | Ephemeral `127.0.0.1:0` |
| App root | CWD or `SILVERBOND_ROOT` | Platform app-data directory |
| Template seeding | Manual | Automatic on first launch |
| Frontend | Embedded in binary or dev server | Same, viewed in WebView |
| Shutdown | Ctrl+C | Window close |

## Startup Sequence

1. Initialize tracing
2. Create `Arc<Mutex<Option<ApplicationHost>>>` for lifecycle management
3. In Tauri `setup` hook:
   - Resolve platform app-data directory
   - Start `ApplicationHost` on ephemeral port (`:0`)
   - Wait for `/api/health` (5-second timeout)
   - Open main WebView pointing to resolved `http://127.0.0.1:{port}`
4. On window close:
   - Gracefully shut down the `ApplicationHost`
   - Exit the process

## Window Configuration

- **Title**: SilverBond
- **Size**: 1440 x 960 (default)
- **Minimum size**: 1100 x 700
- **Resizable**: Yes

## Dependencies

```toml
[dependencies]
tauri = "2.10.1"
silverbond = { path = ".." }  # Same crate as standalone
tokio = { version = "1.47", features = ["rt-multi-thread", "macros", "signal"] }
tracing = "0.1"
tracing-subscriber = "0.3"
```

## Building

```bash
# Development with hot reload
just tauri-dev

# Package as native desktop app
just tauri-build
```

The Tauri build produces platform-specific packages (`.dmg` on macOS, `.msi` on Windows, `.deb`/`.AppImage` on Linux).

## Template Seeding

On first launch, Tauri mode seeds bundled templates from the `templates/` directory into the app-data root. This ensures new users have example workflows available without manual setup. The standalone mode does not seed templates automatically.

## Design Principle

Tauri is packaging and windowing only. It must preserve:
- The same Rust runtime core
- The same HTTP/SSE browser contract
- The same persistence model (SQLite + JSON files)

It must not introduce a separate desktop execution model, Tauri-specific commands, or IPC-based APIs that bypass the HTTP layer.
