#!/usr/bin/env bash
# Instancia y datos propios; no envía teclas al escritorio ni usa la bóveda real.
set -euo pipefail
cd "$(dirname "$0")/.."
test_root=$(mktemp -d -t orbital-004-us5-XXXXXX)
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
tags: [Física]
---
Texto #Física y #química.
Ignorar #123 #ff00ff `#código`.
MD
cat > "$ORBITAL_TEST_VAULT/wiki/dos.md" <<'MD'
---
tags: [Física/Termodinámica]
---
Texto #Física/Termodinámica.
MD
cat > "$ORBITAL_TEST_VAULT/wiki/tres.html" <<'HTML'
<html><body>İ<p>#biología</p><code>#oculta</code></body></html>
HTML
cat > "$XDG_CONFIG_HOME/orbital/estado.yaml" <<EOF_STATE
md_view: codigo
recent_notes: { "$ORBITAL_TEST_VAULT": ["wiki/uno.md"] }
EOF_STATE
# Cada prueba GTK va en un proceso: GTK exige inicializarse siempre en el mismo hilo.
for test_case in us5_tag_widgets_panel us5_tag_widgets_completion us5_tag_widgets_reader; do
    cargo test --bin orbital "$test_case" -- --ignored --test-threads=1 --nocapture
done
cargo build
# argv[0]=orbital: si un sandbox la lanza con otro nombre de proceso, AT-SPI no la encuentra.
bash -c 'exec -a orbital ./target/debug/orbital "$1"' _ "$ORBITAL_TEST_VAULT" > "$test_root/orbital.log" 2>&1 &
PID=$!
export ORBITAL_PID="$PID"
if ! python3 -I tools/verificar_004_us5.py; then
    cat "$test_root/orbital.log"
    exit 1
fi
if [ "${1:-}" = "--manual" ]; then
    printf '%s\n' 'Instancia temporal lista. Revisar árbol, #fis y foco/flechas/Intro/Tab/Escape; Lectura: Tab e Intro sobre etiqueta; lector de pantalla.'
    read -r -p 'Pulsá Intro en esta terminal al terminar para cerrar solo la instancia de prueba. ' _
fi
printf '%s\n' 'Pruebas GTK y AT-SPI ejecutadas. T034 requiere revisar también teclado y lector de pantalla.'
