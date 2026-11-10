#!/usr/bin/env bash
# Prueba explícita US1: navegación de enlaces y encabezados con GTK y AT-SPI.
set -euo pipefail
cd "$(dirname "$0")/.."
test_config=$(mktemp -d -t orbital-us1-config-XXXXXX)
test_vault=$(mktemp -d -t orbital-us1-vault-XXXXXX)
mkdir -p target/us1-validacion

cleanup() {
    if [ -n "${PID:-}" ]; then
        kill -9 "$PID" 2>/dev/null || true
    fi
    rm -rf -- "$test_config" "$test_vault"
}
trap cleanup EXIT

export XDG_CONFIG_HOME="$test_config"
export ORBITAL_INSTANCIA_PROPIA=1

echo "=== 1. Prueba de renderizado, enlaces, anclas y teclado con GTK ==="
cargo test --bin orbital us1 -- --nocapture --test-threads=1 2>&1 | tee target/us1-validacion/resultado.log
cargo test --bin orbital us1_reader -- --ignored --nocapture --test-threads=1 2>&1 | tee -a target/us1-validacion/resultado.log

echo "=== 2. Prueba interactiva por AT-SPI ==="
cargo run -p orbital-vault --example generar-boveda -- "$test_vault" 3

# Agregar enlaces en nota-00001
cat << "EOF" >> "$test_vault/wiki/nota-00001.md"

Enlace a segunda nota: [[nota-00002]]
Enlace a encabezado: [[nota-00002#Seccion]]
Enlace externo: [GNOME](https://gnome.org)
EOF

./target/debug/orbital "$test_vault" &
PID=$!
export ORBITAL_PID="$PID"

python3 -I tools/atspi.py esperar frame "Orbital"
python3 -I tools/atspi.py activar "button" "wiki"
sleep 1
python3 -I tools/atspi.py elegir "nota-00001"
sleep 1
python3 -I tools/atspi.py activar "radio button" "Lectura"
sleep 1
python3 -I tools/atspi.py esperar "text" "Nota formateada"

echo "=== Validación US1 completa: ÉXITO ==="
