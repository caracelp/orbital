#!/usr/bin/env bash
# Prueba de validación AT-SPI para US2 (Saltar a una nota por su nombre)
set -euo pipefail
cd "$(dirname "$0")/.."

test_config=$(mktemp -d -t orbital-004-us2-config-XXXXXX)
test_vault=$(mktemp -d -t orbital-004-us2-vault-XXXXXX)

cleanup() {
    if [ -n "${PID:-}" ]; then
        kill -9 "$PID" 2>/dev/null || true
    fi
    rm -rf -- "$test_config" "$test_vault"
}
trap cleanup EXIT

export XDG_CONFIG_HOME="$test_config"
export ORBITAL_INSTANCIA_PROPIA=1

echo "=== Creando bóveda de prueba en $test_vault ==="
mkdir -p "$test_vault/wiki/fisica" "$test_vault/wiki/quimica" "$test_vault/wiki/bio"

# Pre-poblar estado.yaml con una nota reciente (reaccion)
mkdir -p "$test_config/orbital"
cat << EOF > "$test_config/orbital/estado.yaml"
recent_notes: { "$test_vault": ["wiki/quimica/reaccion.md"] }
EOF

cat << "EOF" > "$test_vault/wiki/fisica/entropia.md"
---
type: atomo
title: Entropía y termodinámica
tags: [fisica, energia]
---
# Entropía

La física cuántica estudia la energía cinética y la entropía.
EOF

cat << "EOF" > "$test_vault/wiki/fisica/mecanica.md"
---
type: atomo
title: Mecánica clásica
tags: [fisica]
---
# Mecánica clásica

La física newtoniana describe la energía potencial.
EOF

cat << "EOF" > "$test_vault/wiki/quimica/reaccion.md"
---
type: atomo
title: Reacción química
tags: [quimica]
---
# Reacción química

Proceso con energía de activación.
EOF

cat << "EOF" > "$test_vault/wiki/bio/biologia.md"
---
type: atomo
title: Biología molecular
aliases: [celula, celulas]
tags: [biologia]
---
# Biología molecular y celular

Estudio de la vida celular.
EOF

echo "=== Compilando Orbital en release si hace falta ==="
cargo build --release

echo "=== Lanzando Orbital en segundo plano ==="
./target/release/orbital "$test_vault" > /tmp/orbital_test.log 2>&1 &
PID=$!
export ORBITAL_PID="$PID"
sleep 1.8

echo "=== Ejecutando pruebas AT-SPI de US2 ==="
if ! python3 -I tools/verificar_004_us2.py; then
    echo "=== LOG DE ORBITAL ==="
    cat /tmp/orbital_test.log || true
    exit 1
fi

echo "=== Prueba de US2 completada con ÉXITO ==="
