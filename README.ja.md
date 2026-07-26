# Binary Patcher

[中文](README.md) | [English](README.en.md)

---

バイナリパッチを作成・適用するためのツールです。ディレクトリ全体のパッチワークフローに対応しています。
パッチエンジンには [HDiffPatch](https://github.com/sisong/HDiffPatch) を使用し、FFI 静的リンクで統合。ビルド時に自動的に C ライブラリをダウンロードしてコンパイルします。

## 機能

- **単一ファイルパッチ** — 2 つのファイル間でパッチの作成・適用
- **ディレクトリバンドル** — `Old/` と `New/` を比較し、`manifest.json` + パッチ + 新規ファイルを自動生成
- **ワンクリック適用** — `apply_patch` がマニフェストを読み取り、SHA256 検証、バックアップ、パッチ適用を実行
- **ワンクリックロールバック** — `rollback_patch` がバックアップを復元し、追加されたファイルを削除
- **適応型メモリ/ストリーミング** — `--mode auto` はメモリ優先、OOM 時にファイル単位で自動ストリーミングに切替
- **低メモリストリーミング** — `--mode stream` でファイルストリームモードを強制、大容量ファイル向けにメモリ使用量を削減
- **安全保証**：
  - パストラバーサル対策（`../` を拒否）
  - パッチ前後での SHA256 検証
  - 検証失敗時に自動ロールバック
  - タイムスタンプ付きバックアップ（上書き防止）
  - マニフェストのフォーマット検証

## バイナリ

| ファイル | 用途 |
|----------|------|
| `binary_patcher` | パッチの作成（単一ファイルおよびディレクトリバンドル） |
| `apply_patch` | パッチバンドルをターゲットディレクトリに適用 |
| `rollback_patch` | 適用済みのパッチバンドルをロールバック |

## インストール

### ソースからビルド

```sh
git clone https://github.com/100pangci/binary_patcher.git
cd binary_patcher
cargo build --release
```

ビルド時に HDiffPatch C ライブラリを自動ダウンロードし静的リンクします。追加の依存関係は不要です。
コンパイルされたバイナリは `target/release/` に配置されます。

### パッケージ

`scripts/build.ps1` を実行すると、`Releases/binary_patcher_toolkit.zip` にパッケージ化します：

```powershell
.\scripts\build.ps1
```

## クイックスタート

### 1. ディレクトリパッチバンドルを生成

ディレクトリ構成：

```
Old/          ← 旧バージョンを配置
New/          ← 新バージョンを配置
Patch/        ← 自動生成
```

**初回実行：**

```sh
binary_patcher
```

`Old/`、`New/`、`Patch/` ディレクトリが自動生成されます。`Old/` に旧バージョン、`New/` に新バージョンを配置してください。

**2 回目の実行：**

```sh
binary_patcher
```

`Old/` と `New/` をスキャンし、各ファイルの SHA256 を計算して比較し、以下を生成します：

- `Patch/manifest.json` — 変更マニフェスト
- `Patch/**/*.patch` — 変更ファイルのバイナリパッチ
- `Patch/**/*.new` — 新規ファイルのコピー
- `Patch/README.txt` — エンドユーザー向け説明書

### 2. パッチバンドルを適用

```
旧バージョンのルート/
├── apply_patch
├── Patch/
│   ├── manifest.json
│   ├── ... .patch
│   └── ... .new
```

```sh
./apply_patch
```

以下の処理が実行されます：

1. 各ファイルが `old_sha256` と一致するか検証
2. 元のファイルを `*.backup_before_patch` としてバックアップ
3. HDiffPatch エンジンでパッチを適用
4. 出力が `new_sha256` と一致するか検証
5. 新規ファイルをコピー、削除されたファイルを削除

### 3. ロールバック

```sh
./rollback_patch
```

`*.backup_before_patch` バックアップを復元し、パッチで追加されたファイルを削除します。

## CLI リファレンス

### `binary_patcher`

| コマンド | 説明 |
|----------|------|
| （引数なし） | ワークスペースモード：`Old/`/`New/`/`Patch/` を初期化し、バンドルを生成 |
| `create <旧> <新> <パッチ>` | 2 つのファイルから単一のパッチファイルを作成 |
| `apply <旧> <パッチ> <出力>` | 単一のパッチファイルを適用 |
| `bundle --base-dir <パス>` | 指定したワークスペースディレクトリでバンドルを生成 |
| `--no-compress` | パッチ圧縮を無効化（デフォルト：zlib圧縮有効） |
| `--mode auto/stream/memory` | 差分モード：`auto` 自動選択（デフォルト）、`stream` 低メモリストリーミング、`memory` 全メモリ高品質 |
| `--format precise/fast` | 差分アルゴリズム：`precise` suffix-string（パッチ小、デフォルト）、`fast` hash（高速） |

### `apply_patch`

```sh
./apply_patch
```

### `rollback_patch`

```sh
./rollback_patch
```

## プロジェクト構造

```
.
├── build.rs                 # ビルドスクリプト：HDiffPatch C ライブラリを自動DL・コンパイル
├── .github/workflows/
│   ├── ci.yml               # CI: cargo check + test（マルチプラットフォーム）
│   └── build.yml            # Release: lint → test → ビルド → GitHub Release
├── scripts/
│   ├── build.ps1            # Windows ワンクリックビルド + パッケージ
│   └── gen_test_data.ps1    # テストデータ生成スクリプト
├── vendor/
│   └── hdiffpatch-sys/      # HDiffPatch C/C++ ラッパーコード
├── Cargo.toml
├── src/
│   ├── lib.rs               # ライブラリルート、全モジュールを公開
│   ├── main.rs              # binary_patcher エントリポイント
│   ├── bin/
│   │   ├── apply_patch.rs   # apply_patch エントリポイント
│   │   └── rollback_patch.rs# rollback_patch エントリポイント
│   ├── cli.rs               # コマンドライン引数解析（clap）
│   ├── ffi.rs               # HDiffPatch C ライブラリ FFI バインディング
│   ├── hdiffpatch.rs        # パッチ作成・適用のラッパー
│   ├── utils.rs             # SHA256、ファイル操作、パス安全性、バックアップ
│   ├── manifest.rs          # マニフェスト型、JSON シリアライズ、検証
│   ├── bundle.rs            # バンドル作成（Old/New → Patch）
│   ├── apply.rs             # バンドル適用ロジック
│   └── rollback.rs          # バンドルロールバックロジック
└── tests/
    └── integration_test.rs  # ユニット + 全フロー統合テスト
```

## セキュリティ

| 機能 | 説明 |
|------|------|
| **パストラバーサル対策** | マニフェスト内の全パスを検証し、`../` によるエスケープを拒否 |
| **マニフェスト検証** | ロード時にスキーマ、フィールド型、フォーマットバージョンを検証 |
| **SHA256 検証** | パッチ前後でファイルの SHA256 を検証し、不一致時は自動ロールバック |
| **安全なバックアップ** | `.backup_before_patch` サフィックスを使用し、既存時はタイムスタンプを追加 |

## 開発

### 環境要件

- Rust 2024 edition（MSRV 1.85+）

### コマンド

```sh
# ビルド
cargo build

# テスト実行
cargo test

# リリースビルド
cargo build --release
```

### Windows ワンクリックビルド

```powershell
.\scripts\build.ps1
```

スクリプトの自動処理：
1. `cargo build --release` で 3 つのバイナリをコンパイル（ビルド時に HDiffPatch C ライブラリを自動DL・コンパイル）
2. `Releases/binary_patcher_toolkit.zip` にパッケージ

### CI / CD

このプロジェクトは GitHub Actions を使用しています：

| ワークフロー | トリガー | 内容 |
|-------------|----------|------|
| **CI** | push / PR | `cargo check` + `cargo test`（Windows / Linux / macOS） |
| **Build & Release** | tag `v*` / 手動 | check → test → `cargo build --release` → HDiffPatch ダウンロード → パッケージ → GitHub Release に公開 |

### TODO

- [x] プレビルドバイナリの提供

## 技術スタック

| 分野 | 選定 |
|------|------|
| 言語 | Rust（edition 2024） |
| CLI フレームワーク | clap（derive モード） |
| シリアライズ | serde + serde_json |
| ハッシュ | SHA-256（ring crate、アセンブリ最適化） |
| ディレクトリ走査 | walkdir |
| 時間処理 | chrono |
| TTY 検出 | atty |
| エラーハンドリング | anyhow |
| ビルド依存 | cc（C/C++ コンパイル）、reqwest + zip（HDiffPatch 自動DL） |
| パッチエンジン | [HDiffPatch](https://github.com/sisong/HDiffPatch)（FFI 静的リンク） |

## ライセンス

[Mozilla Public License 2.0](LICENSE) のもとで公開されています。

## 謝辞

- [HDiffPatch](https://github.com/sisong/HDiffPatch) — バイナリ差分・パッチエンジン
- オリジナルの [binary_patcher](https://github.com/100pangci/binary_patcher) Python プロジェクト
