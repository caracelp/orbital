#!/usr/bin/env bash
# Prueba de validación AT-SPI para US3 (Buscar y reemplazar en la nota abierta)
set -euo pipefail
cd "$(dirname "$0")/.."

test_config=$(mktemp -d -t orbital-004-us3-config-XXXXXX)
test_vault=$(mktemp -d -t orbital-004-us3-vault-XXXXXX)

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
mkdir -p "$test_vault/wiki/fisica" "$test_vault/wiki/html"

cat << "EOF" > "$test_vault/wiki/fisica/termodinamica.md"
---
type: atomo
title: Termodinámica clásica
tags: [fisica, energia]
---
# Termodinámica

El calor es una forma de transferencia térmica.
Cuando aumenta el calor, sube la temperatura.
La entropía mide cómo se disipa el calor en el sistema.
EOF

cat << "EOF" > "$test_vault/wiki/html/pagina.html"
<!DOCTYPE html>
<html>
<head><title>Página de calor</title></head>
<body>
<h1>Prueba de calor en HTML</h1>
<p>El calor fluye espontáneamente del cuerpo caliente al frío.</p>
</body>
</html>
EOF

# Pre-poblar estado para que termodinamica.md sea reciente
mkdir -p "$test_config/orbital"
cat << EOF > "$test_config/orbital/estado.yaml"
recent_notes: { "$test_vault": ["wiki/fisica/termodinamica.md", "wiki/html/pagina.html"] }
md_view: "codigo"
EOF

echo "=== Construyendo Orbital ==="
cargo build

echo "=== Iniciando Orbital con la bóveda de prueba ==="
./target/debug/orbital "$test_vault" > /tmp/orbital_test_us3.log 2>&1 &
PID=$!
export ORBITAL_PID="$PID"

echo "Orbital iniciado con PID $PID"
sleep 2.0

echo "=== Ejecutando script de validación AT-SPI para US3 ==="
if ! python3 -I tools/verificar_004_us3.py; then
    echo "=== LOG DE ORBITAL ==="
    cat /tmp/orbital_test_us3.log || true
    exit 1
fi

echo "=== Validación US3 completada exitosamente ==="
