#!/usr/bin/env bash
# Instancia, configuración, caché y bóveda propias; sin teclas sintéticas.
set -euo pipefail
cd "$(dirname "$0")/.."
test_root=$(mktemp -d -t orbital-005-us1-XXXXXX)
PID=''
KEEP="${1:-}"
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
cat > "$ORBITAL_TEST_VAULT/wiki/principal.md" <<'MD'
---
type: nota
estado: pausado
otra: pausado
---
# Principal

estado:: activo

- Inline de la sección: [%estado]
- Solo en YAML: [%otra]
- Sin valor: [%nada]
MD
cp "$ORBITAL_TEST_VAULT/wiki/principal.md" "$test_root/principal.original"
cat > "$XDG_CONFIG_HOME/orbital/estado.yaml" <<EOF_STATE
md_view: lectura
EOF_STATE
cargo build
start_test_app() {
    # argv[0]=orbital: si un sandbox la lanza con otro nombre de proceso, AT-SPI no la encuentra.
    bash -c 'exec -a orbital ./target/debug/orbital "$1"' _ "$ORBITAL_TEST_VAULT" > "$test_root/orbital.log" 2>&1 &
    PID=$!
    export ORBITAL_PID="$PID"
}
start_test_app
python3 -I tools/verificar_005_us1.py lectura
kill "$PID"
wait "$PID" 2>/dev/null || true
PID=''
start_test_app
python3 -I tools/verificar_005_us1.py codigo
# 1.4: el disco contiene el marcador, nunca el valor interpolado.
grep -q '\[%estado\]' "$ORBITAL_TEST_VAULT/wiki/principal.md"
grep -q '\[%otra\]' "$ORBITAL_TEST_VAULT/wiki/principal.md"
grep -q '\[%nada\]' "$ORBITAL_TEST_VAULT/wiki/principal.md"
# El frontmatter ya dice «pausado»: lo que importa es que el archivo siga byte a byte igual.
if grep -q '«faltante»' "$ORBITAL_TEST_VAULT/wiki/principal.md" ||
    ! cmp -s "$test_root/principal.original" "$ORBITAL_TEST_VAULT/wiki/principal.md"; then
    echo "ERROR: el disco recibió contenido generado"
    exit 1
fi
printf '%s\n' 'OK 1.4: el disco conserva los marcadores y no recibe el valor interpolado.'
if [ "${1:-}" = "--manual" ]; then
    printf '%s\n' 'Instancia temporal lista. Revisar Lectura/Código, edición alrededor del marcador y lector de pantalla.'
    read -r -p 'Pulsá Intro en esta terminal al terminar para cerrar solo la instancia de prueba. ' _
fi
printf '%s\n' 'Pruebas GTK y AT-SPI ejecutadas. T012 requiere revisar también teclado y lector de pantalla.'
