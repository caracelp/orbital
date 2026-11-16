#!/usr/bin/env bash
# Validación aislada 028 US4 (enfocar y miga de pan). Bóveda y configuración propias, instancia
# propia (ORBITAL_INSTANCIA_PROPIA=1), sin teclas sintéticas.
set -euo pipefail
cd "$(dirname "$0")/.."
test_root=$(mktemp -d -t orbital-028-us4-XXXXXX)
PID=''
parar() {
    if [ -n "$PID" ]; then
        kill "$PID" 2>/dev/null || true
        wait "$PID" 2>/dev/null || true
        PID=''
    fi
}
cleanup() {
    parar
    rm -rf -- "$test_root"
}
trap cleanup EXIT
export XDG_CONFIG_HOME="$test_root/config"
export XDG_CACHE_HOME="$test_root/cache"
export ORBITAL_INSTANCIA_PROPIA=1
export ORBITAL_TEST_VAULT="$test_root/vault"
mkdir -p "$ORBITAL_TEST_VAULT/wiki" "$ORBITAL_TEST_VAULT/raw" "$XDG_CONFIG_HOME/orbital" "$XDG_CACHE_HOME"
printf '# Nota\nintro\n\n## Historia\nTexto.\n- Frutas\n  - Manzana\n\n## Ejemplos\nuno\n' > "$ORBITAL_TEST_VAULT/wiki/outline.md"
(cd "$ORBITAL_TEST_VAULT" && sha256sum wiki/* > "$test_root/notas.sha256")
cargo build
bash -c 'exec -a orbital ./target/debug/orbital "$1"' _ "$ORBITAL_TEST_VAULT" >> "$test_root/orbital.log" 2>&1 &
PID=$!
export ORBITAL_PID="$PID"
if ! python3 -I tools/verificar_028_us4.py; then
    cat "$test_root/orbital.log"
    exit 1
fi
parar
(cd "$ORBITAL_TEST_VAULT" && sha256sum -c --quiet "$test_root/notas.sha256")
echo "OK: notas intactas (SC-004)"
printf '%s\n' 'AVISO: AT-SPI no puede posicionar el cursor dentro de GtkSourceView ni ver qué está oculto; eso lo cubren las pruebas GTK en proceso (enfoque_y_miga_de_pan).'
