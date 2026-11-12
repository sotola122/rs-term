# rs-term

[![test](https://github.com/sotola122/rs-term/actions/workflows/test.yml/badge.svg)](https://github.com/sotola122/rs-term/actions/workflows/test.yml)
[![build](https://github.com/sotola122/rs-term/actions/workflows/build.yml/badge.svg)](https://github.com/sotola122/rs-term/actions/workflows/build.yml)

A full-screen serial terminal for Linux and Windows, a Tera Term-style replacement that runs inside your terminal. Built with [ratatui](https://ratatui.rs) and crossterm.

One process is one serial session. There are no built-in panes or tabs: run several instances side by side in tmux, herdr, or Windows Terminal panes and tabs.

Note: the user interface (status line, prompts, help, error messages, `--help`) is in Japanese.

## Features

- Port picker. You can also type any path (`/dev/ttyUSB0`, `COM3`), even if it is not listed.
- Shift_JIS (default) and UTF-8 session encodings.
- Own scrollback (10000 lines by default), scrolled with the mouse wheel or PgUp / PgDn. The host terminal's scrollback is not used.
- Mouse selection copies through OSC 52 and, when available, the OS clipboard.
- Bracketed paste.
- Text logging with size-based rotation.
- Send a string or a file.
- Send a break.
- Change baud rate and encoding on the fly.
- No dropped bytes under bursts: the port reader hands data over a bounded channel and blocks (backpressure) instead of discarding chunks.
- The terminal is restored on exit and on panic.

## Installation

### Prebuilt binaries

Pushing a tag that matches `v*` runs the `release` workflow. It runs fmt, clippy and the tests, builds release binaries for `x86_64-pc-windows-msvc` and `x86_64-unknown-linux-gnu`, and creates a **draft** GitHub release (the workflow does not publish it). The release assets are:

- `rs-term-<tag>-x86_64-pc-windows-msvc.exe`
- `rs-term-<tag>-x86_64-unknown-linux-gnu`
- `LICENSE` and `NOTICE`

Whether a release has been published is up to the maintainer. The `build` workflow also uploads the same binaries as workflow artifacts on every push.

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
| `-e`, `--encoding <E>` | `sjis`, `utf8` | `sjis` (Shift_JIS) | Session encoding |
| `--log <PATH>` | file path | none | Start a text log to this file at startup |
| `--prefix <KEY>` | `ctrl-<letter>`, `ctrl-\`, `ctrl-]` | `ctrl-a` | Prefix key (see below) |
| `-h`, `--help` | | | Print help and exit |

Values may be given as `--baud 9600`, `--baud=9600` or `-b9600`. `ctrl+a`, `c-a` and `^a` are also accepted spellings of a prefix. Only Ctrl plus a letter, Ctrl+`\` and Ctrl+`]` can be a prefix; other Ctrl combinations cannot be told apart by terminals (for example Ctrl+[ is Esc).

Only one port can be given. Bad arguments are reported on stderr and rs-term exits with code 2.

## Key bindings

The default prefix is Ctrl+A. Press the prefix, then one of:

| Key | Action |
| --- | --- |
| `c` | Connect |
| `d` | Disconnect |
| `r` | Reconnect |
| `b` | Send a break (250 ms) |
| `l` | Start a text log (prompts for a path) |
| `L` | Stop the log |
| `s` | Send a string |
| `f` | Send a file |
| `u` | Change the baud rate |
| `e` | Change the encoding (`sjis` or `utf8`) |
| `p` | Paste the OS clipboard |
| `?` | Help |
| `q` | Quit |
| `Esc` | Cancel |
| the prefix again | Send the prefix's control byte (`0x01` for Ctrl+A) to the port |

Notes:

- Ctrl+C does not quit. It is sent to the port as `0x03`. Quit with the prefix, then `q`.
- Scrolling: mouse wheel (3 lines per step) or PgUp / PgDn (a page). This moves rs-term's own scrollback.
- Selection: drag with the mouse. The selection is copied with OSC 52, and also to the OS clipboard when it can be opened.
- Paste: a bracketed paste from the host terminal is sent to the port as text; prefix then `p` pastes the OS clipboard.
- A log cannot be started while one is running; stop it first with `L`.

## Status line

The bottom row shows, from left to right:

| Item | Meaning |
| --- | --- |
| `PREFIX` | Shown only while the prefix key is armed and waiting for the next key |
| port | The port path, or `未選択` (no port selected) |
| frame | Baud rate and framing, e.g. `115200 8N1`; `XON/XOFF` or `RTS/CTS` is appended when flow control is on |
| encoding | `Shift_JIS` or `UTF-8` |
| link state | `未接続` (disconnected), `接続中` (opening), `接続` (connected), `喪失` (connection lost) |
| log state | `LOG off`, `LOG on` (text log running), `LOG 失敗` (failed), `LOG bin` (binary log running) |
| `RX n` / `TX n` | Bytes received / sent |
| message | Right-aligned. Info is plain gray, warnings are prefixed `注意:` (yellow), errors are prefixed `エラー:` (red). A message that does not fit is cut with `…`, or dropped if there is no room |

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
- Exception: an unknown `terminal.palette` falls back to `classic-dark` with a warning, and the rest of the file still loads.
- Legacy keys from older versions are accepted and ignored, with a warning on the status line (`使わない設定を無視した: ui, serial.reconnect`): `serial.reconnect`, `serial.reopen_on_start`, `terminal.font_family`, `terminal.font_size_px`, `terminal.font_fallbacks`, and the whole `[ui]` table.

A complete `settings.toml` that lists every key (these values are the built-in defaults; you can delete any line):

```toml
schema = 1

[serial]
baud_rate = 115200          # 1..=4000000
data_bits = "eight"         # five, six, seven, eight
parity = "none"             # none, odd, even
stop_bits = "one"           # one, two
flow = "none"               # none, xonxoff, rtscts
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
palette = "classic-dark"    # classic-dark, light

[log]
directory = ""              # empty: platform default (see Logs)
name_template = "{port}_{date}_{time}.log"
rotate_bytes = 10485760     # 64 KiB..=1 GiB
rotate_keep = 5             # 1..=20
text_encoding = "utf-8"     # utf-8 only
```


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
- On Windows, a port held by another process is reported as in use (`ポートは使用中（他のプロセスが開いている）`). Linux reports the same message for a busy or locked port.
- CI builds and tests Linux and Windows only.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

The `test` workflow runs these three commands on `windows-latest` and `ubuntu-latest` for every push and pull request. The `build` workflow builds release binaries for both targets on every push.

Workspace layout:

- `crates/rs-term-core`: serial I/O, session, terminal screen, codecs, logging and settings. No UI code.
- `crates/rs-term`: the ratatui/crossterm frontend (command line, keys, status line, view).

## License

Apache-2.0. Copyright 2026 sotola122. See [LICENSE](LICENSE).

Third-party notices are in [NOTICE](NOTICE). Among them, serialport is licensed under MPL-2.0 and is used unmodified.
