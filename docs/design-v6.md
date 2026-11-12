# rs-term 設計 v6

日付は 2026-11-10。対象はリポジトリ `sotola122/rs-term` のブランチ `fix/rs-term-v1-herdr`。著作権者は sotola122（`LICENSE` 付録は `Copyright 2026 sotola122`）。

この文書は v5（`rs-term-design-v5.md`、D-261110-2）を置き換える。単独で読めるように、v5 から残す内容も本文に書き直した。画面の寸法、色、文言、ワイヤーフレームは `docs/screen-design-v6.md` と `docs/wireframes/*.svg` にある。両方を合わせて設計の正とする。コードと設計が食い違えば、設計に合わせてコードを直す。

識別子は英語、説明は日本語である。外部のソースを根拠にした文には `ファイル:行` を付けた。herdr は `/workspace/herdr`（コミット `ee5bfb5`）、`gpui-component` は 0.9.0、`serialport` は 4.10.1 の crates.io ソースを読んだ。実行していないことは「未実行」と書く。

## 0. この版で何が変わったか（要約）

1. ペインの動作とキーは herdr の既定どおりにした。プレフィックスは `Ctrl+B`。アプリ内の状態機械で herdr のプレフィックスモードを再現する（§3）。
2. 分割の名前は herdr の `split_vertical`（右に分割、左右に並ぶ）と `split_horizontal`（下に分割、上下に並ぶ）に統一した。アクション ID、表示名、`layout.toml`、キーが 1 つの語彙になる（§3.3）。
3. 比率の範囲は herdr と同じ 0.1〜0.9 にした。v5 の 50〜950 は捨てた（§3.6）。
4. 状態行は各ペインの下端に 1 行。エラーは全部そこに日本語で出す。`Workspace.status` のような描かれない文字列は禁止する（§5）。
5. パレットのページ遷移、検索欄のフォーカス、クエリの消去、Escape の段階を固定した（§6）。
6. 接続の手順はモック s3 どおり「ポート → ボーレート → フレーム → フロー → DTR」の 5 ページにした。フレームは 1 ページで「8N1」などの組を選ぶ（§6.4）。
7. コアの 3 コミット（`afec483`、`d19bef5`、`7c390ba`）を前提にし、UI クレートを作り直す実装計画にした（§11）。
8. レビュー（R01〜R58、N1〜N31）の全項目をフェーズか「対応しない＋理由」に割り当てた（§12）。

## 1. 変更履歴

### D-261110-6（2026-11-10、v6）

拘束要件（変えてはならない）は次のとおりである。

- ペインの動作とペインのキーは herdr に正確に従う。herdr が TUI のプレフィックスを使うところは、もっとも忠実な GUI の等価物を選び、herdr の `ファイル:行` で理由を書く。別の方式を発明しない。
- タブバー、メニュー、設定画面を出さない。すべての操作はコマンドパレット（`gpui_component::command`、`Ctrl+Shift+P`）から行う。状態行は各ペインの下端に 1 行。ウィンドウは 1 つで、ペインは再帰的に分割する。エラーは全部その状態行に日本語で出す。著作権者は sotola122。
- 見た目は `/workspace/rs-term-mock/s1..s4.png` と `.html` である。

v5 からの変更点と理由は §13 の表にまとめた。

### それ以前

D-261109-8（Windows 優先、`gpui-kit` 0.9.0）、D-261109-9（gpt-6.1-sol レビュー取込み）、D-261110-2（v4、v5）。v5 までの決定事項のうち v6 で残すものは §2 に再掲した。

## 2. 決定事項（v6 で有効なもの）

| 項目 | 決定 | 出所 |
| --- | --- | --- |
| 著作権 | `Copyright 2026 sotola122`。アプリは Apache-2.0。`serialport` の MPL-2.0 はそのまま依存 | 要件（v4） |
| 最初の OS | Windows 10 21H2 x64 / Windows 11 x64。macOS と Linux は二次 | 要件（v3） |
| UI 依存 | `gpui-kit = "=0.9.0"`（`gpui-pre` `=0.3.11`）。`gpui-fast` は付けない | 要件（v3） |
| ツールチェーン | `rust-toolchain.toml` の 1.95.0 を唯一の版とする。ワークスペースの `rust-version` も 1.95。CI はこのファイルを読む | v6（R34、`afec483` の事実） |
| ウィンドウ | 1 つ。追加の接続はペイン | 要件（v4） |
| 操作の入口 | パレットだけ。タブ、メニュー、設定画面、ダイアログは出さない | 要件（v5） |
| ペイン | herdr の既定キーと動作（§3） | 要件（v6） |
| 状態行 | 各ペイン下端に 1 行。全エラーを日本語で | 要件（v6） |
| 曖昧幅 | 1 セル。`width_cjk` は使わない | 要件（v4） |
| ISO-2022-JP | v1 に入れない | 設計（v4） |
| DTR | 既定オフ（`dtr_on_open = None`）。オンは `Some(true)` | 設計（v4） |
| ブレーク保持 | 250 ms | 設計（v4） |

## 3. ペイン：herdr → rs-term

### 3.1 プレフィックスの GUI 等価物

herdr の既定プレフィックスは `ctrl+b` である（`src/config/model.rs:1123`）。理由は、端末の中のプログラムがほとんどのキーを使うので、多重化側は 1 つのキーだけを予約する、である（`docs/next/website/src/content/docs/keyboard.mdx:14-16`）。rs-term の中の「プログラム」はシリアルの対向装置であり、Ctrl+英字を制御コードとして受け取る。理由はそのまま当てはまる。したがって GUI でもプレフィックス方式を残す。これがもっとも忠実な等価物である。

再現する意味論は herdr の `src/client/shell/input.rs:686-718`（`ClientShellMode::Prefix`）である。

| プレフィックスの次に押したキー | herdr | rs-term |
| --- | --- | --- |
| プレフィックス自身（`Ctrl+B`） | ペインへプレフィックスを送り、通常に戻る（`input.rs:695-699`） | `0x02` をフォーカス中のセッションへ送る |
| `Esc` | 取消して通常に戻る（`input.rs:700-704`） | 同じ。何も送らない |
| 束縛されたキー | そのアクションを実行して通常に戻る（`input.rs:705-712`） | 同じ |
| それ以外 | 捨てて通常に戻る（`input.rs:713-715`） | 同じ。ポートへ送らない |
| 時間切れ | 無い | 無い |

GPUI の複数打鍵バインド（`"ctrl-b v"`）は使わない。GPUI の保留打鍵には時間切れがあり、一致しなかった打鍵は通常の入力として再配送される。どちらも herdr の「時間切れ無し、未束縛は捨てる」と違い、未束縛キーが COM へ流れる。そこで UI クレートに `KeyMode` の状態機械を自前で持つ（§7.2）。パレットの行の右に出すキー表示も、この自前の表で作る（§6.2）。

プレフィックス中は、フォーカス中ペインの状態行の左端に `PREFIX` バッジとヒントを出す。herdr は下端の 1 行に ` PREFIX ` とヒントを出す（`src/client/shell/render.rs:86-97`）。

### 3.2 対応表

既定キーは herdr の `KeybindsConfig` の `Default`（`src/config/model.rs:1123-1181`）と同じである。「prefix+x」は「`Ctrl+B` を離して `x`」を表す。

| herdr のアクション | herdr 既定（model.rs 行） | rs-term アクション ID | 表示名 | rs-term 既定 | 動作の出所 |
| --- | --- | --- | --- | --- | --- |
| `prefix` | `ctrl+b`（1123） | — | — | `Ctrl+B` | §3.1 |
| `help` | `prefix+?`（1124） | `palette.toggle` | コマンドパレット | `prefix+?` と `Ctrl+Shift+P` | §3.4 |
| `settings` | `prefix+s`（1125） | `palette.toggle` | 同上 | `prefix+s` | §3.4 |
| `split_vertical` | `prefix+v`（1173） | `pane.split_vertical` | 右に分割 | `prefix+v` | `client/shell/actions.rs:1047-1062`（`SplitDirection::Right`、`focus: true`） |
| `split_horizontal` | `prefix+minus`（1174） | `pane.split_horizontal` | 下に分割 | `prefix+-` | 同上（`SplitDirection::Down`） |
| `focus_pane_left/down/up/right` | `prefix+h/j/k/l`（1162-1165） | `pane.focus_left` ほか | 左／下／上／右のペインへ | `prefix+h/j/k/l` | `app/api/panes.rs:663-717`、`layout.rs:369-433` |
| `swap_pane_left/down/up/right` | `prefix+shift+h/j/k/l`（1166-1169） | `pane.swap_left` ほか | 左／下／上／右と入れ替え | `prefix+H/J/K/L` | `app/api/panes.rs:774-890`（884 でフォーカスは移動したペインに残る） |
| `cycle_pane_next` | `prefix+tab`（1170） | `pane.cycle_next` | 次のペイン | `prefix+Tab` | `client/shell/actions.rs:1063-1086`（行きがけ順、端で一周） |
| `cycle_pane_previous` | `prefix+shift+tab`（1171） | `pane.cycle_previous` | 前のペイン | `prefix+Shift+Tab` | 同上 |
| `last_pane` | 未設定（1172） | `pane.last` | 直前のペイン | 無し（パレットのみ） | `client/shell/actions.rs:1087-1097` |
| `close_pane` | `prefix+x`（1175） | `pane.close` | ペインを閉じる | `prefix+x` | `layout.rs:224-252`、`workspace/tab.rs:500-514` |
| `zoom` | `prefix+z`（1176） | `pane.zoom` | ズーム切替 | `prefix+z` | `app/actions.rs:810-869`、`workspace/tab.rs:412,512` |
| `resize_mode` | `prefix+r`（1177） | `pane.resize_mode` | サイズ変更モード | `prefix+r` | `client/shell/input.rs:1042-1069` |
| `resize_pane_*` | 未設定（1178-1181） | `pane.resize_left` ほか | 仕切りを左／下／上／右へ | 無し（パレットのみ） | `app/api/panes.rs:719-771`（既定量 0.05、737） |
| `rename_pane` | `prefix+shift+p`（1158） | `pane.rename` | ペイン名を変更 | `prefix+P` | herdr の名前付け。名前は状態行の先頭に出す |
| `clear_pane` | 未設定（1160） | `pane.clear` | 画面を消去 | 無し（パレットのみ） | `client/shell/actions.rs:1102-1104` |
| `copy_mode` | `prefix+[`（1161） | `pane.copy_mode` | コピーモード | `prefix+[` | `keyboard.mdx:93-95` |
| `edit_scrollback` | `prefix+e`（1159） | 採用しない | — | — | §3.5 |
| タブ、ワークスペース、サイドバー、デタッチ（`new_tab`、`next_tab`、`workspace_picker`、`detach` ほか） | 各 `prefix+…` | 採用しない | — | 束縛しない | タブとメニューは要件で禁止。プレフィックスの後に押しても「未束縛」として捨てる |

