#!/usr/bin/env bash
# Instancia, configuración, caché y bóveda propias; sin teclas sintéticas.
set -euo pipefail
cd "$(dirname "$0")/.."
test_root=$(mktemp -d -t orbital-004-us8-XXXXXX)
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
printf '%s\n' 'Texto sobre conservacion de la energia.' > "$ORBITAL_TEST_VAULT/wiki/nota.md"
# Lo que AT-SPI no acciona (elegir comando con Intro y flechas) va en la prueba GTK en proceso.
cargo test --bin orbital us8_palette_widgets_choose_and_disabled -- --ignored --test-threads=1 --nocapture
cargo build
start_test_app() {
    # argv[0]=orbital: si un sandbox la lanza con otro nombre de proceso, AT-SPI no la encuentra.
    bash -c 'exec -a orbital ./target/debug/orbital "$1"' _ "$ORBITAL_TEST_VAULT" > "$test_root/orbital.log" 2>&1 &
    PID=$!
    export ORBITAL_PID="$PID"
}
start_test_app
python3 -I tools/verificar_004_us8.py sinnota
kill "$PID"
wait "$PID" 2>/dev/null || true
PID=''
start_test_app
python3 -I tools/verificar_004_us8.py connota
if [ "${1:-}" = "--manual" ]; then
    printf '%s\n' 'Instancia temporal lista. Revisar Ctrl+Mayús+P, filtrado, flechas/Intro/Escape, fila desactivada y lector de pantalla.'
    read -r -p 'Pulsá Intro en esta terminal al terminar para cerrar solo la instancia de prueba. ' _
fi
printf '%s\n' 'Pruebas GTK y AT-SPI ejecutadas. T045 requiere revisar también teclado y lector de pantalla.'
