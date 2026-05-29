# jiwa 競合・類似ツール調査

調査日: 2026-05-29

## 結論

「ターミナル文字演出」という広いカテゴリは混雑しているが、**jiwa の正確な座標
（色がにじむ reveal を、依存ゼロ単一バイナリの Unix パイプフィルタで）には有力な
双子がいない**。隣人はいるが、いずれも放置気味・色だけ・重量級のどれか。

## 2層構造で見る

### 広い層「ターミナル文字演出」= 混雑（正面では勝てない）

| ツール | 言語 | 何をする | ★ | 最終更新 | jiwa との関係 |
|---|---|---|---|---|---|
| **TerminalTextEffects (TTE)** | Python | 36〜70の効果エンジン。`print` 効果がタイプライタ風 reveal。stdin パイプ対応 | 約4,000 | 2026-05-10（現役） | **最大の競合**。ただし多機能・要 Python ランタイム・重量級 |
| **lolcat** (busyloop) | Ruby | レインボー着色のみ（reveal しない） | 約6,500 | 古い（安定） | **周辺＝ポジションの手本**。パイプ単機能フィルタの元祖 |

### 狭い層「色のにじむ reveal を依存ゼロ単一バイナリで」= ほぼ空席

Rust 帯の隣人はすべて低 DL・更新停止：

| クレート | 説明 | DL | 最新 | 重なり |
|---|---|---|---|---|
| typewriter-cli (jackboxx) | タイプライタ CLI、stdin 対応 | 1,441 | 2023-04 放置 | 直接だが極小 |
| typout | typewriter output stream | 8,958 | 2020-04 古い | 直接 |
| terani | Terminal Animator Engine | 少 | 2024-06 | 直接（エンジン型） |
| print_typewriter | 1文字ずつ表示（lib） | 6,696 | — | 周辺 |

色のにじみ（per-grapheme フェードイン）を備えた reveal は、これらのどれにも無い。

### 見えざる競合: `pv -qL 10`

`pv -qL 10` のワンライナーでタイプライタ風が作れてしまう。**jiwa の付加価値は
「色のにじみ」という見た目の部分**。ここを GIF で即伝えないと「pv で足りる」層に
は響かない。

## 差別化ポイント（宣伝で推す）

- **「lolcat の動く版」** — 一言コピー。lolcat 6.5k★ の認知に相乗り。TTE と多機能で
  張り合わない
- **Rust・依存ゼロ・単一バイナリ** — Python(TTE)/Node(typewriter-cli) のランタイム不要。
  `cargo install` / brew / pacman で配れる軽さ。lolcat と同じ手触り
- **CLI 兼 crate** の二刀流。Rust の既存 typewriter クレートは磨きが甘く空席
- **グラフェム単位の正確さ**（絵文字・結合文字・CJK を1文字扱い）

## 弱み・リスク

- TTE の認知度に正面では勝てない → **単機能・軽さ・パイプ適性に振り切る**
- "jiwa" は検索ヒットしない無名語 → **タグライン一行 + crates.io keywords**で発見性を補う
- reveal は「見た瞬間」が価値 → **README に GIF/動画必須**（vhs で録画）

## ニッチ混雑度

- 「ターミナル文字演出」全体: **混んでいる**（TTE が決定版、lolcat が着色定番）
- 「パイプ reveal 専用・依存ゼロ・色フェード」: **空いている**（誰も本気で取りに来ていない）

## 主な出典（確認日 2026-05-29）

- TTE: https://github.com/ChrisBuilds/terminaltexteffects （★約4,000、2026-05-10 更新）
- lolcat: https://github.com/busyloop/lolcat （★約6,500）
- typewriter-cli: https://crates.io/crates/typewriter-cli （DL 1,441、2023-04）
- typout: https://crates.io/crates/typout （DL 8,958、2020-04）
- terani: https://lib.rs/crates/terani （2024-06）
- vhs（録画・補完関係）: https://github.com/charmbracelet/vhs
- pv タイプライタ: https://www.baeldung.com/linux/terminal-simulate-typing-effect

注記: スター数・DL 数は時点変動の概数。HN(2024)/typout(2020)/typewriter-cli(2023) は
1年以上前の情報。
