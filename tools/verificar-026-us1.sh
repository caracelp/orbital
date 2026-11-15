#!/usr/bin/env bash
# Validación aislada 026 (ids de bloque al guardar). Bóveda y configuración propias, sin teclas sintéticas.
set -euo pipefail
cd "$(dirname "$0")/.."
test_root=$(mktemp -d -t orbital-026-us1-XXXXXX)
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
mkdir -p "$ORBITAL_TEST_VAULT/wiki" "$ORBITAL_TEST_VAULT/raw" "$XDG_CONFIG_HOME/orbital" "$XDG_CACHE_HOME"
printf 'Vacía.\n' > "$ORBITAL_TEST_VAULT/wiki/nota.md"
printf 'Otra nota.\n' > "$ORBITAL_TEST_VAULT/wiki/otra.md"
printf 'Fuente original.\n\n- sin ids\n' > "$ORBITAL_TEST_VAULT/raw/fuente.md"
(cd "$ORBITAL_TEST_VAULT" && sha256sum raw/fuente.md > "$test_root/raw.sha256")
for t in ids_al_guardar_salvo_el_bloque_del_cursor abrir_y_dejar_sin_tocar_no_modifica; do
    cargo test -q --bin orbital "editor::ids_bloque::ids_bloque_tests::$t" -- --ignored --exact
done
cargo build
# argv[0]=orbital: si un sandbox la lanza con otro nombre de proceso, AT-SPI no la encuentra.
bash -c 'exec -a orbital ./target/debug/orbital "$1"' _ "$ORBITAL_TEST_VAULT" > "$test_root/orbital.log" 2>&1 &
PID=$!
export ORBITAL_PID="$PID"
if ! python3 -I tools/verificar_026_us1.py; then
    cat "$test_root/orbital.log"
    exit 1
fi
(cd "$ORBITAL_TEST_VAULT" && sha256sum -c --quiet "$test_root/raw.sha256")
echo "OK: raw/ intacto"
printf '%s\n' 'Pruebas GTK y AT-SPI ejecutadas. Queda para la usuaria: teclado y lector de pantalla.'
