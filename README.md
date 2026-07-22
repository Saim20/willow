# Willow

Offline voice assistant for GNOME. Uses [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) for keyword spotting, Silero VAD, Whisper ASR, and speaker verification. No cloud required after models are installed.

## Requirements

- GNOME Shell 45+
- PulseAudio or PipeWire
- [ydotool](https://github.com/ReimuNotMoe/ydotool) (typing mode and key commands)
- Optional: NVIDIA GPU + `cuda` + `cudnn` (matched to your toolkit — CUDA 13 uses Microsoft ORT cuda13 + a local sherpa build; no CUDA 12 wheel fallback)

## Install (AUR / makepkg)

From a **GNOME graphical session**:

```bash
git clone https://github.com/Saim20/willow.git && cd willow
makepkg -si
```

If `cuda` and `cudnn` are installed, the package builds with GPU sherpa-onnx automatically (native to your CUDA major). After install, `willow.install`:

- Creates `~/.config/willow/`
- Downloads speech models (~210 MB)
- Enables the GNOME extension and starts `willow.service` when a user session is active

Enroll your voice: `gnome-extensions prefs willow@saim` → Voice tab.

ydotool (typing / key commands):

```bash
sudo systemctl enable --now ydotool
sudo usermod -aG input $USER   # then log out and back in
```

## Development setup

One command from the repo root:

```bash
./deploy-dev.sh
```

This checks dependencies, links the extension, builds (CUDA when NVIDIA + cuda/cudnn are present), downloads models if missing, installs the user service, and enables the extension.

Options:

| Flag | Effect |
|------|--------|
| `--cpu` | Force CPU build |
| `--skip-models` | Skip model download |
| `--system` | Install binary/unit system-wide (sudo) |

## Usage

| Mode | How to enter | What it does |
|------|--------------|--------------|
| Normal | Default | Listens for hotword only (low CPU) |
| Command | Say **"hey willow"** | Run voice commands, open apps, search web |
| Typing | Say **"typing mode"** | Live dictation into focused window |

Exit typing with **"stop typing"**. Return to normal with **"normal mode"** or **"exit"**.

**Smart commands** (in command mode):
- `open firefox` / `launch spotify` — opens apps
- `search youtube for music` — web search via default browser

Customize defaults in `~/.config/willow/context.json`.

## Configuration

| File | Purpose |
|------|---------|
| `~/.config/willow/config.json` | Hotword, thresholds, commands, `inference` |
| `~/.config/willow/context.json` | Default apps, search engines, aliases |

Streaming is not used for commands. **Silero VAD** detects utterance boundaries; **Whisper** (CUDA when available) transcribes each segment. Set `"inference": { "provider": "auto" }` (or **GPU Acceleration** in prefs).

## Troubleshooting

```bash
systemctl --user status willow.service
journalctl --user -u willow.service -f

# Models
ls ~/.local/share/willow/models/{kws,whisper,vad,speaker}
willow-download-model && systemctl --user restart willow.service
```

**Common issues:**
- **Hotword not working** — check `kws_ready` / `init_error` on D-Bus status; try disabling speaker verification in config
- **Typing does nothing** — ydotool service + `input` group
- **Search won't open browser** — needs a graphical session (`WAYLAND_DISPLAY`/`DISPLAY`)
- **Panel icon ERROR** — Alt+F2 → `r` → Enter (or log out/in on Wayland)

## License

MIT — see [LICENSE](LICENSE).
