# Installation

[← README](../../README.en.md) · [Русский](../ru/install.md)

No Rust, no admin rights needed. Supported: macOS (Apple Silicon and Intel), Linux (`x86_64`, `aarch64`) and Windows 10/11 x64 (preview).

## Method 1: one command (recommended)

```sh
curl -fsSL https://raw.githubusercontent.com/off-art/radar/main/install.sh | bash
```

The script downloads a prebuilt binary for your CPU into `~/.local/bin` and tells you what to add to `PATH`.
Files fetched with `curl` are not quarantined by macOS.

## Method 2: Homebrew

```sh
brew install off-art/radar/radar
brew upgrade off-art/radar/radar   # update
```

Use the full name: the short `brew install radar` installs a different app (a menu-bar Radar from homebrew-cask). Works on macOS and Linuxbrew. For such an install `radar update` prints the `brew upgrade off-art/radar/radar` hint.

## Method 3: manual archive

1. On the [Releases](https://github.com/off-art/radar/releases/latest) page download `radar-aarch64-apple-darwin.tar.gz`
   (Apple Silicon: M1/M2/M3…) or `radar-x86_64-apple-darwin.tar.gz` (Intel). Check your CPU with `uname -m` (`arm64` = Apple Silicon, `x86_64` = Intel).
2. Unpack and move it to `~/.local/bin`:
   ```sh
   cd ~/Downloads
   tar xzf radar-aarch64-apple-darwin.tar.gz        # file name as downloaded
   mkdir -p ~/.local/bin && mv radar ~/.local/bin/
   xattr -d com.apple.quarantine ~/.local/bin/radar # removes the browser quarantine flag
   ```
3. If `~/.local/bin` is not in `PATH`:
   ```sh
   echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc && source ~/.zshrc
   ```

Without `xattr`, macOS says it cannot verify the developer: go to *System Settings → Privacy & Security → "Open Anyway"*.
The same archive can be sent to a colleague over a messenger.

## Method 4: from source

Rust is required. Without brew:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh     # then restart the terminal
git clone https://github.com/off-art/radar.git
cd radar
./install.sh                                                        # builds and installs to ~/.local/bin
```

The first build takes about two minutes. Alternative: `cargo install --path .` (installs to `~/.cargo/bin`).

## Linux (Debian and others)

Prebuilt binaries exist for `x86_64` and `aarch64` (glibc 2.35+: Debian 12/13, Ubuntu 22.04+). Verified on Debian 13 (aarch64, container):
install, `radar update`, background sessions, `radar stop`. Install and update work the same way as above.

### .deb package (Debian, Ubuntu, Linux Mint, SberOS)

```sh
curl -fsSLO https://github.com/off-art/radar/releases/latest/download/radar_$(dpkg --print-architecture).deb
sudo apt install ./radar_$(dpkg --print-architecture).deb
```

Installs `/usr/bin/radar` and pulls the recommended `libnotify-bin`, `pulseaudio-utils`, `xclip` (notifications, sound, clipboard). The architecture is detected automatically (`amd64` or `arm64`).
To update, run the same two commands; `radar update` prints them for such an install. To remove: `sudo apt remove radar`.

If GitHub is unreachable from your network, download `radar-x86_64-unknown-linux-gnu.tar.gz` (or `aarch64`) from Releases and:

```sh
tar xzf radar-x86_64-unknown-linux-gnu.tar.gz
mkdir -p ~/.local/bin && rm -f ~/.local/bin/radar && mv radar ~/.local/bin/
```

Notifications and sound need `libnotify-bin` (`notify-send`) and `pulseaudio-utils` (`paplay`) or `alsa-utils` (`aplay`):
`sudo apt install libnotify-bin pulseaudio-utils`. Mouse copy goes to the system clipboard via `wl-copy` (Wayland) or
`xclip`/`xsel` (X11) if installed, otherwise via OSC 52 (your terminal must support it).
From source: `sudo apt install build-essential curl git`, then Rust and `./install.sh`.

## Windows 10/11 (preview)

In PowerShell, no admin rights needed:

```powershell
irm https://raw.githubusercontent.com/off-art/radar/main/install.ps1 | iex
```

The script puts `radar.exe` into `%LOCALAPPDATA%\radar` and adds the folder to `PATH` (visible in new PowerShell windows). Run Radar in Windows Terminal. Pre-release builds: set `$env:RADAR_VERSION = 'v0.7.0-win.1'` before the command above.

- Agents start through PowerShell (`pwsh`, or the built-in `powershell` if it is missing) and are found on `PATH`, including npm `.cmd` wrappers.
- Sound uses the system WAV player; notifications are balloon tips from the tray icon.
- Update: `radar update`. Uninstall: delete `%LOCALAPPDATA%\radar` and the config `%USERPROFILE%\.config\radar`.
- Windows support is a preview: the main flows are verified with OpenCode, other agents are still being checked.

## After installing

```sh
radar doctor        # which agents were found
radar notify-test   # notification check: icon, sounds, a test notification
radar               # run
```

On the first notification macOS asks for permission for "Radar": click Allow (or enable it in *System Settings → Notifications → Radar*).

## Updating

Settings (`~/.config/radar/`), saved agents and integrations are kept. Close all Radar windows first.

```sh
radar update            # downloads the latest version and replaces itself
radar update --check    # only check whether a newer version exists
```

For a brew install `radar update` prints `brew upgrade off-art/radar/radar`; for a `.deb` it prints the package download commands.

If you are on 0.3.0 or older, `radar update` does not exist yet: update once by re-running the install command (method 1); after that `radar update` is enough.

Manually (archive): **delete** the old binary first, then put the new one in place:

```sh
rm ~/.local/bin/radar && mv radar ~/.local/bin/ && xattr -d com.apple.quarantine ~/.local/bin/radar
radar --version
```

> On Apple Silicon do not overwrite the binary with `cp`: macOS will kill the process (`killed`).
> `install.sh` does it correctly (removes, writes a new file, signs it).

## Uninstalling

```sh
radar stop                          # stop agents running in the background
radar integration uninstall all     # if you enabled integrations: removes Radar hooks from agent configs
rm ~/.local/bin/radar               # .deb: sudo apt remove radar; brew: brew uninstall off-art/radar/radar
rm -rf ~/.config/radar ~/.radar     # settings, agent list, notification helper, git worktrees (optional)
```
