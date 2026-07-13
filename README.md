# Willow

Offline voice assistant for GNOME. Uses [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) for keyword spotting, streaming speech recognition, and speaker verification. No cloud required after models are installed.

## Requirements

- GNOME Shell 45+
- PulseAudio or PipeWire
- [ydotool](https://github.com/ReimuNotMoe/ydotool) (typing mode and key commands)
- Optional: `speech-dispatcher` or `espeak` (TTS feedback)

## Install

**Arch (from repo):**

```bash
git clone https://github.com/Saim20/willow.git && cd willow
makepkg -si
```

**Manual:**

```bash
# Arch deps example
sudo pacman -S gnome-shell sdbus-cpp jsoncpp libpulse ydotool curl cmake git gcc

cd willow/service && mkdir -p build && cd build
cmake -DCMAKE_INSTALL_PREFIX=/usr ..
make -j$(nproc) && sudo cmake --install . --component willow

cp -r ../../gnome-extension/willow@saim ~/.local/share/gnome-shell/extensions/
glib-compile-schemas ~/.local/share/gnome-shell/extensions/willow@saim/schemas/
```

## Setup

Run these from a **GNOME graphical session** (not SSH):

```bash
# 1. Download models (~160 MB)
willow-download-model

# 2. Enable extension and start service
gnome-extensions enable willow@saim
systemctl --user start willow.service
systemctl --user enable willow.service   # optional autostart

# 3. Enroll your voice (recommended)
gnome-extensions prefs willow@saim   # Voice tab → Start Enrollment

# 4. ydotool for typing/key commands
sudo systemctl enable --now ydotool
sudo usermod -aG input $USER   # then log out and back in
```

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
| `~/.config/willow/config.json` | Hotword, thresholds, commands |
| `~/.config/willow/context.json` | Default apps, search engines, aliases |

Preferences UI syncs with the D-Bus service. Edit commands in **Preferences → Commands**.

## Troubleshooting

```bash
# Service status and logs
systemctl --user status willow.service
journalctl --user -u willow.service -f
tail -f /tmp/willow.log

# D-Bus check
gdbus introspect --session --dest com.github.saim.Willow \
  --object-path /com/github/saim/VoiceAssistant

# Models missing?
ls ~/.local/share/willow/models/{kws,streaming,speaker}
willow-download-model && systemctl --user restart willow.service
```

**Common issues:**
- **Hotword not working** — hotword must match config (`hey willow` by default); check D-Bus status fields `kws_ready`, `kws_keywords_source`, and `init_error`. To isolate speaker verification, set `speaker_verification.enabled` to `false` in `~/.config/willow/config.json`
- **Speaker verify fails** — re-enroll in Preferences → Voice tab; you'll hear "Voice not recognized" on failure. Status field `speaker_verification_last_result` shows the last attempt (1=pass, 0=fail, -1=none)
- **Typing does nothing** — check ydotool service and `input` group membership
- **Search won't open browser** — service needs a graphical session (`WAYLAND_DISPLAY`/`DISPLAY`)

## Development

```bash
cd service/build
cmake -DCMAKE_INSTALL_PREFIX=/usr ..
make -j$(nproc)
./willow-service
```

## License

MIT — see [LICENSE](LICENSE).
