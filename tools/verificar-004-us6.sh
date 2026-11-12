#!/usr/bin/env bash
# Validación aislada US6. No usa la app ni la bóveda de la usuaria ni teclas sintéticas.
set -euo pipefail
cd "$(dirname "$0")/.."
test_root=$(mktemp -d -t orbital-004-us6-XXXXXX)
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
cat > "$ORBITAL_TEST_VAULT/wiki/uno.md" <<'MD'
---
tags: [fisica, otra]
---
Texto #fisica #fisicas `#fisica`.
MD
cat > "$ORBITAL_TEST_VAULT/wiki/dos.html" <<'HTML'
<html><head><script type="application/yaml">
tags: [fisica/termodinamica, ciencia]
</script></head><body><p>#fisica/hija</p><code>#fisica</code></body></html>
HTML
cargo test --bin orbital us6_tag_rename_buffers -- --ignored --test-threads=1 --nocapture
cargo build
# argv[0]=orbital: si un sandbox la lanza con otro nombre de proceso, AT-SPI no la encuentra.
bash -c 'exec -a orbital ./target/debug/orbital "$1"' _ "$ORBITAL_TEST_VAULT" > "$test_root/orbital.log" 2>&1 &
PID=$!
export ORBITAL_PID="$PID"
if ! python3 -I tools/verificar_004_us6.py; then
    cat "$test_root/orbital.log"
    exit 1
fi
if [ "${1:-}" = "--manual" ]; then
    printf '%s\n' 'Instancia temporal lista. Revisar con teclado/lector de pantalla: botones del árbol, nombre nuevo, confirmación y Deshacer; repetir con nota abierta/editada.'
    read -r -p 'Pulsá Intro al terminar; se cerrará solo esta instancia y se borrarán sus datos temporales. ' _
fi
printf '%s\n' 'Pruebas GTK y AT-SPI ejecutadas. T039 requiere revisar también teclado y lector de pantalla.'