キー表記の注意。herdr の `prefix+shift+h` は rs-term の表示では `Ctrl+B, Shift+H` である。`prefix+minus` は US 配列でも JIS 配列でも `-` キーで、テンキーの `-` も同じに扱う。`prefix+?` は Shift を伴う配列（US の `Shift+/`、JIS の `Shift+・`）をどちらも「`?` という文字」で照合する。照合は打鍵の文字（`key_char`）で行い、物理キーでは行わない。

### 3.3 動作の詳細（herdr の意味論）

**分割**。フォーカス中の葉を `Split { first: 対象, second: 新ペイン, ratio: 0.5 }` に置き換える（`layout.rs:182-195`、`619-626`、`workspace/tab.rs:329-331` の `ratio.unwrap_or(0.5)`）。`split_vertical` は右（左右に並ぶ、`SplitDirection::Right` → `Direction::Horizontal`、`app/api/panes.rs:2283-2290`）、`split_horizontal` は下（上下に並ぶ）である。新ペインにフォーカスを移す（`client/shell/actions.rs:1058` の `focus: true`、`workspace/tab.rs:408-410`）。ズームは解除する（`workspace/tab.rs:412`）。新ペインは未接続の新しい `Session` で、シリアル設定は `settings.toml` の既定値を使う。元ペインの設定は複製しない（herdr の新ペインも元のプロセスを複製せず新しいシェルを起こす、`workspace/tab.rs:340-389`）。

**閉じる**。直前にフォーカスしていたペインが残っていればそこへ、無ければ行きがけ順で次、最後なら前へフォーカスを移す（`layout.rs:224-252`）。ズームは解除する（`workspace/tab.rs:512`）。閉じたペインのログは flush して閉じ、ポートは切断する。最後の 1 枚について herdr の `close_focused` は `false` を返し（`layout.rs:225-227`）、代わりにタブ／ワークスペースが閉じる。その確認は `ui.confirm_close = true`（`model.rs:1213`）のとき、最後のペインのときだけ出る（`client/shell/overlay_input.rs:1030-1062`）。rs-term にはタブもワークスペースも無く、ウィンドウは空にしない。そこで最後の 1 枚を閉じると、そのペインを捨てて新しい未接続ペイン（新 `PaneId`、新 `Session`、空のグリッド）に置き換える。herdr の確認の位置づけに合わせ、最後の 1 枚が接続中かログ中のときだけ、パレットに確認ページ（§6.5）を出す。それ以外の close は確認しない。

**方向フォーカス**。実際のペイン矩形（論理 px）で、指定方向に重なりのあるペインのうち、辺の距離、重なりの大きさ（大きい方）、中心の距離、行きがけ順の位置、の順で最小のものを選ぶ（`layout.rs:369-433`）。見つからなければ何もしない（`PaneFocusDirectionReason::NoNeighbor`、`app/api/panes.rs:687-689`）。端で回り込まない。ズーム中もズーム前の矩形で計算する（`app/api/panes.rs:2109-2120`、テスト `app/actions.rs:4380`）。ズームは解除せず、新しくフォーカスしたペインがズーム表示になる。

**巡回**。行きがけ順で次（前）へ。端で一周する（`client/shell/actions.rs:1063-1086`）。

**入れ替え**。方向フォーカスと同じ規則で相手を選び、木の形と比率を保ったまま 2 つの ID を交換する（`layout.rs:281-292`）。フォーカスは移動したペイン（元のペイン）に残る（`app/api/panes.rs:884`）。ズーム中はズームのまま、隠れた配置だけを変える（テスト `app/actions.rs:4466`）。

**ズーム**。フォーカス中ペインだけを全面に描く。仕切りは描かない（`ui/tab_surface.rs:68-72`）。隠れたペインの端末サイズは変えない（`ui/panes.rs:324-339` はズームしたペインだけを resize する）。1 枚のときは何もしない（`app/actions.rs:831-838`、`SinglePane`）。rs-term は状態行に「ペインが 1 つなのでズームできない」を出す。分割と閉じるでズームは解除される。

**サイズ変更モード**。`prefix+r` で入る。`h`/`←` `j`/`↓` `k`/`↑` `l`/`→` で、フォーカス中ペインの指定方向の辺にある仕切りを 0.05 動かす。その辺に仕切りが無ければ反対側の辺の仕切りを動かす（`layout.rs:300-324`）。`Esc`、`Enter`、`r`（`resize_mode` の右辺）で抜ける。それ以外のキーは捨て、ポートへ送らない（`client/shell/input.rs:1042-1069`）。状態行に ` RESIZE ` バッジとヒントを出す（`render.rs:112-122`）。

**仕切りのドラッグ**。仕切りをつかんだ位置のずれを保ち、ドラッグ中は 33 ms 以上あけて比率を更新し（`client/shell/mouse.rs:1130`、`1167`）、ボタンを離したとき最終値を確定する（`mouse.rs:1340-1360`）。rs-term は描画を毎フレーム更新し、`layout.toml` の保存はボタンを離したとき 1 回だけ行う（N10）。

**比率**。キー、ドラッグ、ファイル、どこから来ても 0.1〜0.9 に丸める。有限でない値は 0.5（`layout.rs:294-297`、`628-634`）。

**クリック**。ペインのどこかを左クリックするとそのペインにフォーカスする（`keyboard.mdx:10`）。クリックはポートへ何も送らない。

**選択とコピー**。ドラッグで選択し、離したときクリップボードへコピーする。herdr の既定 `ui.copy_on_select = true`（`model.rs:1208`）に従う。

**名前**。`pane.rename` はパレットの検索欄に名前を入力させる。空で確定すると名前を消す。名前は状態行の先頭に出し、`layout.toml` に保存する。

**消去**。`pane.clear` はグリッドとスクロールバックを消す（`Term` の全消去）。ポートとログは触らない。

### 3.4 パレットのキー

`Ctrl+Shift+P` は要件の指定である。herdr は全バインドの一覧（`prefix+?`、`model.rs:1124`）と設定画面（`prefix+s`、`model.rs:1125`）を持つ。rs-term ではその 2 つの役割をパレットが持つ。パレットは全コマンドとキーを出し、部分一致で絞り込め、設定の変更もここで行う。したがって `prefix+?` と `prefix+s` もパレットを開く。

### 3.5 採用しないもの

| herdr | 理由 |
| --- | --- |
| `edit_scrollback`（`prefix+e`、`$EDITOR` で開く） | Windows の GUI アプリに `$EDITOR` の慣習が無い。スクロールバックの保存はログ機能が受け持つ。§14 の未決 Q3 |
| 右クリックメニュー（`keyboard.mdx:10`） | メニューは要件で禁止。右クリックは何もしない |
| タブ、ワークスペース、サイドバー、デタッチ、通知、エージェント | 単一ウィンドウでタブ無しの要件の外 |
| 既定の直接コード（`ctrl+alt+…`） | herdr の既定は全部プレフィックス付きで、直接コードは利用者が足す例（`keyboard.mdx:114-128`）である。既定に入れない。§3.6 |

### 3.6 プレフィックス無しの直接コード

`keymap.toml` は herdr と同じ書式（`[keys]`、`prefix`、各アクション名、文字列か配列、`prefix+…` と直接コード）を受け付ける（§9.3）。herdr の文書の例（`keyboard.mdx:116-128`）からタブの行を除いたものを、そのまま貼れる見本として `docs/keymap-example.toml`（フェーズ 2 で追加）に置く。

```toml
[keys]
focus_pane_left = ["prefix+h", "ctrl+alt+h"]
focus_pane_down = ["prefix+j", "ctrl+alt+j"]
focus_pane_up = ["prefix+k", "ctrl+alt+k"]
focus_pane_right = ["prefix+l", "ctrl+alt+l"]
split_vertical = ["prefix+v", "ctrl+alt+d"]
split_horizontal = ["prefix+minus", "ctrl+alt+shift+d"]
zoom = ["prefix+z", "ctrl+alt+z"]
```

Windows では `Ctrl+Alt` が AltGr と同じに扱われる配列（独語など）がある。そこで直接コードは、打鍵の文字（`key_char`）が AltGr の文字を出したときは照合しない。US と JIS 配列には AltGr が無いので、上の例はそのまま効く。

### 3.7 ペイン以外の直接キー

herdr はコピー、貼り付けを外側の端末に任せる。rs-term はその外側の端末そのものなので、Windows Terminal と同じ慣習を持つ。

| キー | アクション | 理由 |
| --- | --- | --- |
| `Ctrl+Shift+P` | `palette.toggle` | 要件 |
| `Ctrl+Shift+K` | `session.connect` | v5 から継続。コアの `SessionError::NotOpen` の文言が参照している |
| `Ctrl+Shift+D` | `session.disconnect` | v5 から継続 |
| `Ctrl+Shift+B` | `session.break` | v5 から継続 |
| `Ctrl+Shift+C` | `edit.copy` | 外側端末の役（選択があるときだけ。無ければ何もしない） |
| `Ctrl+Shift+V` | `edit.paste` | 外側端末の役 |

herdr は素のシェルへ `Ctrl+Shift+英字` を制御コードとして渡す（`keyboard.mdx:69`）。rs-term は上の 6 文字だけを予約し、それ以外の `Ctrl+Shift+英字` は同じ制御コードを送る。予約した 6 文字の制御コードは `Ctrl+英字`（Shift 無し）で送れる。

## 4. 全体構成

### 4.1 クレート

