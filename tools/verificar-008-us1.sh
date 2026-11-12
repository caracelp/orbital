#!/usr/bin/env bash
# Instancia, configuración, caché y bóveda propias; sin teclas ni mouse sintéticos (008).
set -euo pipefail
cd "$(dirname "$0")/.."
test_root=$(mktemp -d -t orbital-008-us1-XXXXXX)
KEEP="${1:-}"
PID=''
cleanup() {
    if [ -n "$PID" ]; then kill "$PID" 2>/dev/null || true; wait "$PID" 2>/dev/null || true; fi
    [[ "$KEEP" == "--keep" ]] || rm -rf -- "$test_root"
}
trap cleanup EXIT
export XDG_CONFIG_HOME="$test_root/config"
export XDG_CACHE_HOME="$test_root/cache"
export ORBITAL_INSTANCIA_PROPIA=1
export ORBITAL_TEST_VAULT="$test_root/vault"
mkdir -p "$ORBITAL_TEST_VAULT/wiki" "$XDG_CONFIG_HOME/orbital" "$XDG_CACHE_HOME"
printf 'Nota A.\nEnlaza a [[b]] y [[c]].\n' > "$ORBITAL_TEST_VAULT/wiki/a.md"
printf 'Nota B.\nVuelve a [[a]].\n' > "$ORBITAL_TEST_VAULT/wiki/b.md"
printf 'Nota C.\n' > "$ORBITAL_TEST_VAULT/wiki/c.md"
echo 'md_view: codigo' > "$XDG_CONFIG_HOME/orbital/estado.yaml"
sha256sum "$ORBITAL_TEST_VAULT"/wiki/*.md > "$test_root/notas.sha256"
cargo test --bin orbital grafo::tests -- --nocapture
cargo build
bash -c 'exec -a orbital ./target/debug/orbital "$1"' _ "$ORBITAL_TEST_VAULT" > "$test_root/orbital.log" 2>&1 &
PID=$!
export ORBITAL_PID="$PID"
if ! python3 -I tools/verificar_008_us1.py; then
    cat "$test_root/orbital.log"
    exit 1
fi
sha256sum -c "$test_root/notas.sha256"
printf '%s\n' 'Comprobaciones 008 ejecutadas. Pendiente para la usuaria: teclado, mouse y lector de pantalla.'
