#!/usr/bin/env bash
# install.sh — instala kdc (KodeChain CLI) en Linux.
# Uso: ./install.sh [--prefix ~/.cargo/bin] [--no-test]
# Hace: toolchain Rust (si falta) → build release → instala binario →
# verifica con --version + pruebas offline.
set -euo pipefail

PREFIX="${1:-$HOME/.cargo/bin}"
RUN_TESTS=1
for arg in "$@"; do
    case "$arg" in
        --no-test) RUN_TESTS=0 ;;
        --prefix=*) PREFIX="${arg#--prefix=}" ;;
        --prefix) shift; PREFIX="${1:-$HOME/.cargo/bin}" ;;
    esac
done

cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

echo "==> 1/5 toolchain Rust"
if ! command -v cargo >/dev/null 2>&1; then
    echo "    instalando rustup (minimal)..."
    curl -sSf https://sh.rustup.rs -o /tmp/rustup-kdc.sh
    sh /tmp/rustup-kdc.sh -y --profile minimal --default-toolchain stable
    export PATH="$HOME/.cargo/bin:$PATH"
    rm -f /tmp/rustup-kdc.sh
fi
cargo --version
rustc --version

echo "==> 2/5 build release"
cargo build --release --locked 2>/dev/null || cargo build --release

BIN="$PWD/target/release/kdc"
[ -x "$BIN" ] || { echo "❌ no se generó $BIN"; exit 1; }

if [ "$RUN_TESTS" = "1" ]; then
    echo "==> 3/5 pruebas offline (unit + CLI, sin nodo)"
    cargo test --lib 2>&1 | tail -n 1
    cargo test --test cli 2>&1 | tail -n 1
else
    echo "==> 3/5 pruebas omitidas (--no-test)"
fi

echo "==> 4/5 instalar en $PREFIX"
mkdir -p "$PREFIX"
cp -f "$BIN" "$PREFIX/kdc"
chmod +x "$PREFIX/kdc"

echo "==> 5/5 verificación"
"$PREFIX/kdc" --version
"$PREFIX/kdc" manual overview | head -n 3
case ":$PATH:" in
    *":$PREFIX:"*) ;;
    *) echo "⚠️  $PREFIX no está en tu PATH. Agrega: export PATH=\"$PREFIX:\$PATH\"" ;;
esac
echo ""
echo "✅ kdc listo. Empieza con: kdc manual overview"
echo "   Red local: kdc node start --mode all   (desde el checkout del engine o con --engine-dir)"
