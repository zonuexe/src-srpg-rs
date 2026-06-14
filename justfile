# Just は Nix 開発シェルに含めてあるタスクランナー / Task runner included in the Nix devShell.

default:
    @just --list

# 開発サーバ起動 (http://127.0.0.1:8080)
serve:
    trunk serve

# 本番ビルド (dist/ に出力)
build:
    trunk build --release

# 全クレートの型チェック (wasm32 ターゲット)
check:
    cargo check --workspace --target wasm32-unknown-unknown

# ネイティブ単体テスト (src-core のみ)
test:
    cargo test -p src-core

# nextest: プロセス分離が必要なとき / process-isolated run (macOS では cargo test の方が速い)
test-nextest:
    cargo nextest run -p src-core

# Lint
lint:
    cargo clippy --workspace --target wasm32-unknown-unknown --all-targets -- -D warnings

fmt:
    cargo fmt --all

clean:
    rm -rf dist target

# 開発サーバを起こした状態を保ちつつブラウザで確認する補助:
# - foreground で trunk serve
# - Claude Preview MCP からは `.claude/launch.json` の "trunk-serve" 構成で起動可能
verify:
    @echo "Open http://127.0.0.1:8080/ in your browser (or use Claude Preview MCP)"
    trunk serve