```text
Cargo.toml                 workspace。gpui-kit = "=0.9.0"、rust-version = "1.95"
Cargo.lock                 コミットする。CI は --locked
rust-toolchain.toml        1.95.0（唯一の版の置き場所）
crates/rs-term-core/       GPUI に依存しない。接続、画面、codec、ログ、設定
crates/rs-term/
  src/main.rs              windows_subsystem、application().run、終了時 quit
  src/ui/app.rs            Workspace（1 つの Entity）。KeyMode、パレット、ペイン木
  src/ui/layout.rs         herdr TileLayout の移植（純粋関数、GPUI 非依存）
  src/ui/keymap.rs         herdr 書式の keymap.toml、KeyMode の状態機械（純粋）
  src/ui/pane.rs           1 ペインの Entity。Session、EventRx タスク、状態行の内容
  src/ui/status.rs         状態行の文字列を作る純粋関数
  src/ui/palette.rs        ページ、行、確定の振り分け（純粋なモデル + 描画）
  src/ui/terminal.rs       自前 Element。セル描画
  src/ui/color.rs          TerminalPalette → 描画色
  src/ui/layout_file.rs    layout.toml の読み書き
```

純粋関数にできる部分（`layout.rs`、`keymap.rs`、`status.rs`、`palette.rs` のモデル）は GPUI を使わない単体テストで受け入れる。描画と配送だけを手動受入に残す。

### 4.2 スレッドとデータの流れ

v5 §4 のとおりで、コミット `d19bef5` が実装済みである。

- GPUI のメインスレッドがペインの `Session`（`alacritty_terminal::Term` を含む）を持つ。
- 開いたポート 1 つにつき専用 OS スレッドが 1 本、`Box<dyn SerialPort>` を占有する。Tokio は入れない。
- 受信はデータ用の有界チャネル（32 × 4096 B）と、状態イベント用の無界チャネル（`PortLost`、`CommandFailed`、`ApplyFailed`、`Pins`、`Dropped`）に分かれる。状態イベントはデータ満杯でも落ちない（テスト `status_events_survive_a_full_data_channel`）。
- 受信の処理順は `Session::apply_rx` だけが知る：生ログ → 復号（Shift_JIS / UTF-8） → 受信改行 → 1 つの `vte::ansi::Processor` → `Term`。テキストログは同じハンドラ呼び出しの中で書く（`text_log_follows_handler_order_and_pause_discards`）。
- ポートのオープンは `begin_connect`（UI スレッド）→ `PendingOpen::open`（バックグラウンド）→ `finish_connect`（UI スレッド）の 3 段で、UI を止めない。途中の `disconnect` はオープンを取り消す。
- 切断はスレッドを待たない。`Session::tick` が回収し、2 秒で終わらないスレッドを状態行に出す。

ペインの前景タスクは `EventRx::recv().await` で起き、1 回の起床で最大 32 イベントを処理し、そのペインだけ `cx.notify()` する（N31）。ワークスペース全体は再描画しない。`Session::tick` は 250 ms のタイマで全ペインに呼ぶ（スレッド回収、ログの 2 秒 flush、経過時間表示の更新）。

### 4.3 コアの公開面（現状を正とする）

`crates/rs-term-core/src/lib.rs` が公開する名前を UI が使う。主なもの：

- `Session::{new, begin_connect, finish_connect, disconnect, reset_to_unconnected, close, link, params, encoding, newline, local_echo, note, clear_note, set_note, tick, apply_rx, apply_input, apply_event, resize, screen_content, set_encoding, set_newline, set_local_echo, set_serial_params, set_dtr, set_rts, log, log_state, scroll_lines, scroll_page, scroll_top, scroll_bottom, select_*, selected_text}`
- `Link::{Disconnected, Opening, Open, Lost}`、`IoEvent`、`EventRx`、`PendingOpen`、`OpenedPort`
- `list_ports`、`PortInfo`、`PortKind`、`UsbIdentity`、`SerialParams`、`DataBits`、`Parity`、`StopBits`、`Flow`
- エラー型 `OpenError`、`PortError`、`SessionError`、`LogError`。`Display` は日本語（`afec483`、テスト `every_message_is_japanese`）
- 設定 `Settings` と読み書き。ペイン木は含まない（`7c390ba`、テスト `layout_is_not_a_core_setting`）

v6 でコアに足す変更は小さい。

| 変更 | 理由 | フェーズ |
| --- | --- | --- |
| `Session::note()` を `Option<Message>`（`severity` と本文）に変える。`Severity::{Info, Warn, Error}` | 状態行が色と寿命を決めるため（§5.3） | 1 |
| `SessionError` の文言「path の変更は切断してから」を「ポートの変更は切断してから」にする | 英単語の混入 | 1 |
| `SessionConfig::default` を Shift_JIS にする | R29 | 1 |
| `ui.ligatures` を schema 1 から外す（読み込むと未知キー扱い） | R36。どこにも効かないキーを残さない | 1 |
| `Session::clear_screen()` | `pane.clear` 用 | 3 |
| `Session::copy_mode_*`（`Term` の vi モード移動と検索を包む） | `pane.copy_mode` 用 | 7 |

### 4.4 エンコーディング、改行、キー、ログ（v5 から変更なし）

v5 §5.6、§5.10、§6.6 の内容は `d19bef5` と `afec483` で実装され、テストがある。要点だけ再掲する。

- 受信：`Shift_JIS` は `encoding_rs::SHIFT_JIS` のストリーミング復号。不正列は U+FFFD。`81 1B 5B 48` は U+FFFD + `ESC [ H`（`malformed_lead_before_esc_is_replacement_not_prolonged_sound`）。切断で未完了バイトを U+FFFD にして閉じる（`disconnect_finishes_the_receive_stream`）。
- 受信改行 `Cr / CrLf / Lf / Auto`、送信改行 `Cr / CrLf / Lf` は Tera Term 5 のマニュアルどおり（`receive_newline_matches_the_manual`）。
- 送信：`encode_from_utf8_without_replacement`。表せない文字があればその入力は 1 バイトも送らない（`unmappable_character_encodes_nothing`）。貼り付けは 1 コマンドで全部送るか全部送らない、上限 16 MiB（`a_large_paste_is_sent_whole`、`a_paste_over_the_limit_sends_nothing`）。
- キー表：Ctrl-C/D/Z、BS `0x08`、Delete `ESC [ 3 ~`、矢印 CSI / アプリケーションカーソルで SS3、Home `ESC [ H`、End `ESC [ F`、F1〜F12、Alt は ESC 前置、`Ctrl+[ \ ] ^ _ @ ?`（`keys_match_the_table`、`app_cursor_mode_sends_ss3_arrows`）。
- ログ：Binary は復号前の受信バイト。Text は UTF-8、BOM なし、ハンドラ順、固定書式のローカル時刻。一時停止中はファイルを伸ばさない。64 KiB ごとと 2 秒無通信で flush、ドロップでも flush。OS ロックで同一パスを排他（`log.rs` の 9 テスト）。

### 4.5 端末の描画

自前の `Element` がセルを描く。v5 §4.5、§5.9 を引き継ぎ、次を直す。

- セルの寸法は端末フォントの `'0'` のアドバンスと、整形した行の高さから測る。フォント名かサイズが変わったら測り直す（R20）。ペインの内容矩形が変わったら同じフレームの prepaint で列と行を計算し `Session::resize` を呼び、変化があれば `cx.notify()` する（N12）。
- 列数 = floor(内容幅 / セル幅)、行数 = floor(内容高 / セル高)。どちらも論理 px。`scale_factor` は掛けない。
- 字形は行単位でまとめて整形する（同じ属性の連続をひとつの `TextRun`）。太字と斜体を `TextRun` に渡す（R27、N31）。
- 選択の背景は `TerminalPalette.selection`。カーソルはセルを反転して描き、下の字が見えるようにする（R27）。
- 端末フォントの候補は、設定名、`Consolas`、`Cascadia Mono`、`DejaVu Sans Mono`、`Noto Sans Mono`、`Liberation Mono`、`Menlo` の順で `all_font_names` にある最初のもの。どれも無ければグリッドは空のまま状態行にエラーを出す。`calt`、`liga`、`clig` は 0。

## 5. 状態行

### 5.1 原則

- 各ペインの下端に 1 行。高さ 24 px。ズーム中は見えているペインの 1 行だけ。
- ボタンにしない。クリックはそのペインにフォーカスするだけ。
- アプリの全メッセージ（エラー、警告、情報）はこの行に出す。ペインに属さないメッセージ（設定の読込失敗、キーマップの誤り、保存失敗、版表示）は、その時点でフォーカスしているペインの行に出す。描かれない文字列の置き場を作らない（R03）。
- 1 行に収まらないメッセージは末尾を「…」で切る。全文はパレットの「メッセージ履歴」（`app.messages`）で読める。

### 5.2 欄の並び

左から順に次の欄を並べる。欄と欄の間は 14 px。詳しい書式、色、例は screen-design §3。

1. モードバッジ（フォーカス中ペインだけ）：`PREFIX`、`RESIZE`、`COPY`、`ZOOM`。
2. ペイン名（名前があるときだけ）。
3. 接続の点と名前：`● COM3`（緑、接続）、`● COM3 接続中…`（黄）、`● COM3 切断`（赤、喪失）、`● COM3`（灰、利用者が切断）、`● 未接続`（灰、ポート未選択）。
4. 速度とフレーム：`115200 8N1`。未選択なら `—`。
5. フロー：`Flow: None`、`Flow: XON/XOFF`、`Flow: RTS/CTS（送信のみ）`。
6. エンコーディング：`UTF-8` / `Shift_JIS`。
7. 付帯：`ECHO`（ローカルエコー時）、`RX/TX 改行` は既定と違うときだけ `改行 受信:CRLF 送信:CR`。
8. ログ：`● LOG text 00:02:51`（赤）、`● LOG bin 00:00:40`、`LOG 一時停止 00:01:10`（黄）、`LOG off`。
9. メッセージ（残りの幅すべて）。

幅が足りないときは、メッセージを最低 160 px 残すまで、7 → 5 → 6 → 4 の順に欄を落とす。それでも足りなければメッセージを切る。

### 5.3 メッセージの寿命

| 重さ | 表示 | 消えるとき |
| --- | --- | --- |
| エラー | 赤字で `エラー: 本文` | 同じペインで新しいメッセージが出たとき、または原因が解消したとき（例：接続成功で接続エラーを消す、ログ開始成功でログのエラーを消す）。時間では消さない |
| 注意 | 黄字で `注意: 本文` | 新しいメッセージが出たとき、または 15 秒後 |
| 情報 | 通常色で本文 | 新しいメッセージが出たとき、または 5 秒後 |

全メッセージは時刻付きで最大 200 件をアプリが保持し、`app.messages` で新しい順に見られる。

