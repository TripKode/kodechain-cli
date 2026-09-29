#!/usr/bin/env bash
# dist-windows.sh — compila kdc para Windows (x64 MSVC) y arma el .zip listo para usar.
# Uso: ./dist-windows.sh
# Requiere: cargo-xwin (cargo install cargo-xwin). Descarga el toolchain
# MSVC automáticamente, sin sudo ni Visual Studio.
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

echo "==> 1/3 build Windows x86_64 (release)"
rustup target list --installed | grep -q "x86_64-pc-windows-msvc" \
    || rustup target add x86_64-pc-windows-msvc
cargo xwin build --release --target x86_64-pc-windows-msvc

EXE="target/x86_64-pc-windows-msvc/release/kdc.exe"
[ -f "$EXE" ] || { echo "❌ no se generó $EXE"; exit 1; }

echo "==> 2/3 smoke test del .exe (ayuda, sin nodo)"
# Wine si existe; si no, solo se verifica el binario PE.
if command -v wine >/dev/null 2>&1; then
    wine "$EXE" --version
else
    echo "    (sin wine: se verifica cabecera PE)"
    head -c 2 "$EXE" | grep -q "MZ" && echo "    PE header OK"
fi

echo "==> 3/3 empaquetar"
VER=$(grep '^version' Cargo.toml | head -n 1 | cut -d'"' -f2)
DIST="kdc-windows-x86_64-v$VER"
rm -rf "$DIST" "$DIST.zip"
mkdir -p "$DIST"
cp "$EXE" "$DIST/"
cp README.md "$DIST/"
(cd "$DIST" && sha256sum kdc.exe README.md > SHA256SUMS.txt)
if command -v zip >/dev/null 2>&1; then
    zip -r -q "$DIST.zip" "$DIST"
else
    python3 - "$DIST" <<'EOF'
import sys, zipfile, os
d = sys.argv[1]
with zipfile.ZipFile(d + ".zip", "w", zipfile.ZIP_DEFLATED) as z:
    for root, _, files in os.walk(d):
        for f in files:
            p = os.path.join(root, f)
            z.write(p, os.path.relpath(p, "."))
EOF
fi
ls -la "$DIST.zip"
echo ""
echo "✅ $DIST.zip listo para distribuir (descomprimir y usar, sin instalación)"
