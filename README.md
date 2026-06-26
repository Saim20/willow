# 🌿 Willow - Voice Assistant for GNOME

**Simple offline configurable voice assistant for GNOME**

Willow brings powerful offline voice control to your GNOME desktop using whisper.cpp for fast, accurate speech recognition. Control your desktop entirely by voice - no cloud, no tracking, no internet required.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![GNOME Shell](https://img.shields.io/badge/GNOME%20Shell-45%2B-blue)](https://www.gnome.org/)

## ✨ Features

- 🎤 **Offline Speech Recognition** - Powered by whisper.cpp, works completely offline
- 🔧 **Highly Configurable** - JSON-based command system with visual preferences UI
- 🎯 **Three Operating Modes**:
  - **Normal Mode**: Listens for hotword activation ("hey" by default)
  - **Command Mode**: Executes configured voice commands automatically
  - **Typing Mode**: Converts speech directly to keyboard input
- 🖥️ **Native GNOME Integration** - Panel icon with real-time status, D-Bus architecture
- ⚡ **GPU Acceleration** - Optional CUDA and Vulkan support for faster processing
- 🔒 **Privacy First** - All processing happens locally on your machine
- 🎨 **Visual Command Builder** - GUI for creating keyboard shortcuts and commands
- 🌊 **Wayland Compatible** - Uses ydotool for reliable input simulation

## 📦 Installation

### Arch Linux (AUR)

```bash
# Clone the repository
git clone https://github.com/Saim20/willow.git
cd willow

# Build and install
makepkg -si

# Enable the extension
gnome-extensions enable willow@saim

# Start the service
systemctl --user start willow.service
```

### Manual Installation

```bash
# Install dependencies (Arch Linux example)
sudo pacman -S gnome-shell sdbus-cpp jsoncpp libpulse ydotool cmake git gcc
# sdbus-c++ 2.x requires a C++20-capable compiler (GCC 10+ or Clang 12+)

# Clone the repository
git clone https://github.com/Saim20/willow.git
cd willow

# Build the service
cd service
mkdir build && cd build
cmake -DCMAKE_INSTALL_PREFIX=/usr ..
make -j$(nproc)
sudo make install

# Install GNOME extension
mkdir -p ~/.local/share/gnome-shell/extensions/
cp -r ../gnome-extension/willow@saim ~/.local/share/gnome-shell/extensions/

# Enable extension
gnome-extensions enable willow@saim

# Start service
systemctl --user start willow.service

# Download whisper model
willow-download-model
```

## 🚀 Getting Started

### Initial Setup

1. **Verify Service Status**
   ```bash
   systemctl --user status willow.service
   ```

2. **Download Whisper Model**
   ```bash
   willow-download-model
   ```
   Choose the tiny.en model (~75MB) for best performance, or larger models for better accuracy.

3. **Enable Auto-start** (Optional)
   ```bash
   systemctl --user enable willow.service
   ```

4. **Configure Preferences**
   - Click the Willow icon in the top panel
   - Select "Preferences"
   - Adjust command threshold, hotword, and processing interval
   - Add custom voice commands

### Usage

The panel icon shows the current mode:
- 🎤 **Microphone** - Normal mode (listening for hotword)
- 🔴 **Red pulsing** - Command mode (processing commands)
- ⌨️ **Keyboard** - Typing mode (speech-to-text)

**Basic Voice Commands**:
- Say "**hey**" to activate command mode
- Say "**typing mode**" to enable dictation
- Say "**stop typing**" to return to normal mode
- Configure custom commands in Preferences

**Smart Workflows**:
- **Open/Launch Apps**: Say "**open [app name]**" or "**launch [app name]**"
  - Examples: "open spotify", "launch firefox", "start discord"
  - Automatically finds and launches apps from your system
- **Web Search**: Say "**search [engine] for [query]**"
  - Examples: "search youtube for tutorials", "search google for recipes"
  - Supported engines: youtube, google, facebook, reddit, wikipedia, github
  - Opens results in your default browser

## ⚙️ Configuration

All settings are managed through the GNOME extension preferences UI, which syncs to `~/.config/willow/config.json`.

### Context Configuration

Willow uses a context file at `~/.config/willow/context.json` to enable smart workflows:

- **Default Apps**: Configure your preferred browser, terminal, file manager, etc.
- **Search Engines**: Define custom search engines with base URLs
- **App Aliases**: Map common app names to their system commands

Example `context.json`:
```json
{
  "default_apps": {
    "browser": "firefox",
    "terminal": "kgx",
    "file_manager": "nautilus"
  },
  "search_engines": {
    "youtube": "https://www.youtube.com/results?search_query=",
    "google": "https://www.google.com/search?q="
  },
  "app_aliases": {
    "spotify": ["spotify"],
    "vscode": ["code", "code-oss", "vscodium"]
  }
}
```

### Main Configuration

The main configuration file is at `~/.config/willow/config.json`:

- **Command Threshold**: Minimum confidence % for command execution (50-100%, default 80%)
- **Processing Interval**: Delay before processing speech (0.5-5.0s, default 1.5s)
- **Hotword**: Activation word (default "hey")
- **GPU Acceleration**: Enable CUDA/Vulkan for faster inference

### Adding Custom Commands

Use the visual command builder in Preferences:
1. Open Preferences → Commands tab
2. Click "Add Command"
3. Enter voice phrase and system command
4. Use the keyboard shortcut builder for complex key combinations

Example commands:
```json
{
  "name": "Open Terminal",
  "command": "systemd-run --user --scope gnome-terminal",
  "phrases": ["open terminal", "launch terminal"]
}
```

## 🎮 Keyboard Shortcuts

Willow uses ydotool for Wayland-compatible keyboard simulation. The visual builder in Preferences generates the correct ydotool commands.

**Example key commands**:
- Ctrl+C: `ydotool key 29:1 46:1 46:0 29:0`
- Alt+Tab: `ydotool key 56:1 15:1 15:0 56:0`

## 🐛 Troubleshooting

### Service won't start
```bash
# Check logs
journalctl --user -u willow.service -f

# Verify D-Bus connection
gdbus introspect --session --dest com.github.saim.Willow --object-path /com/github/saim/VoiceAssistant
```

### ydotool not working
```bash
# Enable and start ydotool service
sudo systemctl enable --now ydotool

# Add user to input group
sudo usermod -aG input $USER
# Log out and back in
```

### Model not found
```bash
# Check model location
ls -lh ~/.local/share/willow/models/

# Download if missing
willow-download-model
```

### Extension not showing
```bash
# Restart GNOME Shell
# On X11: Alt+F2, type 'r', press Enter
# On Wayland: Log out and back in

# Check extension status
gnome-extensions list
gnome-extensions info willow@saim
```

## 📚 Advanced Configuration

For developers and advanced users, the configuration file at `~/.config/willow/config.json` supports:

- Custom command definitions with regex phrase matching
- Multiple whisper model configurations
- Audio capture settings (sample rate, channels)
- Confidence threshold tuning
- Processing interval optimization

See `config.json` in the repository for the full structure.

## 🔧 Development

### Building from Source

```bash
# Clone with the whisper.cpp submodule
git clone --recursive https://github.com/Saim20/willow.git
cd willow

# If you already cloned without submodules:
# git submodule update --init --recursive

# Build service (whisper.cpp is fetched automatically if the submodule is missing)
cd service
mkdir build && cd build
cmake ..
make -j$(nproc)

# Run service directly
./willow-service
```

### GPU Acceleration

**CUDA Support**:
```bash
export ENABLE_CUDA=1
makepkg -si
```

**Vulkan Support**:
```bash
export ENABLE_VULKAN=1
makepkg -si
```

See `docs/GPU_ACCELERATION.md` for detailed setup instructions.

## 🤝 Contributing

Contributions are welcome! Please feel free to submit pull requests or open issues for bugs and feature requests.

## 📄 License

MIT License - see [LICENSE](LICENSE) file for details.

## 🙏 Credits

- **whisper.cpp** - Georgi Gerganov and contributors for the excellent C++ implementation of OpenAI's Whisper
- **GNOME Project** - For the amazing desktop environment
- **sdbus-c++** - For the elegant D-Bus C++ bindings

## 🔗 Links

- [GitHub Repository](https://github.com/Saim20/willow)
- [Issue Tracker](https://github.com/Saim20/willow/issues)
- [whisper.cpp](https://github.com/ggerganov/whisper.cpp)

---