## 6. コマンドパレット

### 6.1 部品

`gpui_component::command` の `Command` と `CommandState` を使う。読んだ API（`gpui-component` 0.9.0）：

- `CommandState::focus`（`src/command/state.rs:306`）で検索欄にフォーカスする。
- `CommandState::set_query`（`state.rs:234`）でクエリを置き換える。ページを変えるたびに `""` にする（R09）。
- `Command::placeholder`（`command.rs:179`）、`empty`（`185`）、`header`（`219`）、`footer`（`231`）、`on_confirm`（`159`）、`on_cancel`（`170`）、`max_h`（`197`）。
- `on_confirm` の `IndexPath` は「絞り込む前のモデルでの位置」である（`state.rs:250-255` の文書、`384` と `419` で入力位置から作る）。したがって N1 の懸念（絞り込み後の位置）は起きない。それでも確定はモデルの行に持たせた値で引く（§6.3）。
- Escape：クエリが空でなければクエリを消すだけ。空なら `on_cancel` を同期で呼んでから伝播する（`state.rs:600-615`）。
- コンポーネントに日本語ロケールは無い（`locales/ui.yml:350-356` は en、pt-BR、ru、zh だけ）。プレースホルダと空表示は必ず自分で渡す。
- 行の右のキー表示は既定では GPUI の束縛から作られる（`state.rs:793-801`）。プレフィックスのキーは GPUI の束縛ではないので、全行を `CommandItem::child`（`item.rs:91-104`）で描き、ラベル、補足、キーを自分で並べる。

パレットはウィンドウ中央上寄りの 1 枚の重ね表示で、別ウィンドウにも `Dialog` にもしない。

### 6.2 開閉とフォーカス

| 契機 | 動作 |
| --- | --- |
| `Ctrl+Shift+P`、`prefix+?`、`prefix+s` | 閉じていればルートページで開き、クエリを空にし、`CommandState::focus` を呼ぶ。開いていれば閉じる（`Ctrl+Shift+P` のみ。プレフィックスはパレット表示中は効かない） |
| `Ctrl+Shift+K` など直接キーのコマンド | パレットが閉じていても、そのコマンドの最初のページで開く |
| 確定で次のページがある | ページを積み、クエリを空にし、フォーカスを検索欄に保つ |
| 確定で次のページが無い（実行して終わり） | 実行し、パレットを閉じ、フォーカス中ペインへフォーカスを返す（R11） |
| Escape（クエリ有り） | コンポーネントがクエリを消す |
| Escape（クエリ空、ページが 2 段以上） | 1 つ前のページへ戻る。戻ったページのクエリは空 |
| Escape（クエリ空、ルート） | 閉じてペインへフォーカスを返す |
| パレットの外をクリック | 閉じてクリックしたペインへフォーカス |

`on_cancel` の後に Cancel は親へ伝播する。ワークスペースは Cancel を処理しない（二重に閉じない、監査の `27-one-escape-from-ports.png`）。パレット表示中のキーは検索欄へ行き、ポートには行かない。閉じたら必ずフォーカス中ペインの `FocusHandle` にフォーカスを戻す（R04）。

### 6.3 ページのモデル

```rust
enum Page {
    Root,
    Ports,              // 接続 1/5
    Baud { wizard: bool },
    Framing { wizard: bool },
    Flow { wizard: bool },
    Dtr { wizard: bool },
    Rts, Encoding, ReceiveNewline, TransmitNewline { receive: ReceiveNewline },
    LocalEcho, SendText, LogName { kind: LogKind }, LogMode { kind: LogKind, name: String },
    LogTimestamp { name: String, append: bool }, TerminalFont, FontSize, UiFont,
    Theme, RenamePane, ConfirmClosePane, Messages, ResetSettings,
}

struct Row { label: String, sub: String, key: Option<String>, value: RowValue, enabled: bool }
```

各ページは「今のクエリから `Vec<Row>` を作る関数」である。描画のたびに同じ関数で作り、`on_confirm` の `IndexPath.row` でその `Vec` の行を引き、行の `RowValue` で処理する。行を行番号で別の配列から引かない（N1 の予防）。接続の途中の値は `ConnectDraft`（ポート、速度、フレーム、フロー、DTR）に積み、DTR の確定で 1 回だけ `begin_connect` を呼ぶ。途中で Escape して閉じたら下書きは捨てる。

ページの一覧、行の文言と順序、ヘッダ、フッタは screen-design §4 が正である。

### 6.4 接続の手順

`session.connect`（`Ctrl+Shift+K`）は、フォーカス中ペインに対して 5 ページを順に出す。

1. ポート：`list_ports` の結果。1 秒ごとに更新する（このページが開いている間だけ）。ポートが無いときは確定できない行「ポートが見つからない（USB を挿すと 1 秒以内に出る）」を 1 行出し、Enter は何もしない（R08）。
2. ボーレート：9600、19200、38400、57600、115200、230400、460800、921600。クエリが 1〜4000000 の整数なら先頭に「この値を使う：{n}」を足す。現在値に「✓」。
3. フレーム：`8N1` を先頭に、データ 8/7/6/5 × パリティ N/E/O × ストップ 1/2 の 24 通り（重複除く）。
4. フロー：None、XON/XOFF、RTS/CTS（送信のみ）。
5. DTR：オフ（既定）、オン。確定で接続を始める。

初期の選択行は、そのペインの前回値（無ければ設定の既定）である。ペインがすでに接続中なら、1 ページ目の前に「すでに COM3 に接続中。切断してから選び直す」行だけの確認ページを出し、確定で切断して 1 ページ目へ進む。

接続を始めたら状態行は `接続中…`。成功で `● COM3`、失敗でエラー（例：「COM3 を開けない: アクセスが拒否された（他のアプリが使用中の可能性）」）。

### 6.5 ペインを閉じる確認

`pane.close` で、閉じるペインが最後の 1 枚で、かつ接続中（`Opening`、`Open`）かログ中（`Running`、`Paused`）のときだけ、パレットに確認ページを出す（§3.3）。行は「閉じる（切断してログを止める）」「やめる」の 2 つ、初期選択は「やめる」。

## 7. キー入力

### 7.1 配送の順序

1 打鍵は次の順で 1 か所だけに届く。

1. パレットが開いていれば、パレット（検索欄）。`Ctrl+Shift+P` だけはパレットを閉じる。
2. GPUI の束縛にある直接キー（§3.7 と keymap.toml の直接コード）。アクションを実行して終わり。
3. `KeyMode` の状態機械（§7.2）。
4. フォーカス中ペインの `Session::apply_input`。未接続なら送らず、状態行にエラー「未接続なので送らなかった（Ctrl+Shift+K で接続）」を出す。ローカルエコーも行わない（送っていないので）。

フォーカスしていないペインには届かない。IME はパレットの検索欄（`session.send_text`）だけで受け、グリッドは入力ハンドラを持たない（v5 §6.4 と同じ）。

### 7.2 KeyMode

```rust
enum KeyMode {
    Terminal,
    Prefix,              // Ctrl+B の後、次の 1 打鍵だけ
    Resize,              // prefix+r から Esc / Enter / r まで
    Copy(CopyState),     // prefix+[ から q / Esc まで
}

fn route(mode: &KeyMode, key: &Keystroke, keymap: &Keymap) -> (KeyMode, Routed);
enum Routed { Action(PaneAction), SendToPane(Input), Swallow }
```

`route` は純粋関数で、herdr の `client/shell/input.rs:660-730` と `1042-1069` の表をそのまま写す。

| 現在 | 打鍵 | 次 | 結果 |
| --- | --- | --- | --- |
| Terminal | プレフィックス | Prefix | Swallow |
| Terminal | その他 | Terminal | SendToPane |
| Prefix | プレフィックス | Terminal | SendToPane（`0x02`） |
| Prefix | Esc | Terminal | Swallow |
| Prefix | 束縛あり | Terminal（`resize_mode` なら Resize、`copy_mode` なら Copy） | Action |
| Prefix | 束縛なし | Terminal | Swallow |
| Resize | h/j/k/l、矢印 | Resize | Action（resize_*） |
| Resize | Esc、Enter、`r` | Terminal | Swallow |
| Resize | その他 | Resize | Swallow |
| Copy | §7.3 | Copy または Terminal | Action / Swallow |

モードはワークスペースに 1 つで、フォーカス中ペインに対して働く。パレットを開くと Prefix は Terminal に戻る。Resize と Copy はパレットを開いても保つ（閉じたら続く）。

### 7.3 コピーモード

`prefix+[` で入る。herdr の `keyboard.mdx:93-95` のキーを写す：`h/j/k/l` と矢印、`w/b/e`、`W/B/E`、`{` `}`、`PageUp/PageDown`、`Ctrl+F`（前方ページ）、`Ctrl+U/Ctrl+D`、`/` と `?` で前方／後方の文字列検索（大文字を含まなければ大小無視）、`n`/`N`、`v` と Space で選択開始、`V` で行選択、`y` と Enter でコピーして抜ける、`q` と Esc で抜ける（選択か検索があれば先にそれを消す）。プレフィックスはコピーモード中もプレフィックスである（`Ctrl+B` でページ戻りはしない、herdr と同じ）。受信は止めない。

### 7.4 スクロール

マウスホイールで 3 行ずつ。キーボードのスクロールはコピーモードで行う（herdr と同じ）。ターミナルモードの `PageUp/PageDown/Home/End` は v5 §6.6 の送信表どおりポートへ送る（R24 の「送信バイトを奪わない」）。新しい受信があったら最下部へ戻る（`display_offset` を 0 に）。

## 8. マウス

| 操作 | 動作 |
| --- | --- |
| ペイン上で左クリック | そのペインへフォーカス。パレットが開いていれば閉じる |
| ペイン上で左ドラッグ | 文字選択。離したらクリップボードへコピー（`copy_on_select`）。状態行に情報「{n} 文字をコピーした」 |
| 仕切り上で左ドラッグ | 比率を変える（§3.3）。離したら layout.toml を保存 |
| ホイール | スクロールバック 3 行 |
| 右クリック | 何もしない |

仕切りの当たり判定は、見た目 2 px の隙間の中心から左右（上下）3 px ずつ、計 6 px。カーソルは左右分割で `col-resize`、上下分割で `row-resize`。

## 9. 設定とファイル

### 9.1 置き場所

