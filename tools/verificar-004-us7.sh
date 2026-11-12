#!/usr/bin/env bash
# Instancia, configuración, caché y bóveda propias; sin teclas sintéticas.
set -euo pipefail
cd "$(dirname "$0")/.."
test_root=$(mktemp -d -t orbital-004-us7-XXXXXX)
PID=''
cleanup() {
    if [ -n "$PID" ]; then kill "$PID" 2>/dev/null || true; wait "$PID" 2>/dev/null || true; fi
    rm -rf -- "$test_root"
}
trap cleanup EXIT
export XDG_CONFIG_HOME="$test_root/config"
export XDG_CACHE_HOME="$test_root/cache"
export ORBITAL_INSTANCIA_PROPIA=1
export ORBITAL_TEST_VAULT="$test_root/vault"
mkdir -p "$ORBITAL_TEST_VAULT/wiki" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME"
printf '%s\n' 'Texto #fisica.' > "$ORBITAL_TEST_VAULT/wiki/nota.md"
sha256sum "$ORBITAL_TEST_VAULT/wiki/nota.md" > "$test_root/notas.sha256"
cargo test --bin orbital us7_saved_widgets_persistence_and_conflict -- --ignored --test-threads=1 --nocapture
cargo build
start_test_app() {
    # argv[0]=orbital: si un sandbox la lanza con otro nombre de proceso, AT-SPI no la encuentra.
    bash -c 'exec -a orbital ./target/debug/orbital "$1"' _ "$ORBITAL_TEST_VAULT" > "$test_root/orbital.log" 2>&1 &
    PID=$!
    export ORBITAL_PID="$PID"
}
start_test_app
python3 -I tools/verificar_004_us7.py crear
kill "$PID"
wait "$PID" 2>/dev/null || true
PID=''
start_test_app
python3 -I tools/verificar_004_us7.py reinicio
sha256sum -c "$test_root/notas.sha256"
printf '%s\n' 'Comprobaciones US7 ejecutadas; revisar también teclado/lector de pantalla antes de cerrar T042.'
