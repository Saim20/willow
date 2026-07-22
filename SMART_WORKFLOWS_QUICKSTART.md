# Quick Start — Smart Workflows

## Setup

```bash
# From the willow repo
./deploy-dev.sh

# Ensure context aliases exist
mkdir -p ~/.config/willow
cp context.json ~/.config/willow/context.json
```

Models (KWS, streaming ASR, Whisper, VAD) install under `~/.local/share/willow/models/` via deploy or `willow-download-model`.

## Usage

Say the hotword (**hey willow**), then:

### Opening applications

| Say this | What happens |
|----------|--------------|
| "open spotify" | Launches Spotify if installed |
| "open firefox" | Opens Firefox |
| "open terminal" | Opens your default terminal (kgx) — exact phrases early-fire |
| "open" then "firefox" | Multi-turn slot fill via the floating HUD |

Mid-word ASR fragments like `"open termin"` are held until the registered phrase completes.

### Web searches

| Say this | What happens |
|----------|--------------|
| "search youtube for cooking recipes" | YouTube results |
| "search google for weather today" | Google search |
| "search github for python projects" | GitHub search |

### Cancel

Say **"cancel"** or **"never mind"** to exit Command mode or clear a pending workflow.

## Customize

Edit `~/.config/willow/context.json` for app aliases and search engines. Edit `~/.config/willow/config.json` or use **GNOME Extensions → Willow** prefs for:

- Early command fire
- Streaming silence / workflow session timeout
- Optional local LLM fallback (GGUF path, tokens, timeout)
- Typing auto-revert

## Optional LLM

Requires `llama-cli` on `PATH` and a GGUF at the configured path. Off by default; only runs after endpoint when deterministic matching fails.
