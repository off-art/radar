# Installation

[← README](../../README.en.md) · [Русский](../ru/install.md)

No brew, no Rust, no admin rights needed. Supported: macOS (Apple Silicon and Intel) and Linux (`x86_64`, `aarch64`).

## Method 1: one command (recommended)

```sh
curl -fsSL https://raw.githubusercontent.com/off-art/radar/main/install.sh | bash
```

The script downloads a prebuilt binary for your CPU into `~/.local/bin` and tells you what to add to `PATH`.
Files fetched with `curl` are not quarantined by macOS.

## Method 2: manual archive

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

## Method 3: from source

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

If GitHub is unreachable from your network, download `radar-x86_64-unknown-linux-gnu.tar.gz` (or `aarch64`) from Releases and:

```sh
tar xzf radar-x86_64-unknown-linux-gnu.tar.gz
mkdir -p ~/.local/bin && rm -f ~/.local/bin/radar && mv radar ~/.local/bin/
```

Notifications and sound need `libnotify-bin` (`notify-send`) and `pulseaudio-utils` (`paplay`) or `alsa-utils` (`aplay`):
`sudo apt install libnotify-bin pulseaudio-utils`. Mouse copy goes to the system clipboard via `wl-copy` (Wayland) or
`xclip`/`xsel` (X11) if installed, otherwise via OSC 52 (your terminal must support it).
From source: `sudo apt install build-essential curl git`, then Rust and `./install.sh`.

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
rm ~/.local/bin/radar
rm -rf ~/.config/radar ~/.radar     # settings, agent list, notification helper, git worktrees (optional)
```
