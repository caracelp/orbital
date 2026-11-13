#!/usr/bin/env bash
# Validación aislada 010 (historial de versiones). Bóveda y configuración propias, sin teclas sintéticas.
set -euo pipefail
cd "$(dirname "$0")/.."
test_root=$(mktemp -d -t orbital-010-us1-XXXXXX)
PID=''
cleanup() {
    if [ -n "$PID" ]; then
        kill "$PID" 2>/dev/null || true
        wait "$PID" 2>/dev/null || true
    fi
    rm -rf -- "$test_root"
}
trap cleanup EXIT
export XDG_CONFIG_HOME="$test_root/config"
export XDG_CACHE_HOME="$test_root/cache"
export ORBITAL_INSTANCIA_PROPIA=1
export ORBITAL_TEST_VAULT="$test_root/vault"
mkdir -p "$ORBITAL_TEST_VAULT/wiki" "$XDG_CONFIG_HOME/orbital" "$XDG_CACHE_HOME"
printf 'prehistórico\n' > "$ORBITAL_TEST_VAULT/wiki/nota.md"
printf 'Sin versiones todavía\n' > "$ORBITAL_TEST_VAULT/wiki/otra.md"
cargo test --bin orbital us2_historial_dialogo -- --ignored --test-threads=1 --nocapture
cargo build
# argv[0]=orbital: si un sandbox la lanza con otro nombre de proceso, AT-SPI no la encuentra.
bash -c 'exec -a orbital ./target/debug/orbital "$1"' _ "$ORBITAL_TEST_VAULT" > "$test_root/orbital.log" 2>&1 &
PID=$!
export ORBITAL_PID="$PID"
if ! python3 -I tools/verificar_010_us1.py; then
    cat "$test_root/orbital.log"
    exit 1
fi
printf '%s\n' 'Pruebas GTK y AT-SPI ejecutadas. Queda para la usuaria: teclado y lector de pantalla.'
