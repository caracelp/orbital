#!/usr/bin/env bash
# Instancia, configuración, caché y bóveda propias; sin teclas sintéticas.
set -euo pipefail
cd "$(dirname "$0")/.."
test_root=$(mktemp -d -t orbital-005-us4-XXXXXX)
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
---
type: nota
---
# Principal

```toc
```

## Primera

### Adentro

## Segunda
MD
cat > "$ORBITAL_TEST_VAULT/wiki/filtro.md" <<'MD'
---
type: nota
---
# Topo

```toc
minLevel: 2
maxLevel: 3
```

## Segundo

### Profundo
MD
echo 'md_view: lectura' > "$XDG_CONFIG_HOME/orbital/estado.yaml"
sha256sum "$ORBITAL_TEST_VAULT/wiki/principal.md" "$ORBITAL_TEST_VAULT/wiki/filtro.md" > "$test_root/notas.sha256"
cargo build
# argv[0]=orbital: si un sandbox la lanza con otro nombre de proceso, AT-SPI no la encuentra.
bash -c 'exec -a orbital ./target/debug/orbital "$1"' _ "$ORBITAL_TEST_VAULT" > "$test_root/orbital.log" 2>&1 &
PID=$!
export ORBITAL_PID="$PID"
if ! python3 -I tools/verificar_005_us4.py; then
    cat "$test_root/orbital.log"
    exit 1
fi
sha256sum -c "$test_root/notas.sha256"
printf '%s\n' 'Pruebas GTK y AT-SPI ejecutadas. T021 requiere revisar también teclado y lector de pantalla.'
