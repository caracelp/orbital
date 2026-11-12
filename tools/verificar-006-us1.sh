#!/usr/bin/env bash
# Instancia, configuración, caché y bóveda propias; sin teclas ni mouse sintéticos (006).
# El multi-cursor se verifica con pruebas GTK en proceso; este script es la regresión AT-SPI:
# el editor sigue exponiendo el texto y un único cursor, y guardar no cambia el disco.
set -euo pipefail
cd "$(dirname "$0")/.."
test_root=$(mktemp -d -t orbital-006-us1-XXXXXX)
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
mkdir -p "$ORBITAL_TEST_VAULT/wiki" "$XDG_CONFIG_HOME/orbital" "$XDG_CACHE_HOME"
cat > "$ORBITAL_TEST_VAULT/wiki/principal.md" <<'MD'
# Principal

Línea uno.
Línea dos.
Línea tres.
MD
echo 'md_view: codigo' > "$XDG_CONFIG_HOME/orbital/estado.yaml"
sha256sum "$ORBITAL_TEST_VAULT/wiki/principal.md" > "$test_root/notas.sha256"
# Las pruebas GTK del multi-cursor, una por proceso.
for t in us6_cursors_edit_tanda_and_undo us6_cursors_backspace_delete_selections \
         us6_cursors_remove_and_limit us6_column_clamp_and_edges us6_rects_for_overlay; do
    cargo test --bin orbital "multi_cursor::tests::$t" -- --ignored --test-threads=1 --nocapture
done
cargo build
# argv[0]=orbital: si un sandbox la lanza con otro nombre de proceso, AT-SPI no la encuentra.
bash -c 'exec -a orbital ./target/debug/orbital "$1"' _ "$ORBITAL_TEST_VAULT" > "$test_root/orbital.log" 2>&1 &
PID=$!
export ORBITAL_PID="$PID"
if ! python3 -I tools/verificar_006_us1.py; then
    cat "$test_root/orbital.log"
    exit 1
fi
sha256sum -c "$test_root/notas.sha256"
printf '%s\n' 'Comprobaciones AT-SPI de la 006 ejecutadas. Pendiente para la usuaria: teclado y lector de pantalla.'
