#!/usr/bin/env bash
# Instancia, configuración, caché y bóveda propias; sin teclas ni mouse sintéticos (009).
#
# Los archivos de raw/ no son activables por AT-SPI (ni por el selector ni por las filas de
# búsqueda, que solo exponen listitem.scroll-to): la verificación de la interfaz va en una prueba
# GTK en proceso y acá queda la regresión de que la app abre y la bóveda queda intacta.
set -euo pipefail
cd "$(dirname "$0")/.."
test_root=$(mktemp -d -t orbital-009-us1-XXXXXX)
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
mkdir -p "$ORBITAL_TEST_VAULT/raw" "$XDG_CONFIG_HOME/orbital" "$XDG_CACHE_HOME"
printf '%s\n' '%PDF-1.4 de prueba.' > "$ORBITAL_TEST_VAULT/raw/libro.pdf"
cargo test -p orbital-vault --test enlaces -- --nocapture
cargo test --bin orbital us9_ -- --ignored --test-threads=1 --nocapture
cargo build
bash -c 'exec -a orbital ./target/debug/orbital "$1"' _ "$ORBITAL_TEST_VAULT" > "$test_root/orbital.log" 2>&1 &
PID=$!
export ORBITAL_PID="$PID"
sleep 6
if ! kill -0 "$PID" 2>/dev/null; then
    cat "$test_root/orbital.log"
    exit 1
fi
printf '%s\n' 'La instancia abre sin errores.'
if grep -qiE "panic|error crítico" "$test_root/orbital.log"; then
    cat "$test_root/orbital.log"
    exit 1
fi
printf '%s\n' 'Comprobaciones 009 ejecutadas. Pendiente para la usuaria: teclado, mouse y lector de pantalla.'
