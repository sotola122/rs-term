# rs-term

[![test](https://github.com/sotola122/rs-term/actions/workflows/test.yml/badge.svg)](https://github.com/sotola122/rs-term/actions/workflows/test.yml)
[![build](https://github.com/sotola122/rs-term/actions/workflows/build.yml/badge.svg)](https://github.com/sotola122/rs-term/actions/workflows/build.yml)

A full-screen serial terminal for Linux and Windows, a Tera Term-style replacement that runs inside your terminal. Built with [ratatui](https://ratatui.rs) and crossterm.

One process is one serial session. There are no built-in panes or tabs: run several instances side by side in tmux, herdr, or Windows Terminal panes and tabs.

## Features

- Port picker. You can also type any path (`/dev/ttyUSB0`, `COM3`), even if it is not listed.
- Shift_JIS (default) and UTF-8 session encodings.
- Own scrollback (10000 lines by default), scrolled with the mouse wheel or PgUp / PgDn. The host terminal's scrollback is not used.
- Mouse selection copies through OSC 52 and, when available, the OS clipboard.
- Bracketed paste.
- Color themes (dark, light, Gruvbox, Tokyo Night, Catppuccin, Dracula, Nord, Solarized and more), switched at run time.
- Text logging with size-based rotation.
- Send a string or a file.
- Send a break.
- Change baud rate and encoding on the fly.
- No dropped bytes under bursts: the port reader hands data over a bounded channel and blocks (backpressure) instead of discarding chunks.
- The terminal is restored on exit and on panic.

## Installation

### Windows installer (MSI)

Each release has a signed per-user MSI, `rs-term-<tag>-x86_64-pc-windows-msvc.msi`. It needs no administrator rights and installs:

- `rs-term.exe`, `LICENSE`, `NOTICE`, `README.md` and `rs-term-codesign.cer` (the public signing certificate) in `%LOCALAPPDATA%\Programs\rs-term`.
- That folder on your **user** `PATH`, so `rs-term` works in new terminals. Uninstalling removes it again.
- A Start menu shortcut named `rs-term`. rs-term is a console app, so Windows 11 opens it in your default terminal application (normally Windows Terminal; change it under Settings > System > For developers > Terminal).

Installing a newer MSI replaces the older one (upgrade). The entry under "Installed apps" shows the publisher `sotola122` and the project URL. The `build` workflow also uploads an **unsigned** MSI as a workflow artifact on every push; it is for testing.

#### Trusting the publisher (optional)

The MSI and `rs-term.exe` are signed with a self-issued certificate, not one from a public authority. Windows therefore shows an unknown publisher (UAC or SmartScreen) until you trust it. The installer works either way. To trust it, download `rs-term-codesign.cer` from the release (compare its thumbprint with the one the maintainer published) and run in PowerShell:

```powershell
Import-Certificate -FilePath rs-term-codesign.cer -CertStoreLocation Cert:\CurrentUser\TrustedPublisher
Import-Certificate -FilePath rs-term-codesign.cer -CertStoreLocation Cert:\CurrentUser\Root
```

- `TrustedPublisher` marks software signed by this certificate as coming from a publisher you accept.
- `Root` makes the certificate a trusted root for your account, so the signature validates as a whole. Windows asks you to confirm this one. Only do it if you trust the certificate, because it applies to anything that chains to it.

Both steps are per user and can be undone by deleting the certificate from those stores.

To check a signature:

```powershell
Get-AuthenticodeSignature .\rs-term-<tag>-x86_64-pc-windows-msvc.msi | Format-List Status, StatusMessage, SignerCertificate
```

Without the certificate in the stores above, `Status` is `UnknownError` (the chain does not end at a trusted root); the signer thumbprint must match the one in `rs-term-codesign.cer`.

### Prebuilt binaries

Pushing a tag that matches `v*` runs the `release` workflow. It runs fmt, clippy and the tests, builds release binaries for `x86_64-pc-windows-msvc` and `x86_64-unknown-linux-gnu`, signs `rs-term.exe`, builds and signs the MSI, and creates a **draft** GitHub release (the workflow does not publish it). The release assets are:

- `rs-term-<tag>-x86_64-pc-windows-msvc.msi` (signed)
- `rs-term-<tag>-x86_64-pc-windows-msvc.exe` (signed)
- `rs-term-codesign.cer`
- `rs-term-<tag>-x86_64-unknown-linux-gnu`
- `LICENSE` and `NOTICE`

Whether a release has been published is up to the maintainer. The `build` workflow also uploads the same binaries (and the unsigned MSI) as workflow artifacts on every push.

### Build from source

The toolchain (Rust 1.95.0, with rustfmt and clippy) is pinned in `rust-toolchain.toml`; rustup selects it automatically. The workspace `rust-version` is 1.95.

```sh
cargo build --locked --release -p rs-term
```

The binary is `target/release/rs-term` (`rs-term.exe` on Windows).

On Linux, building needs `pkg-config` and `libudev-dev`:

```sh
sudo apt install pkg-config libudev-dev
```

## Usage

```text
rs-term [PORT] [options]
```

If `PORT` is omitted, rs-term starts with a port picker. Pick an entry, or type a path directly; paths that are not in the list can be opened.

```sh
rs-term /dev/ttyUSB0 -b 9600
rs-term COM3 -e utf8 --log session.log
rs-term                      # port picker
```

### Options

Defaults come from the built-in values or from `settings.toml` (see [Configuration](#configuration)); options given on the command line win.

| Option | Values | Default | Description |
| --- | --- | --- | --- |
| `-b`, `--baud <N>` | 1 to 4000000 | 115200 | Baud rate |
| `--data <N>` | `5`, `6`, `7`, `8` | `8` | Data bits |
| `--parity <P>` | `none`, `odd`, `even` (also `n`, `o`, `e`) | `none` | Parity |
| `--stop <N>` | `1`, `2` | `1` | Stop bits |
| `--flow <F>` | `none`, `xonxoff`, `rtscts` | `none` | Flow control |
| `--dtr <on\|off>` | `on`, `off` | `off` | DTR level after the port is opened |
| `--rts <on\|off>` | `on`, `off` | `off` | RTS level after the port is opened. Ignored with `--flow rtscts`, where the driver owns RTS |
| `-e`, `--encoding <E>` | `sjis`, `utf8` | `sjis` (Shift_JIS) | Session encoding |
| `--log <PATH>` | file path | none | Start a text log to this file at startup |
| `--prefix <KEY>` | `ctrl-<letter>`, `ctrl-\`, `ctrl-]` | `ctrl-a` | Prefix key (see below) |
| `--ambiguous-width <W>` | `1`, `2`, `auto` | `auto` | Cells taken by ambiguous-width characters (see [Platform notes](#platform-notes)) |
| `--theme <NAME>` | see [Themes](#themes) | `dark` | Color theme |
| `-h`, `--help` | | | Print help and exit |

Values may be given as `--baud 9600`, `--baud=9600` or `-b9600`. `ctrl+a`, `c-a` and `^a` are also accepted spellings of a prefix. Only Ctrl plus a letter, Ctrl+`\` and Ctrl+`]` can be a prefix; other Ctrl combinations cannot be told apart by terminals (for example Ctrl+[ is Esc).

Only one port can be given. Bad arguments are reported on stderr and rs-term exits with code 2.

## Key bindings

The default prefix is Ctrl+A (change it with `--prefix`). Press the prefix, then one of:

| Key | Action |
| --- | --- |
| `c` | Connect |
| `d` | Disconnect |
| `r` | Reconnect |
| `b` | Break: send a break (250 ms) |
| `l` | Log: start a text log (prompts for a path) |
| `L` | Stop log |
| `s` | Send: send a string |
| `f` | File: send a file |
| `u` | Baud: change the baud rate |
| `e` | Encoding: change the encoding (`sjis` or `utf8`) |
| `p` | Paste: paste the OS clipboard (Shift+Insert does the same without the prefix) |
| `t` | Theme: switch to the next theme (`T`: the previous one) |
| `?` | Help: show the help overlay |
| `q` | Quit |
| `Esc` | Cancel: leave the prefix state |
| the prefix again | Send the prefix's control byte (`0x01` for Ctrl+A) to the port |

Notes:

- Ctrl+C does not quit. It is sent to the port as `0x03`. Quit with the prefix, then `q`.
- Scrolling: mouse wheel (3 lines per step) or PgUp / PgDn (a page). This moves rs-term's own scrollback.
  Pressing Enter while scrolled back jumps to the latest output (and sends the newline as usual); other keys and new output do not move the view.
- Selection: drag with the mouse. The selection is copied with OSC 52, and also to the OS clipboard when it can be opened.
- Paste: a bracketed paste from the host terminal is sent to the port as text; prefix then `p`, or Shift+Insert, pastes the OS clipboard. Ctrl+V is not a paste key: it is sent to the port as `0x16`. After any paste the status line shows `Pasted N bytes`, the number of bytes queued for the port in the session encoding (including the newline conversion and, when the device enabled bracketed paste, its markers).
- A log cannot be started while one is running; stop it first with `L`.

## Status line

The bottom row shows, from left to right:

| Item | Meaning |
| --- | --- |
| `PREFIX` | Shown only while the prefix key is armed and waiting for the next key |
| port | The port path, or `no port` (no port selected) |
| frame | Baud rate and framing, e.g. `115200 8N1`; `XON/XOFF` or `RTS/CTS` is appended when flow control is on |
| encoding | `Shift_JIS` or `UTF-8` |
| link state | `disconnected`, `connecting` (opening), `connected`, `lost` (connection lost) |
| log state | `LOG off`, `LOG on` (text log running), `LOG failed`, `LOG bin` (binary log running) |
| `RX n` / `TX n` | Bytes received / sent |
| right side | Right-aligned. While the prefix is armed it lists the commands (`c connect  d disconnect  ... Esc cancel`), or only their keys (`c d r b l L s f u e p t ? q Esc`) when the full list does not fit. If even the keys do not fit beside the left items (for example at 80 columns), the left side shrinks to `PREFIX` and the list gets the rest of the row. Otherwise it is the latest message: info is plain gray, warnings are prefixed `Warning:` (yellow), errors are prefixed `Error:` (red). With no message it shows the hint `<prefix> ? help`, e.g. `Ctrl+A ? help`. Text that does not fit is cut with `...`, or dropped if there is no room. The left items are never cut |

## Configuration

rs-term reads an optional `settings.toml` (schema 1):

| OS | Path |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/rs-term/settings.toml`, or `~/.config/rs-term/settings.toml` if `XDG_CONFIG_HOME` is unset or empty |
| Windows | `%APPDATA%\rs-term\settings.toml` |

Behavior:

- Command-line options override the file; the file overrides the built-in defaults. A missing file is not an error.
- If the file is broken, rs-term starts with the built-in defaults and shows the reason on the status line as an error. The file is never rewritten.
- Every table and key is optional. A key you omit takes its built-in default, so an empty file is valid and a file may contain only the keys you want to change.
- Otherwise the schema is strict. A `schema` other than `1`, an unknown key, a syntax error, or an out-of-range number rejects the whole file.
- An unknown `terminal.palette` rejects the file too; the error lists the valid theme names.
- Legacy keys from older versions are accepted and silently ignored (no message): `serial.reconnect`, `serial.reopen_on_start`, `terminal.font_family`, `terminal.font_size_px`, `terminal.font_fallbacks`, and the whole `[ui]` and `[layout]` tables (any contents).

A complete `settings.toml` that lists every key (these values are the built-in defaults; you can delete any line):

```toml
schema = 1

[serial]
baud_rate = 115200          # 1..=4000000
data_bits = "eight"         # five, six, seven, eight
parity = "none"             # none, odd, even
stop_bits = "one"           # one, two
flow = "none"               # none, xonxoff, rtscts
dtr = "off"                 # on, off: DTR level after opening
rts = "off"                 # on, off: RTS level after opening (ignored with rtscts)
read_timeout_ms = 20        # 1..=100

[newline]
receive = "auto"            # cr, crlf, lf, auto
transmit = "cr"             # cr, crlf, lf

[encoding]
session = "shift_jis"       # shift_jis, utf-8

[terminal]
columns = 80                # 1..=500
rows = 24                   # 1..=500
scrollback = 10000          # 0..=100000
local_echo = false
palette = "dark"            # a theme name, see Themes
ambiguous_width = "auto"    # 1, 2, auto (see Platform notes)

[log]
directory = ""              # empty: platform default (see Logs)
name_template = "{port}_{date}_{time}.log"
rotate_bytes = 10485760     # 64 KiB..=1 GiB
rotate_keep = 5             # 1..=20
text_encoding = "utf-8"     # utf-8 only
```


## Themes

A theme sets the default text and background colors, the 16 ANSI colors, the cursor, the selection, the status line and the boxes (help, dialogs). Colors that the device sends as 256-color or truecolor attributes are drawn as sent; only the default colors and the 16 named colors follow the theme.

Choose one with `--theme <NAME>` or `palette` under `[terminal]` in `settings.toml` (the option wins). Names are not case sensitive:

`dark, light, gruvbox-dark, gruvbox-light, tokyo-night, tokyo-night-storm, tokyo-night-light, catppuccin-mocha, catppuccin-latte, dracula, nord, solarized-dark, solarized-light, one-dark, one-light, kanagawa, kanagawa-lotus, rose-pine, rose-pine-dawn, vesper`

`classic-dark` is accepted as another spelling of `dark`. The Gruvbox, Catppuccin, Solarized and Tokyo Night themes also answer to `gruvbox`, `catppuccin`, `solarized`, `tokyo-night-day` and so on. Press the prefix, then `t` to switch to the next theme in this order (`T` for the previous one); the status line shows `Theme: <name>`. The switch is not saved: the next start uses the option or the file again.

## Logs

Prefix then `l` asks for a path, pre-filled with the default directory and file name. `--log PATH` starts a text log at startup. Text logs are UTF-8.

Default log directory (used when `log.directory` is empty):

| OS | Directory |
| --- | --- |
| Linux | `$XDG_STATE_HOME/rs-term/logs`, or `~/.local/state/rs-term/logs` |
| Windows | `%LOCALAPPDATA%\rs-term\logs` |

If the needed environment variables are not set, the fallback is `rs-term/logs` under the system temporary directory.

The default file name template is `{port}_{date}_{time}.log`:

- `{port}`: the last path component of the port (`ttyUSB0`, `COM3`), or `rs-term` when no port is selected.
- `{date}`: local date as `YYYYMMDD`.
- `{time}`: local time as `HHMMSS`.

Example: `ttyUSB0_20260101_093000.log`. While a log is open, a `<file>.lock` file next to it holds an OS file lock; a leftover lock file after a crash does not block reuse. Logs rotate at `rotate_bytes`, keeping `rotate_keep` files.

## Platform notes

- Use Windows Terminal or another VT-capable console host (modern conhost). The app relies on the alternate screen, mouse capture and bracketed paste.
- On Windows, a port held by another process is reported as in use (`Port is busy (opened by another process)`). Linux reports the same message for a busy or locked port.
- DTR and RTS: rs-term sets both lines to the configured levels (`--dtr`, `--rts`, `dtr` and `rts` under `[serial]`; both off by default) right after opening the port, on every OS, and applies the same levels again on reconnect and after a baud or frame change. With `--flow rtscts` RTS is left to the driver. On Linux the kernel raises DTR and RTS while it opens the tty, so a very short pulse can still occur before rs-term lowers them; a device that resets on DTR may notice it.
- Ambiguous-width characters (for example `①`, `°`, `×`, `α`, `★`) are drawn two cells wide by the Windows console host (conhost) when its output code page is a CJK one (932, 936, 949 or 950), and one cell wide elsewhere. `--ambiguous-width` (or `ambiguous_width` under `[terminal]` in `settings.toml`) tells rs-term which to assume: `1` or `2` forces it, and `auto` (the default) means two cells only on Windows, when the console has a visible window of its own (a real conhost window; Windows Terminal, VS Code and Cursor use a hidden pseudo-console window) and the output code page is one of those. Environment variables such as `WT_SESSION` are not consulted, because a conhost window started from another terminal inherits them. On Linux `auto` means one cell. Box drawing and block elements (U+2500 to U+259F) always stay one cell. Copying and logs keep the real characters.
- CI builds and tests Linux and Windows only.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

The `test` workflow runs these three commands on `windows-latest` and `ubuntu-latest` for every push and pull request. The `build` workflow builds release binaries for both targets and the unsigned MSI on every push, and checks the signing script with a throwaway certificate.

Workspace layout:

- `crates/rs-term-core`: serial I/O, session, terminal screen, codecs, logging and settings. No UI code.
- `crates/rs-term`: the ratatui/crossterm frontend (command line, keys, status line, view).

### Maintainer notes: code signing and the MSI

The `release` workflow fails unless two repository secrets exist. Create a code-signing certificate as a PFX (for example with `New-SelfSignedCertificate -Type CodeSigningCert` and `Export-PfxCertificate`), then:

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes('codesign.pfx')) | gh secret set WINDOWS_SIGN_PFX_BASE64
gh secret set WINDOWS_SIGN_PFX_PASSWORD
```

`.github/scripts/sign.ps1` decodes the PFX to a temporary file, signs with `signtool` (SHA-256, timestamp server `http://timestamp.digicert.com`, override with `SIGN_TIMESTAMP_URL`), exports the public certificate as `rs-term-codesign.cer`, and deletes the temporary PFX. Keep the same certificate across releases so the trust steps above are done once. The MSI source is `installer/rs-term.wxs` (WiX Toolset 5.0.2, installed with `dotnet tool install --global wix --version 5.0.2`); its `UpgradeCode` must never change. `.github/scripts/build-msi.ps1` builds it, taking the version from `crates/rs-term/Cargo.toml`.

## License

Apache-2.0. Copyright 2026 sotola122. See [LICENSE](LICENSE).

Third-party notices are in [NOTICE](NOTICE). Among them, serialport is licensed under MPL-2.0 and is used unmodified.
