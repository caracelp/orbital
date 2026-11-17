#!/usr/bin/env bash
# Bases: instancia propia, archivos temporales, sin teclado sintético ni bóveda real.
set -euo pipefail
cd "$(dirname "$0")/.."
test_root=$(mktemp -d -t orbital-029-us1-XXXXXX)
PID=''
stop() {
    if [ -n "$PID" ]; then
        kill "$PID" 2>/dev/null || true
        wait "$PID" 2>/dev/null || true
        PID=''
    fi
}
cleanup() { stop; rm -rf -- "$test_root"; }
trap cleanup EXIT
export XDG_CONFIG_HOME="$test_root/config"
export XDG_CACHE_HOME="$test_root/cache"
export ORBITAL_INSTANCIA_PROPIA=1
export ORBITAL_TEST_VAULT="$test_root/vault"
mkdir -p "$ORBITAL_TEST_VAULT/wiki" "$ORBITAL_TEST_VAULT/raw" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME"
cat > "$ORBITAL_TEST_VAULT/wiki/a.md" <<'NOTE'
---
title: Alfa
precio: 10
estado: pendiente
---
# Cuerpo intacto

Este texto no cambia al editar una celda.
NOTE
cat > "$ORBITAL_TEST_VAULT/wiki/b.html" <<'NOTE'
<!DOCTYPE html><html><head><script type="application/yaml">
title: Beta
precio: 20
estado: pendiente
</script></head><body><p>HTML intacto.</p></body></html>
NOTE
printf 'Fuente intocable\n' > "$ORBITAL_TEST_VAULT/raw/fuente.txt"
cat > "$ORBITAL_TEST_VAULT/wiki/precios.base" <<'BASE'
format_version: 1
view: table
columns:
  - property: precio
    label: Precio
    type: number
filters:
  - property: estado
    value: pendiente
sort:
  property: precio
  descending: false
BASE
CARGO_BUILD_JOBS=1 cargo build --offline
launch() {
    bash -c 'exec -a orbital ./target/debug/orbital "$1"' _ "$ORBITAL_TEST_VAULT" > "$test_root/orbital.log" 2>&1 &
    PID=$!
    export ORBITAL_PID="$PID"
}
phase() {
    if ! python3 -I tools/verificar_029_us1.py "$1"; then
        cat "$test_root/orbital.log"
        exit 1
    fi
}
launch
phase editar
stop
launch
phase reabrir
stop
printf '%s\n' 'OK: Bases por AT-SPI (crear, editar MD/HTML, cambio externo y reabrir).' 'Pendiente: teclado físico y lector de pantalla.'
