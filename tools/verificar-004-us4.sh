#!/usr/bin/env bash
# Prueba de validación AT-SPI para US4 (Buscar por lugar y por propiedades)
set -euo pipefail
cd "$(dirname "$0")/.."

test_config=$(mktemp -d -t orbital-004-us4-config-XXXXXX)
test_vault=$(mktemp -d -t orbital-004-us4-vault-XXXXXX)

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
mkdir -p "$test_vault/wiki/fisica" "$test_vault/wiki/quimica" "$test_vault/wiki/biologia"

cat << "EOF" > "$test_vault/wiki/fisica/entropia.md"
---
type: atomo
title: Entropía y termodinámica
tags: [fisica/termodinamica, energia]
---
# Sección Primera
El calor y la energía se transfieren en la misma línea.

# Sección Segunda
Aquí solo se menciona calor.
Y más abajo en otra línea está transferencia.

estado:: activo
EOF

cat << "EOF" > "$test_vault/wiki/quimica/reacciones.html"
<!DOCTYPE html>
<html>
<head><title>Reacciones químicas</title></head>
<body>
<h1>Química orgánica</h1>
<p>Etiqueta: #quimica/organica</p>
<p>tipo:: enlace</p>
</body>
</html>
EOF

cat << "EOF" > "$test_vault/wiki/biologia/celula.md"
---
type: celula
estado: archivado
---
# Biología celular
estado:: activo
La célula tiene biología y núcleo en una sola sección.
EOF

mkdir -p "$test_config/orbital"
cat << EOF > "$test_config/orbital/estado.yaml"
recent_notes: { "$test_vault": ["wiki/fisica/entropia.md"] }
EOF

echo "=== Construyendo Orbital ==="
cargo build

echo "=== Iniciando Orbital con la bóveda de prueba ==="
./target/debug/orbital "$test_vault" > /tmp/orbital_test_us4.log 2>&1 &
PID=$!
export ORBITAL_PID="$PID"

echo "Orbital iniciado con PID $PID"
sleep 2.0

echo "=== Ejecutando script de validación AT-SPI para US4 ==="
if ! python3 -I tools/verificar_004_us4.py; then
    echo "=== LOG DE ORBITAL ==="
    cat /tmp/orbital_test_us4.log || true
    exit 1
fi

echo "=== Validación US4 completada exitosamente ==="
