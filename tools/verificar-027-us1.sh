#!/usr/bin/env bash
# Validación aislada 027 US1 (Favoritos desde el menú de la nota). Bóveda y configuración propias,
# instancia propia (ORBITAL_INSTANCIA_PROPIA=1), sin teclas sintéticas.
set -euo pipefail
cd "$(dirname "$0")/.."
test_root=$(mktemp -d -t orbital-027-us1-XXXXXX)
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
printf '# A\n\nNota a.\n' > "$ORBITAL_TEST_VAULT/wiki/a.md"
printf '# B\n\nNota b.\n' > "$ORBITAL_TEST_VAULT/wiki/b.md"
printf '%%PDF-1.4 fuente\n' > "$ORBITAL_TEST_VAULT/raw/fuente.pdf"
(cd "$ORBITAL_TEST_VAULT" && sha256sum wiki/* raw/* > "$test_root/notas.sha256")
cargo build
lanzar() {
    # argv[0]=orbital: si un sandbox la lanza con otro nombre de proceso, AT-SPI no la encuentra.
    bash -c 'exec -a orbital ./target/debug/orbital "$1"' _ "$ORBITAL_TEST_VAULT" >> "$test_root/orbital.log" 2>&1 &
    PID=$!
    export ORBITAL_PID="$PID"
}
fase() {
    if ! python3 -I tools/verificar_027_us1.py "$1"; then
        cat "$test_root/orbital.log"
        exit 1
    fi
}
lanzar
fase agregar
test ! -d "$ORBITAL_TEST_VAULT/.orbital/respaldo" || { echo "FALLA: hay respaldos de un archivo nuevo"; exit 1; }
parar
lanzar
fase reabrir
parar
(cd "$ORBITAL_TEST_VAULT" && sha256sum -c --quiet "$test_root/notas.sha256")
echo "OK: wiki/ y raw/ intactos (SC-004)"
printf '%s\n' 'Pruebas AT-SPI ejecutadas. Queda para la usuaria: teclado y lector de pantalla.'