| ファイル | Windows | 二次 OS |
| --- | --- | --- |
| 設定 | `%APPDATA%\rs-term\settings.toml` | `$XDG_CONFIG_HOME/rs-term/` または `~/.config/rs-term/` |
| ペイン配置 | `%APPDATA%\rs-term\layout.toml` | 同上 |
| キーマップ | `%APPDATA%\rs-term\keymap.toml` | 同上 |
| ログ | `%LOCALAPPDATA%\rs-term\logs\` | `$XDG_STATE_HOME/rs-term/logs` または `~/.local/state/rs-term/logs` |

環境変数が空ならパスを推測しない。設定は既定値で起動し、状態行にエラーを出す。ログは開始を拒否し、状態行にエラーを出す。二次 OS も設定とログを同じ方針（XDG）で決める（R22）。

### 9.2 settings.toml

v5 §7 の骨格から `[ui] ligatures` を除いたもの（R36）。`schema = 1`、`deny_unknown_fields`、範囲外は全体を拒否（`read_timeout_ms` は 1〜100、N13）。読み込みに失敗したファイルは書き換えない。そのプロセスでは設定の保存をしない（「読み込み失敗のラッチ」）。保存の失敗はラッチしない：古いファイルを残してエラーを出し、次の変更でまた保存を試みる（R21）。

既定値に戻す唯一の手段はパレットの `app.reset_settings`「設定を初期化する」で、確認ページの後、既定値で `settings.toml` を上書きする（R37）。壊れたファイルは上書き前に `settings.toml.broken-YYYYMMDD-HHMMSS` へ改名して残す。

保存の契機：パレットで値を確定したとき（その値を settings の既定にもする項目だけ。§9.4）。

### 9.3 keymap.toml

herdr と同じ書式である（`src/main.rs:136-186` の注釈、`keyboard.mdx:97-128`）。

```toml
[keys]
prefix = "ctrl+b"
split_vertical = "prefix+v"
zoom = ["prefix+z", "ctrl+alt+z"]
```

- キー名は herdr のアクション名（§3.2 の表の 1 列目）と、rs-term 独自の `palette`、`connect`、`disconnect`、`break`、`copy`、`paste`。
- 値は文字列か文字列の配列。空文字列は束縛を外す。
- 構文は herdr の「`ctrl`/`shift`/`alt` の修飾 + 文字、`enter`/`tab`/`esc`/矢印、`minus` などの名前付き記号」。
- TOML として読めなければファイル全体を無視して既定を使い、状態行にエラー。未知のアクション名、解釈できないキーはその 1 項目だけ捨て、状態行に注意（何件捨てたか、最初の 1 件の名前）を出す（R23）。
- 読むのは起動時だけ。パレットの `app.reload_keymap`「キーマップを読み直す」でも読む（herdr の `reload_config` の役）。

### 9.4 layout.toml

UI クレートだけが読み書きする。コアの設定には入れない（`7c390ba`）。

```toml
schema = 1
focus = 3

[root]
direction = "right"     # herdr の SplitDirection。"right" | "down"
ratio = 0.5             # 0.1〜0.9
[root.first]
pane = 1
[root.second]
direction = "down"
ratio = 0.5
[root.second.first]
pane = 2
[root.second.second]
pane = 3

[[pane]]
id = 1
name = ""
port = "COM3"
baud_rate = 115200
data_bits = "eight"
parity = "none"
stop_bits = "one"
flow = "none"
dtr_on_open = "off"
encoding = "shift_jis"
receive = "auto"
transmit = "cr"
local_echo = false
```

- 葉の ID の重複と 0 は全体を拒否する（N29、スタッシュの `saved_ratios_are_clamped_and_duplicates_refused`）。拒否したら 1 枚の未接続ペインで起動し、ファイルは残し、エラーを出す。
- 比率は読むときも 0.1〜0.9 に丸める。
- 保存の契機：分割、閉じる、入れ替え、ドラッグ終了、サイズ変更モードを抜けたとき、名前の変更、接続成功（N9）、ペイン単位の設定変更。デバウンスして 500 ms 以内の連続は 1 回にまとめる（N10）。
- 起動時にポートは開かない（`reopen_on_start` は既定 false、v5 と同じ）。開くなら DTR が動くからである。

## 10. 配布

v5 §7.1 のとおり。v6 で足すこと：

- `main.rs` に `#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]`（N7）。
- 最後のウィンドウが閉じたら `cx.quit()`（N26）。終了前に全ペインのログを flush し、ポートを切断する。
- zip の中身：`rs-term.exe`、`LICENSE`、`NOTICE`、`THIRD-PARTY-LICENSES.txt`（`cargo about generate` で生成、MPL-2.0 の条文を含む）、`README-ja.txt`（キー表、SmartScreen の注意）（R35、N21）。パレット（16 色）はバイナリに埋め込むので別ファイルにしない。
- `release.yml` はテストジョブの成功を `needs` にし、Windows の zip だけを下書きリリースに載せる（N22）。
- CI のツールチェーンは `rust-toolchain.toml` を読む。`rustup target add $TARGETS` の引用符を外す（N23）。権限を最小にし、サードパーティのアクションを SHA で固定する（N24）。
- `app.about`「バージョン情報」は状態行に情報「rs-term {版} — Copyright 2026 sotola122 — Apache-2.0」を出す。

## 11. 実装計画

### 11.1 出発点（2026-11-10 の事実）

- ブランチ `fix/rs-term-v1-herdr` の先端は `7c390ba`。コアの 3 コミットは v6 の §4 と一致する。
- `rs-term` UI クレートは先端でビルドできない。`crates/rs-term/src/ui/workspace.rs:28` が `7c390ba` で消えた `LayoutNode`、`PaneSettings` を使い、`workspace_methods.rs:622-630`、`982-1053` が `settings.layout` を読み書きしている（静的に確認。この環境の rustc は 1.85.1 で、`cargo check` は依存の `rust-version` で止まり、実行できなかった）。
- スタッシュ `opus-wip-ui-keymap` は `layout.rs` を herdr の `TileLayout` の写しに近づけ（比率 0.1〜0.9、`from_saved` の重複拒否、`resize_mode` をレイアウトから外す、テスト 7 本）、`color.rs` の N19 を直し、端末フォントの候補を足している。v6 と一致するので、フェーズ 0 でそのまま適用する。
- 既存 UI の `workspace.rs` と `workspace_methods.rs`（計約 2000 行）は、状態行、パレット、キーの作りが v6 と合わないので、§4.1 のモジュールへ作り直す。`layout.rs`、`color.rs`、`actions.rs` のアクション定義は使い回す。

### 11.2 進め方

各フェーズは前のフェーズの受入が崩れていないことを確かめてから始める。「受入」は、(a) 名前を挙げた `cargo test` が `windows-latest` で通ること、(b) Windows 実機での手動確認、の両方である。手動確認の手順は `docs/acceptance/phase-N.md` に書き、結果（日付、OS 版、スクリーンショット名）を同じファイルに追記する。Linux の確認はフェーズを完了にしない。

UI の自動テストは、GPUI を使わない純粋関数（`layout`、`keymap`、`status`、`palette` のモデル）で書く。`gpui-kit` の `test-support` が `windows-latest` で動くと確かめられたら、§11.4 の「UI 統合テスト」を足す。それまでは手動確認を正とする。

テスト名は `クレート::モジュール::tests::名前` で書く。新しく書くテストは名前を先に決めてある。実装者は名前を変えない。

### 11.3 フェーズ

#### フェーズ 0：ビルドと最初の 1 枚

作業：
- スタッシュを適用する。UI クレートを §4.1 のモジュールに組み替え、コアの現 API に合わせてビルドを通す。
- 起動時の `apply_rx(b"hello\r\n")` を消す（R01）。`Workspace.status` を消す（R03）。
- 単一ウィンドウ、OS のキャプション、1 枚の未接続ペイン、下端の状態行（この時点では接続欄とメッセージ欄だけでよい）。
- グリッドの空表示：未接続で受信が 1 バイトも無いペインは、グリッドの左上に淡色で「（未接続）  Ctrl+Shift+K で接続」を描く（s3、s4）。これはグリッドの内容ではなく重ね描きで、`Term` には入れない。
- `windows_subsystem`、最後のウィンドウで `quit`（N7、N26）。
- CI：ツールチェーンを `rust-toolchain.toml` だけにする。`release.yml` に `needs`（N22、N23、N24、R34）。

自動受入：
- `cargo test --locked --workspace` が `windows-latest` で緑。
- `rs-term-core::session::tests::hello_is_visible_without_a_port`、`rs-term-core::screen::tests::hello_crlf_is_visible_without_a_port`（hello はテストだけ）。
- `rs-term::ui::layout::tests::{split_puts_the_new_pane_second_and_focuses_it, grid_focus_moves_by_direction_and_stops_at_the_edge, cycle_walks_preorder_and_wraps, close_without_history_takes_the_next_pane_in_preorder, close_and_split_end_zoom, swap_keeps_focus_on_the_moved_pane, saved_ratios_are_clamped_and_duplicates_refused, resize_moves_the_divider_and_clamps}`（スタッシュのテスト）。
- 新規 `rs-term::ui::app::tests::startup_grid_is_empty`：起動時の全ペインの `screen_content()` に空白以外の文字が無い。
- 新規 `rs-term::tests::no_status_side_channel`：`crates/rs-term/src` に `status: String` というフィールドが無いことを `include_str!` で検査する（R03 の再発防止）。

手動受入（Windows）：
- M0-1 起動して画面が `wireframes/01-single-disconnected.svg` と同じ構成である。`hello` が無い。状態行が下端にある。
- M0-2 リリースビルドの exe をエクスプローラから起動してもコンソール窓が出ない。閉じるとプロセスが残らない（タスクマネージャ）。
- M0-3 壊れた `settings.toml` を置いて起動すると、窓が開き、状態行に「エラー: 設定ファイルを読めない（…）。既定値で起動した。ファイルは書き換えない」が出る。終了後、ファイルのハッシュが変わらない。

#### フェーズ 1：状態行とメッセージ

作業：§5 の全部。コアの `Message`（§4.3）。`app.messages`、`app.about`。

