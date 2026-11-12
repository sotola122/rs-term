# rs-term

著作権 2026 sotola122。Apache-2.0。

全画面のシリアル端末です。1 つのプロセスが 1 つのポートを開きます。ペインやタブは持ちません。複数の接続は herdr、tmux、Windows Terminal で rs-term を並べて使います。

設計は `docs/design-v7.md` です。`docs/design-v6.md` は GUI 版の履歴です。

## 起動

```text
rs-term [PORT] -b BAUD --data 8 --parity none --stop 1 --flow none -e sjis|utf8 --log PATH
```

`PORT` を省くと、画面の中でポートを選ぶか、`/dev/ttyUSB0` や `COM3` のようなパスを直接入力します。一覧に無いパスも開けます。

既定は 115200 8N1、フロー無し、Shift_JIS です。同じディレクトリの決め方で `settings.toml`（schema 1）があれば、その値を既定にし、コマンドラインが上書きします。

| OS | 設定ファイル | ログの既定ディレクトリ |
| --- | --- | --- |
| Linux | `$XDG_CONFIG_HOME/rs-term/settings.toml`（無ければ `~/.config/rs-term/`） | `$XDG_STATE_HOME/rs-term/logs`（無ければ `~/.local/state/rs-term/logs`） |
| Windows | `%APPDATA%\rs-term\settings.toml` | `%LOCALAPPDATA%\rs-term\logs` |

どれも決まらないときは、ログは一時ディレクトリの `rs-term/logs` に落ちます。

## 操作

プレフィックスは既定で Ctrl+A です。`--prefix ctrl-b` のように変えられます。プレフィックスをもう一度押すと、その制御バイト（Ctrl+A なら `0x01`）をポートへ送ります。

| 次のキー | 動作 |
| --- | --- |
| `c` / `d` / `r` | 接続 / 切断 / 再接続 |
| `b` | ブレーク（250 ms） |
| `l` / `L` | ログ開始 / ログ停止 |
| `s` / `f` | 文字列を送る / ファイルを送る |
| `u` / `e` | ボーレート / エンコーディング |
| `p` | クリップボードを貼る |
| `?` | ヘルプ |
| `q` | 終了 |
| Esc | 取消 |

Ctrl+C は終了ではありません。`0x03` としてポートへ送ります。終了はプレフィックスの次の `q` です。

ホイールと PgUp / PgDn は、このプロセスのスクロールバックを動かします。Shift の有無は同じです。ホスト端末のスクロールは使いません。ドラッグで選択すると OSC 52 でコピーし、OS のクリップボードも使えるときはそこへも書きます。

下端の 1 行が状態です。ポート、フレーム（`9600 8N1`）、エンコーディング、接続、ログ、RX/TX バイト数、そしてエラーを日本語で出します。

## ビルド

```text
cargo build --locked -p rs-term
```

Linux では `pkg-config` と `libudev-dev` が要ります。
