#!/usr/bin/env bash
# Instancia, configuración, caché y bóveda propias; sin teclas ni mouse sintéticos (014).
set -euo pipefail
cd "$(dirname "$0")/.."
test_root=$(mktemp -d -t orbital-014-us1-XXXXXX)
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
cat > "$ORBITAL_TEST_VAULT/wiki/a.md" <<'EOF'
---
type: atomo
title: Órbitas
description: Demo de OnStage.
---
# Uno

Texto de la primera diapositiva con [[nota-b]].

## Dos

Texto de la segunda diapositiva.
EOF
cat > "$ORBITAL_TEST_VAULT/wiki/nota-html.html" <<'EOF'
<!doctype html><html><head><script type="application/yaml">
type: sintesis
title: HTML
</script></head><body><p>Síntesis</p></body></html>
EOF
sha256sum "$ORBITAL_TEST_VAULT"/wiki/*.md > "$test_root/notas.sha256"
cargo test --bin orbital onstage::tests -- --nocapture --include-ignored
cargo build
bash -c 'exec -a orbital ./target/debug/orbital "$1"' _ "$ORBITAL_TEST_VAULT" > "$test_root/orbital.log" 2>&1 &
PID=$!
export ORBITAL_PID="$PID"
if ! python3 -I tools/verificar_014_us1.py; then
    cat "$test_root/orbital.log"
    exit 1
fi
sha256sum -c "$test_root/notas.sha256"
printf '%s\n' 'Comprobaciones 014 ejecutadas. Pendiente para la usuaria: teclado, mouse y lector de pantalla.'