自動受入（新規、`rs-term::ui::status::tests`）：
- `disconnected_without_port_reads_mitsetsuzoku`：`Link::Disconnected` かつポート未選択 → 欄 3 が `未接続`、欄 4 が `—`（R02）。
- `user_disconnected_keeps_port_name_gray`：ポート選択済みで切断 → `COM3`、点は灰。
- `open_line_matches_mock_s1`：COM3、115200 8N1、None、UTF-8、テキストログ 171 秒 → `COM3  115200 8N1  Flow: None  UTF-8  ● LOG text 00:02:51`。
- `rtscts_shows_send_only`：`Flow: RTS/CTS（送信のみ）`（R31）。
- `local_echo_flag_is_shown`（R16）。
- `fields_drop_in_order_when_narrow`：幅 300 px で 7 → 5 → 6 → 4 の順に落ち、メッセージは 160 px 残る。
- `error_persists_info_expires`：情報は 5 秒、注意は 15 秒で消え、エラーは残る。
- `connect_success_clears_connect_error`。
- `rs-term-core::error::tests::every_message_is_japanese`（既存）と、新規 `rs-term::ui::messages::tests::every_ui_message_is_japanese`（screen-design §6 の全文言を列挙し、ひらがな・カタカナ・漢字を含むことを見る）。

手動受入：
- M1-1 未接続でキーを打つと、状態行に「エラー: 未接続なので送らなかった（Ctrl+Shift+K で接続）」が赤で出る。
- M1-2 `app.about` で版と著作権が出て、5 秒で消える。
- M1-3 `APPDATA` を空にして起動すると、状態行にエラーが出る（推測パスを作らない）。

#### フェーズ 2：パレットとキーの状態機械

作業：§6.1〜§6.3、§7.1〜§7.2、§9.3。ルートページの全行（実行できない行は「準備中」ではなく、まだ出さない）。

自動受入（新規）：
- `rs-term::ui::keymap::tests::default_is_herdr`：既定の表が §3.2 と一致（`prefix = ctrl+b`、`split_vertical = prefix+v` …）。
- `prefix_then_prefix_sends_stx`、`prefix_then_esc_swallows`、`prefix_then_unbound_is_discarded_not_sent`、`prefix_has_no_timeout`（時刻を進めても Prefix のまま）。
- `resize_mode_swallows_other_keys_and_exits_on_esc_enter_r`。
- `keymap_file_parses_herdr_syntax`（文字列、配列、空文字列で解除）、`unknown_action_is_skipped_with_a_warning`、`broken_toml_falls_back_to_defaults`（R23）。
- `direct_chord_ignores_altgr_text`。
- `rs-term::ui::palette::tests::root_labels_match_screen_design`（screen-design §4.1 の表と完全一致、順序込み）（R10）。
- `search_bo_finds_baud`：クエリ「ボー」でボーレートの行が残る（R10、s2）。
- `page_change_clears_query`（R09）。
- `confirm_reads_the_row_value_not_an_external_index`：クエリ `12345` のボーレートページで先頭行を確定すると 12345（N1）。
- `escape_steps_back_one_page_then_closes_at_root`（R04）。
- `action_without_next_page_closes_palette`（R11）。

手動受入：
- M2-1 `Ctrl+Shift+P` 直後に「ボー」と打つと検索欄に入り、ボーレートが選ばれている（s2、`wireframes/04-palette-root.svg`）。
- M2-2 Escape：クエリ有り → 消える、空 → 閉じる。閉じた直後に `Ctrl+Shift+K` が効く（クリック不要）（R04）。
- M2-3 `Ctrl+B` を押すと状態行に `PREFIX` が出る。`Ctrl+B` をもう一度押すと対向に `0x02` が届く（com0com か USB ループバックで確認）。`Ctrl+B` → `q` は何も送らない。
- M2-4 `prefix+?` でパレットが開く。

#### フェーズ 3：ペイン（herdr）

作業：§3 の全部。`layout.toml`。仕切りのドラッグ。ズーム。サイズ変更モード。閉じる確認。名前。消去。実矩形での方向計算（R15）。

自動受入（新規、`rs-term::ui::layout::tests` に追加）：
- `split_vertical_is_right_split_horizontal_is_down`（R05）。
- `focus_after_close_prefers_previous_then_preorder`（herdr `layout.rs:224-252`）。
- `focus_while_zoomed_uses_unzoomed_rects_and_stays_zoomed`。
- `swap_while_zoomed_stays_zoomed`。
- `zoom_single_pane_is_noop_with_reason`。
- `resize_uses_opposite_edge_when_none`。
- `drag_ratio_is_clamped_to_point_one_point_nine`。
- `rs-term::ui::app::tests::new_pane_uses_settings_defaults_not_the_source`（R12）。
- `closing_last_pane_replaces_it_with_a_fresh_disconnected_pane`（R13）。
- `closing_last_connected_pane_asks_first`。
- `rs-term::ui::layout_file::tests::{round_trip_keeps_shape_ratio_names_and_params, duplicate_ids_reject_the_file, ratio_is_clamped_on_load, save_is_debounced}`（N9、N10、N29、R49）。

手動受入：
- M3-1 1 枚から `prefix+v`、`prefix+h`、`prefix+-`、`prefix+l`、`prefix+-` で 2×2 になり、`wireframes/03-split-2x2-focus.svg` と同じ形で、最後に作った右下にフォーカス枠がある（s4）。
- M3-2 `prefix+h/j/k/l` で枠が動き、端では動かない。`prefix+Tab` は一周する。
- M3-3 `prefix+z` で 1 枚が全面になり状態行に `ZOOM`、もう一度で戻る（`wireframes/10-zoomed.svg`）。ズーム中 `prefix+l` で右のペインが全面になる。
- M3-4 `prefix+r` → `l` を 3 回 → Esc で仕切りが 15% 動く。サイズ変更中の `x` はポートへ送られない。
- M3-5 仕切りをドラッグして離す。再起動で同じ比率、同じ形、各ペインの設定が戻り、ポートは開かない。ドラッグ中に `layout.toml` の更新時刻が変わらない。
- M3-6 キーはフォーカス中ペインのポートだけに届く（2 本のループバックで確認）。

#### フェーズ 4：接続とセッションのコマンド

作業：§6.4、`session.*` の全ページ。パラメータ変更の失敗の表示（R45、N25 はコア済み）。`begin_connect` の非同期化を UI でつなぐ（R52）。`Session::tick` の定期呼び出し。

自動受入：
- 新規 `rs-term::ui::palette::tests::connect_walks_port_baud_framing_flow_dtr`（5 ページ、`ConnectDraft` が `SerialParams` に写る）。
- `no_port_row_is_not_confirmable`（R08）。
- `framing_page_lists_8n1_first_and_24_combinations`（R07 の構造的解消）。
- `rts_page_only_when_flow_is_none`（N17、コアは `manual_rts_only_without_flow_control`）。
- 既存コア：`open_is_split_and_a_disconnect_cancels_it`、`refused_parameters_are_reverted`、`timeout_is_not_disconnect_and_other_errors_are`、`eof_is_port_lost`、`status_events_survive_a_full_data_channel`、`disconnect_does_not_wait_for_a_slow_read`。

