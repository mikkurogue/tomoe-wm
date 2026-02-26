# Tomoe Window Manager

A Wayland compositor written in Rust using the Smithay library, featuring a niri-style scrolling tiling layout.

## Features

- Scrolling horizontal tiling layout (similar to niri)
- Layer shell support (waybar, wofi, etc.)
- Configurable keybinds
- XDG shell support
- DMA-BUF support

## Building

### Dependencies

**Ubuntu/Debian:**
```bash
sudo apt-get install -y \
    build-essential \
    libxkbcommon-dev \
    libudev-dev \
    libinput-dev \
    libgbm-dev \
    libseat-dev \
    libwayland-dev
```

**Arch Linux:**
```bash
sudo pacman -S \
    base-devel \
    libxkbcommon \
    systemd-libs \
    libinput \
    mesa \
    seatd \
    wayland
```

**Fedora:**
```bash
sudo dnf install -y \
    gcc \
    libxkbcommon-devel \
    systemd-devel \
    libinput-devel \
    mesa-libgbm-devel \
    libseat-devel \
    wayland-devel
```

### Build

```bash
cargo build --release
```

The binary will be at `target/release/tomoe-wm`.

## Running

### Nested Mode (Development/Testing)

The easiest way to test Tomoe is to run it inside an existing Wayland compositor (like GNOME, KDE Plasma, Sway, etc.):

```bash
# From within another Wayland session
cargo run

# Or with debug logging
RUST_LOG=debug cargo run
```

This will open a window running Tomoe as a nested compositor. Applications launched from within this window will use Tomoe as their compositor.

### Real Environment (TTY/Session)

> **Note:** Currently, Tomoe only supports the winit backend for nested testing. To run as a standalone compositor from a TTY, you would need to add DRM/libinput backends. This is planned for future development.

To test Tomoe as your actual desktop environment (once DRM backend is implemented):

1. Switch to a TTY (Ctrl+Alt+F3 or similar)
2. Log in
3. Run:
   ```bash
   /path/to/tomoe-wm
   ```

Or create a desktop session entry:

**`/usr/share/wayland-sessions/tomoe.desktop`:**
```ini
[Desktop Entry]
Name=Tomoe
Comment=Tomoe Wayland Compositor
Exec=/path/to/tomoe-wm
Type=Application
```

Then select "Tomoe" from your display manager's session menu.

## Configuration

Configuration is stored at `~/.config/tomoe/config.toml`. A default configuration is created on first run.

### Example Configuration

```toml
[general]
gap = 8        # Gap between windows
margin = 8     # Margin from screen edges

[keyboard]
layout = "us"  # Keyboard layout
# variant = ""
# options = "ctrl:nocaps"
repeat_delay = 200
repeat_rate = 25

[tiling]
default_window_width = 0.5  # Windows take 50% of screen width
scrolling = true

[keybinds]
# Format: "Modifier+Key" = { action = "...", ... }
"Super+Ctrl+Return" = { Spawn = { command = "alacritty" } }
"Super+Ctrl+d" = { Spawn = { command = "wofi --show drun" } }
"Super+Ctrl+q" = "Close"
"Super+Ctrl+h" = "FocusPrev"
"Super+Ctrl+l" = "FocusNext"
"Super+Ctrl+Left" = "ScrollLeft"
"Super+Ctrl+Right" = "ScrollRight"
"Super+Ctrl+f" = "Fullscreen"
"Super+Ctrl+Shift+e" = "Quit"

# Startup commands
on_start = [
    "waybar",
    # "swaybg -i /path/to/wallpaper.png",
]
```

### Keybind Actions

| Action | Description |
|--------|-------------|
| `Spawn { command = "..." }` | Run a command |
| `Close` | Close focused window |
| `FocusNext` | Focus next window |
| `FocusPrev` | Focus previous window |
| `ScrollLeft` | Scroll view left |
| `ScrollRight` | Scroll view right |
| `Fullscreen` | Toggle fullscreen |
| `Quit` | Exit the compositor |

## Using with Status Bars and Launchers

Tomoe supports the wlr-layer-shell protocol, so it works with:

- **waybar** - Status bar
- **wofi** - Application launcher (keyboard input works!)
- **mako** / **dunst** - Notification daemons
- **swaylock** - Screen locker
- **swaybg** - Background setter

### Waybar

Waybar will automatically use exclusive zones, and Tomoe will position windows below the bar.

### Wofi

Launch wofi with your keybind. The compositor properly handles keyboard focus for layer shell surfaces.

## Development

### Running Tests

```bash
cargo test
```

### Debug Logging

```bash
RUST_LOG=debug cargo run
RUST_LOG=tomoe_wm=trace cargo run  # More verbose
```
## License

MIT
