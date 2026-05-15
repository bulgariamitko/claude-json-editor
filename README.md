# claude.json editor

A desktop GUI for managing your Claude Code config (`~/.claude.json`), settings (`~/.claude/settings.json`), hooks, and skills.

Built with [Tauri](https://tauri.app/) — small native binary, no Node runtime, no server, no Electron. Works on macOS, Windows, and Linux.

## Why

The `~/.claude.json` file grows large over time (hundreds of project entries, MCP server configs, skill overrides, hooks). Editing it by hand is error-prone. This app gives you:

- **MCP** — view and edit global + per-project MCP servers, plus visibility into plugin-managed referenced names
- **Projects** — list every project entry with a live ✅/❌ indicator for whether the folder still exists on disk; one-click cleanup of missing entries
- **Hooks** — manage hooks across all 20 events (`SessionStart`, `PreToolUse`, `Stop`, etc.) with matcher groups
- **Skills** — list user skills, edit `SKILL.md` in a textarea modal, toggle visibility (`on` / `name-only` / `user-invocable-only` / `off`)
- **Caches & Counters** — reset usage counters, prompt queue caches
- **Raw JSON** — search and edit the full file at any key path
- **Backups** — auto-rolling backups before every save (last 10 kept), one-click restore

## Install

### macOS

Download the `.dmg` for your architecture (`aarch64` for Apple Silicon, `x64` for Intel) from the [latest release](../../releases/latest), open it, and drag the app to Applications.

The app is unsigned by default. On first launch right-click → Open → Open, or run:

```sh
xattr -dr com.apple.quarantine "/Applications/claude-json-editor.app"
```

### Windows

Download the `.msi` installer from the [latest release](../../releases/latest) and run it.

### Linux

Download either the `.AppImage` (portable, just `chmod +x` and run) or the `.deb` for Debian/Ubuntu from the [latest release](../../releases/latest).

## Auto-update

The app silently checks for new releases on launch. When a new version is found, you'll be prompted to install — accepts, downloads, restarts. Updates are cryptographically signed (Minisign / Ed25519) and verified before install.

## Safety

- **Auto-backup** before every save. The last 10 backups are kept in `~/.claude.json.backups/`.
- **No telemetry, no network calls** except the update check against GitHub releases.
- **Local-only**: every operation is a direct read/write of files under your home directory.

## Build from source

You need [Rust](https://rustup.rs/) and [Node](https://nodejs.org/).

```sh
git clone https://github.com/bulgariamitko/claude-json-editor.git
cd claude-json-editor
npm install
npm run dev        # hot-reload dev mode
npm run build      # release build → src-tauri/target/release/bundle/
```

## License

MIT — see [LICENSE](./LICENSE).