手動受入（Windows 実機、USB シリアル 2 本）：
- M4-1 `Ctrl+Shift+K` → COM を選び Enter 5 回で 115200 8N1 None DTR オフで開く。各ページが `wireframes/05..09` と同じ。
- M4-2 Bluetooth の仮想 COM を開いて応答が遅くても、その間に他のペインの表示とキーが止まらない（R52）。
- M4-3 接続中に USB を抜くと 1 秒以内に `● COM3 切断`（赤）とエラー「ポートとの通信が切れた（OS: …）」（`wireframes/11-error.svg`）。
- M4-4 COM10 以上を `\\.\` なしで開ける。
- M4-5 921600 bps で連続受信中もキー入力が 100 ms 以内に送られる。

#### フェーズ 5：描画、テーマ、フォント

作業：§4.5。`theme.dark`、`theme.light`、`font.*`。

自動受入：
- 既存 `rs-term::ui::color::tests::*` と新規 `bright_foreground_reads_index_267`（スタッシュ、N19）。
- 新規 `rs-term::ui::terminal::tests::grid_size_follows_font_change`（セル寸法を差し替えると列・行が変わる、R20）。
- 新規 `selection_uses_palette_selection_color`、`cursor_cell_is_inverted_not_covered`（R27）。

手動受入：
- M5-1 Consolas 14 px、100% / 150% / 200% の DPI で列が崩れない。モニタ間移動でも崩れない。
- M5-2 フォントサイズを 18 に変えると直ちに列数が変わり、80 桁の行を受信しても右端が切れない（目視）。
- M5-3 窓のリサイズ直後、受信が無くても行数が変わる（N12）。
- M5-4 ダーク／ライトの切替が再起動後も残る。

#### フェーズ 6：ログ

作業：`log.*` のページ（screen-design §4.8）。状態行のログ欄と経過時間。

自動受入：既存 `rs-term-core::log::tests` の 9 本と `text_log_follows_handler_order_and_pause_discards`、`text_log_maps_line_drawing_like_the_screen`。新規 `rs-term::ui::palette::tests::log_text_asks_new_or_append_then_timestamp`、`log_binary_has_no_timestamp_page`、`empty_log_name_uses_the_template`。

手動受入：
- M6-1 テキストログ開始で状態行が `● LOG text 00:00:00` になり、秒が進む。一時停止で黄色の `LOG 一時停止`。
- M6-2 アプリを閉じた後、ログファイルの末尾に最後の受信が入っている（N5）。
- M6-3 2 つ目のペインで同じファイル名を指定すると「そのログファイルは別のペインかアプリが使用中」。

#### フェーズ 7：コピーモード、選択、貼り付け

作業：§7.3、§7.4、§8 の選択とコピー、`edit.paste`。

自動受入（新規 `rs-term::ui::keymap::tests`）：`copy_mode_keys_match_herdr`、`copy_mode_esc_clears_selection_before_exit`、`prefix_still_works_in_copy_mode`。コア新規 `copy_mode_search_is_case_insensitive_without_uppercase`。

手動受入：
- M7-1 ドラッグして離すとクリップボードに入る。`Ctrl+Shift+V` で対向へ送られる。
- M7-2 `prefix+[` → `k` で過去行へ、`/` で検索、`y` でコピー。その間も受信が表示され続ける。

#### フェーズ 8：配布

作業：§10。`README.md`（実際に効くキーだけを書く、R47）。

受入：
- `release.yml` の下書きリリースに zip が 1 つだけ載り、テストが落ちた場合は作られない。
- M8-1 クリーンな Windows 11 x64 で zip を展開して起動できる。zip に `THIRD-PARTY-LICENSES.txt` と MPL の条文がある。

### 11.4 UI 統合テスト（条件付き）

`gpui-kit` の `test-support` が `windows-latest` で動くなら、次を足して手動確認の一部を自動化する：`palette_opens_with_focus_and_takes_keys`、`escape_closes_and_returns_focus_to_pane`、`prefix_v_splits_and_focuses_the_new_pane`。動かなければ書かない。手動確認の M2-1、M2-2、M3-1 が正である。

## 12. 監査とレビューの対応表

「済」はコミット済み（どのコミットか）、「P*n*」はフェーズ番号、「対応しない」は理由付き。監査 = `rs-term-audit-report.md`（R）、レビュー = `rs-term-claude-review.md`（N）。

### 12.1 監査 R01〜R58

| ID | 内容（短縮） | 対応 | 受入 |
| --- | --- | --- | --- |
| R01 | 本番で `hello` を注入 | P0 | `startup_grid_is_empty`、M0-1 |
| R02 | 状態行が上端、欄不足 | P0（位置）、P1（欄） | `status::tests::*`、M0-1 |
| R03 | `Workspace.status` が描かれない | P0（削除）、P1（全メッセージを状態行へ） | `no_status_side_channel`、M1-1 |
| R04 | パレットにフォーカスが無い、Escape | P2 | `escape_steps_back_one_page_then_closes_at_root`、M2-1、M2-2 |
| R05 | 分割の語彙が 3 通り | P3。herdr の語彙に統一（§3.2、§9.4） | `split_vertical_is_right_split_horizontal_is_down` |
| R06 | キーが設計とも herdr とも違う | P2、P3。herdr 既定（§3） | `default_is_herdr`、M2-3、M3-1 |
| R07 | フレームの 2〜3 ページに到達しない | P4。フレームを 1 ページにした | `framing_page_lists_8n1_first_and_24_combinations` |
| R08 | 「ポートが無い」が確定できる、ヒントが無い | P4 | `no_port_row_is_not_confirmable`、M4-1 |
| R09 | ページ遷移でクエリが残る、英語の空表示 | P2 | `page_change_clears_query`、空表示は screen-design §4 |
| R10 | 表示名とキーワード | P2 | `root_labels_match_screen_design`、`search_bo_finds_baud` |
| R11 | 実行後もパレットが残る | P2 | `action_without_next_page_closes_palette` |
| R12 | 新ペインが設定を複製 | P3 | `new_pane_uses_settings_defaults_not_the_source` |
| R13 | 最後のペインを閉じたときの表示 | P3。新しい未接続ペインに置き換え（§3.3）。レビューの「グリッドを残す」は、閉じた＝ペインを捨てる、なので採らない | `closing_last_pane_replaces_it_with_a_fresh_disconnected_pane` |
| R14 | 木がコアにある、比率の範囲 | 済（`7c390ba`、木を UI へ）。比率は herdr の 0.1〜0.9 に設計を変えた（スタッシュ、P0） | `layout_is_not_a_core_setting`、`resize_moves_the_divider_and_clamps` |
| R15 | 方向フォーカスが仮の矩形 | P3。実矩形 | `focus_while_zoomed_uses_unzoomed_rects_and_stays_zoomed`、M3-2 |
| R16 | エコー表示、未接続時の表示 | P1。未接続ではエコーしない（送っていない）がエラーは出す | `local_echo_flag_is_shown`、M1-1 |
| R17 | `Ok(0)` | 済（`d19bef5`） | `eof_is_port_lost` |
| R18 | `reconnect` が配線されていない | 対応しない（v1）。v5 §5.3 のとおり v1 は再接続しない。`reconnect = true` を読んだら状態行に注意「serial.reconnect は v1 では効かない」を出す（P1） | `status::tests::reconnect_key_warns` |
| R19 | テキストログ用の 2 本目のパーサ | 済（`d19bef5`） | `text_log_follows_handler_order_and_pause_discards` |
| R20 | フォント変更で測り直さない | P5 | `grid_size_follows_font_change` |
| R21 | 保存失敗で永久に保存停止 | P1 | 新規 `rs-term::ui::app::tests::save_failure_does_not_latch` |
| R22 | ログのパス方針 | P6（§9.1） | 新規 `log_dir_follows_the_settings_os_policy` |
| R23 | keymap が TOML でない | P2 | `keymap_file_parses_herdr_syntax` |
| R24 | スクロールのキー | P7。キーボードのスクロールはコピーモード（herdr）。送信キーは奪わない | `copy_mode_keys_match_herdr` |
| R25 | 曖昧幅 | 済（OK） | `wide_character_occupies_two_cells` |
| R26 | 色の解決 | 済（OK）。N19 はスタッシュ | `bright_foreground_reads_index_267` |
| R27 | 選択色、太字、カーソル | P5 | `selection_uses_palette_selection_color`、`cursor_cell_is_inverted_not_covered` |
| R28 | ログの語、名前、状態行 | P6。語は「新規（上書き）」「追記」。名前が空ならテンプレート | `log_text_asks_new_or_append_then_timestamp`、`empty_log_name_uses_the_template` |
| R29 | `SessionConfig::default` が UTF-8 | P1（コア 1 行） | 新規 `rs-term-core::session::tests::default_config_is_shift_jis` |
| R30 | キー表のテストが緩い | 済（`d19bef5`） | `keys_match_the_table`、`app_cursor_mode_sends_ss3_arrows` |
| R31 | DTR、ブレーク、RTS/CTS | 済（OK）。状態行の文言は P1 | `rtscts_shows_send_only` |
| R32 | OSC 52 無効 | 済（OK） | — |
| R33 | 単一ウィンドウ、メニュー無し | 済（OK）。v6 も同じ | M0-1 |
| R34 | ツールチェーン 1.88 と 1.95 | 済（`afec483` で `rust-version` 1.95）。設計を 1.95 に改訂。CI は P0 | CI の設定確認 |
| R35 | zip の中身 | P8 | M8-1 |
| R36 | `ui.ligatures` が効かない | P1。キーを schema 1 から外す | 新規 `rs-term-core::config::tests::ui_ligatures_is_an_unknown_key` |
| R37 | 「設定を初期化する」が無い | P2（§9.2） | 新規 `reset_settings_renames_the_broken_file_first` |
| R38 | ゴールデン、UI テスト | P0〜P2。パーサのゴールデンは `screen.rs` のテスト（`sgr_red_then_reset` ほか 7 本）で代える。`tests/fixtures/` は作らない（理由：入力と期待が短く、テストの中に置く方が読みやすい）。UI はモデルの単体テスト + 手動確認 + §11.4 | 列挙したテスト |
| R39 | write の再試行、ブレーク中の park、落ちたバイト数 | 済（`d19bef5`）。CTS 待ちの `TimedOut` 再試行は妥当（レビュー判定どおり） | `status_events_survive_a_full_data_channel`、`break_clears_without_a_sleep_inside_the_port` |
| R40 | path の変更禁止、ピン | 済（OK）。ピンは v1 では状態行に出さない | — |
| R41 | 2 秒の join が UI を止める | 済（`d19bef5`、`tick` で回収） | `disconnect_does_not_wait_for_a_slow_read` |
| R42 | Tokio 無し | 済（OK） | — |
| R43 | テーマ | 済（OK）。警告の表示は P1 | `unknown_theme_falls_back_without_rejecting`、M5-4 |
| R44 | 改行ページ | P4（受信 → 送信の 2 ページ） | 新規 `newline_asks_receive_then_transmit` |
| R45 | 丸めない、OS エラーを出す | P4（表示）。丸めない規則は維持 | `refused_parameters_are_reverted`、M4-1 |
| R46 | 壊れた設定の理由が見えない | P0、P1 | M0-3 |
| R47 | README が無い | P8 | README のキー表が `default_is_herdr` の表と同じ（レビュー時に目視） |
| R48 | `Ok(0)` の回帰テスト | 済 | `eof_is_port_lost` |
| R49 | 復元の 1 回だけの適用 | P3。v6 は `ResizableState` を使わず自前の木で描くので、復元は読み込み時の 1 回で終わる（§13 の 6） | `round_trip_keeps_shape_ratio_names_and_params` |
| R50 | UI フォント | 済（OK） | — |
| R51 | コピー、貼り付けのキー | 済（OK）。v6 も `Ctrl+Shift+C/V` | M7-1 |
| R52 | オープンが UI スレッド | 済（コア `d19bef5`）、UI 配線 P4 | `open_is_split_and_a_disconnect_cancels_it`、M4-2 |
| R53 | flush と停電の説明 | P6。ログ開始ページのフッタに 1 文（screen-design §4.8） | `root_labels_match_screen_design` の対象に含める |
| R54 | 空の送信文字列 | P4。状態行にエラー「送る文字列が空」 | 新規 `send_text_rejects_empty` |
| R55 | フォーカス枠の色 | 済（OK）。#4a8cff、1 px（screen-design §2） | M3-1 |
| R56 | 2 枚目の窓を作らない | 済（OK） | — |
| R57 | ズーム中のフォーカス | P3（herdr と同じ） | `focus_while_zoomed_uses_unzoomed_rects_and_stays_zoomed` |
| R58 | 読取タイムアウト既定 20 ms | 済（OK）。上限 100 ms は N13 | `read_timeout_above_100_ms_rejects_the_file` |

### 12.2 レビュー N1〜N31

| ID | 内容（短縮） | 対応 | 受入 |
| --- | --- | --- | --- |
| N1 | 確定の行番号が絞り込み後かも | 現象は起きない：`IndexPath` は絞り込み前の位置（`gpui-component` `state.rs:250-255`、`384`、`419`）。それでも行の値で引く設計にした（§6.3）。P2 | `confirm_reads_the_row_value_not_an_external_index` |
| N2 | 状態イベントが満杯で落ちる | 済（`d19bef5`） | `status_events_survive_a_full_data_channel` |
| N3 | read エラー後もハンドルを保持 | 済（`d19bef5`） | `read_error_releases_the_port_without_shutdown` |
| N4 | 大きな貼り付けが途中で切れる | 済（`d19bef5`、全部か無しか） | `a_large_paste_is_sent_whole`、`a_paste_over_the_limit_sends_nothing` |
| N5 | ドロップで flush しない | 済（`afec483`） | `dropping_the_engine_writes_the_buffer`、M6-2 |
| N6 | 時間による flush | 済（`afec483`、2 秒） | `a_quiet_buffer_is_flushed_by_tick` |
| N7 | コンソール窓 | P0 | M0-2 |
| N8 | `Shutdown` の失敗を即時に出す | 済（`d19bef5`、切断は待たず `tick` が報告） | `disconnect_does_not_wait_for_a_slow_read` |
| N9 | 接続成功で保存しない | P3（layout.toml） | `round_trip_keeps_shape_ratio_names_and_params`、M3-5 |
| N10 | ドラッグ中に毎回保存 | P3 | `save_is_debounced`、M3-5 |
| N11 | 切断でパーサの途中状態が残る | 済（`d19bef5`） | `disconnect_finishes_the_receive_stream` |
| N12 | リサイズ後に `Term` が古い | P5 | M5-3 |
| N13 | タイムアウト上限 60000 | 済（`7c390ba`、1〜100） | `read_timeout_above_100_ms_rejects_the_file` |
| N14 | Auto の CR 保留 | 済（`d19bef5`、保留しない） | `auto_holds_a_cr_across_chunks`（名前は旧来、内容は即時通過を見る） |
| N15 | `finish_codec` の結果を捨てる | 済（`d19bef5`） | `reset_flushes_a_dangling_lead` |
| N16 | 単独 CR でタイムスタンプ | 済（`afec483`） | `a_lone_cr_does_not_start_a_timestamped_line` |
| N17 | XON/XOFF で手動 RTS | 済（`d19bef5`）。パレットの RTS 行も `None` のときだけ（P4） | `manual_rts_only_without_flow_control`、`rts_page_only_when_flow_is_none` |
| N18 | Ctrl 記号、AltGr | 済（コア、`d19bef5`）。AltGr は P2 の配送で `key_char` を優先 | `ctrl_without_a_control_code_is_an_error`、`direct_chord_ignores_altgr_text` |
| N19 | `BrightForeground` の添字 | スタッシュ（P0 で適用） | `bright_foreground_reads_index_267` |
| N20 | `TryRecvError` の露出など | 済（`d19bef5` の `EventRx::try_recv` は `Option`）。設定の型はドメインの型として公開を続ける | — |
| N21 | 第三者ライセンス | P8 | M8-1 |
| N22 | release がテストを待たない | P0 | CI の設定確認 |
| N23 | `rustup target add` の引用符、版が 2 か所 | P0 | 同上 |
| N24 | 権限と SHA 固定 | P0 | 同上 |
| N25 | 拒否された設定値が残る | 済（`d19bef5`） | `refused_parameters_are_reverted` |
| N26 | 窓を閉じても終わらない | P0 | M0-2 |
| N27 | 残った lock | 済（`afec483`、OS ロック） | `a_leftover_lock_file_does_not_block_the_path` |
| N28 | 名前の制御文字 | 済（`afec483`） | `names_reject_traversal_and_reserved_devices` |
| N29 | 葉 ID の重複 | スタッシュ（P0）+ layout.toml（P3） | `saved_ratios_are_clamped_and_duplicates_refused`、`duplicate_ids_reject_the_file` |
| N30 | sleep 頼みのテスト、実ポートを開くテスト | 済（`d19bef5`）。新しく書く UI テストも時刻を注入し sleep しない | — |
| N31 | 全ペインの再描画、セルごとの整形 | P0（ペインを個別 Entity）、P5（行単位の整形） | M4-5（性能の目視） |

## 13. v5 からの変更と理由

| # | v5 | v6 | 理由 |
| --- | --- | --- | --- |
| 1 | 分割キー `ctrl-shift-h/j`、閉じる `ctrl-shift-w`、次ペイン `ctrl-shift-right` | herdr 既定のプレフィックス方式（§3.2） | 拘束要件。R05、R06 |
| 2 | 「横に分割＝`h_resizable`」「縦に分割＝`v_resizable`」 | `pane.split_vertical`＝右に分割、`pane.split_horizontal`＝下に分割。表示名は方向で書く | 横／縦は読み手で逆になる（監査「Names that do not match」）。herdr の文書も「Split right / down」（`keyboard.mdx:25`） |
| 3 | 次ペインだけ | 方向フォーカス、入れ替え、巡回（前後）、直前、ズーム、サイズ変更モード、名前、消去、コピーモード | herdr のペイン操作の全部（§3.2）。採らないものは §3.5 |
| 4 | 比率 50〜950（千分率） | 0.1〜0.9、刻み 0.05 | herdr `layout.rs:628-634`、`app/api/panes.rs:737`。R14 |
| 5 | 最後のペインを閉じると「未接続に戻す」 | 新しい未接続ペインに置き換え。接続中・ログ中なら確認 | herdr の close はペインを捨てる。最後の 1 枚の確認は herdr の `confirm_close`（§3.3） |
| 6 | 分割の幾何は `gpui-component` の `h_resizable`/`v_resizable` と `ResizableState` | 自前の木（herdr `TileLayout` の移植）で矩形を計算し、絶対配置で描く。仕切りも自前 | 方向フォーカス、入れ替え、サイズ変更モード、ズームは矩形と比率の木が要る。`ResizableState` は px の配列しか持たず、herdr の意味論（比率の範囲、反対側の辺）を表せない。復元の 1 回問題（R49）も消える |
| 7 | 新ペインの扱い未記載（実装は複製） | 設定の既定値の未接続ペイン | R12、herdr の新ペインは新しいプロセス |
| 8 | 状態行の欄は「ポート、速度とフレーム、Link、ログ、最後のエラー」 | §5 の 9 欄、メッセージの重さと寿命、履歴 | 要件のモック s1〜s4 と「全エラーを状態行に」。R02、R03 |
| 9 | パレットの各ページの詳細なし | ページのモデル、フォーカス、Escape の段階、行の値で確定（§6） | R04、R09、R10、R11、N1 |
| 10 | 接続はポート、ボーレート、データ、パリティ、ストップ、フロー、DTR の 7 ページ | ポート、ボーレート、フレーム、フロー、DTR の 5 ページ | モック s3 のヒント文。R07 の 3 ページ連鎖の不具合が構造的に無くなる |
| 11 | ツールチェーン 1.88.0 | 1.95.0、1 か所 | 固定の `gpui-kit` が 1.95 を要する（`afec483`）。R34、N23 |
| 12 | キーマップは `KeyBinding::load` で読む独自書式 | herdr の `[keys]` 書式 | 利用者が herdr の設定をそのまま使える。プレフィックスを表せる。R23 |
| 13 | 分割木を settings.toml に保存 | layout.toml（UI 専用） | `7c390ba`、R14 |
| 14 | `ui.ligatures` を持つ | 削除 | R36 |
| 15 | ログは「新規か追記」 | 「新規（上書き）」「追記」 | 実際の動作が truncate であることを語に出す（R28、レビュー判定） |
| 16 | スクロールはキーボードとホイール | ホイール + コピーモード。PageUp などは送信する | herdr と同じ。送信キーを奪わない（R24） |
| 17 | `prefix+?`／`prefix+s` 無し | パレットを開く | herdr の help／settings の役（§3.4） |
| 18 | 選択はコピーキーで | 離したらコピー（`copy_on_select`）+ `Ctrl+Shift+C` | herdr `model.rs:1208` |
| 19 | 本番 UI の受入は Windows の手動のみ | 純粋モデルのテスト名を先に決め、手動手順をファイルに残す | 監査「A green test run is not acceptance」と、次の監査で照合できるように |
| 20 | `tests/fixtures/` のゴールデン | `screen.rs` 内のテストで代える | R38。短い入力はテストの中が読みやすい |

## 14. 未決（確認したいこと）

決めるまでは「既定」の列で実装する。違う指示があれば実装前にこの文書を直す。

| # | 質問 | 既定 |
| --- | --- | --- |
| Q1 | herdr 文書の直接コード（`ctrl+alt+h/j/k/l`、`ctrl+alt+d` ほか）を既定で有効にするか | 有効にしない。herdr の既定どおりプレフィックスのみ。見本ファイルを同梱 |
| Q2 | `Ctrl+B`（`0x02`）を常用する装置があるか。あるならプレフィックスを変えるか | `Ctrl+B` のまま。`Ctrl+B` 2 回で `0x02` を送れる |
| Q3 | herdr の `edit_scrollback`（`prefix+e`）の代わりに「スクロールバックをファイルに保存してメモ帳で開く」を入れるか | 入れない |
| Q4 | Tera Term 流の「右クリックで貼り付け」を入れるか（herdr には無い） | 入れない。右クリックは何もしない |
| Q5 | `Ctrl+Shift+K/D/B`（接続、切断、ブレーク）を残すか。herdr の流儀ならペイン以外の直接キーは最小 | 残す（v5 から継続、コアの文言が参照） |
| Q6 | 状態行のフロー欄。モック s1 は出し、s4 は出していない | 出す（s1 に合わせ、狭いときは §5.2 の順で落とす） |
| Q7 | 端末フォントの既定。モックは JetBrains Mono 風、v5 は Consolas | Consolas 14 px（Windows 標準で入っている） |

## 15. 確認に使ったもの

- herdr `/workspace/herdr` `ee5bfb5`：`src/config/model.rs`、`src/main.rs`、`src/layout.rs`、`src/client/shell/{actions,input,render,mouse,overlay_input}.rs`、`src/app/actions.rs`、`src/app/api/panes.rs`、`src/workspace/tab.rs`、`src/ui/{panes,tab_surface}.rs`、`docs/next/website/src/content/docs/keyboard.mdx`。
- `gpui-component` 0.9.0：`src/command/{command,item,state}.rs`、`locales/ui.yml`。
- `serialport` 4.10.1：`src/windows/enumerate.rs:410-418`（USB の `product` は `SPDRP_FRIENDLYNAME`、`manufacturer` は `SPDRP_MFG`）。
- rs-term：`git show afec483 d19bef5 7c390ba`、`git stash show -p stash@{0}`、`crates/rs-term-core/src/*.rs`、`crates/rs-term/src/ui/{actions,workspace,workspace_methods}.rs`。
- 入力：`rs-term-design-v5.md`、`rs-term-audit-report.md`、`rs-term-claude-review.md`、`rs-term-mock/s1..s4.{png,html}`、`base.css`。
- 実行していないこと：アプリの起動、Windows での確認、`cargo build`（この環境の rustc 1.85.1 では依存が要求する版に届かない）。
