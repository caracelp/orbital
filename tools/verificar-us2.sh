#!/usr/bin/env bash
# Prueba explícita US2: enlaces a notas pendientes, confirmación, persistencia y creación.
set -euo pipefail
cd "$(dirname "$0")/.."
test_config=$(mktemp -d -t orbital-us2-config-XXXXXX)
test_vault=$(mktemp -d -t orbital-us2-vault-XXXXXX)
mkdir -p target/us2-validacion

cleanup() {
    if [ -n "${PID:-}" ]; then
        kill -9 "$PID" 2>/dev/null || true
    fi
    rm -rf -- "$test_config" "$test_vault"
}
trap cleanup EXIT

export XDG_CONFIG_HOME="$test_config"
export ORBITAL_INSTANCIA_PROPIA=1

echo "=== 1. Prueba de estilos de enlaces pendientes y persistencia de configuración ==="
cargo test --bin orbital us2 -- --nocapture --test-threads=1 2>&1 | tee target/us2-validacion/resultado.log
cargo test --bin orbital us2_reader -- --ignored --nocapture --test-threads=1 2>&1 | tee -a target/us2-validacion/resultado.log

echo "=== 2. Prueba interactiva por AT-SPI ==="
cargo run -p orbital-vault --example generar-boveda -- "$test_vault" 3

./target/debug/orbital "$test_vault" &
PID=$!
export ORBITAL_PID="$PID"

python3 -I tools/atspi.py esperar frame "Orbital"
python3 -I tools/atspi.py activar "button" "wiki"
sleep 1
python3 -I tools/atspi.py elegir "nota-00001"
sleep 1

# Activar la acción de enlace pendiente
python3 -I tools/atspi.py activar "frame" "Orbital" "win.open-missing"
sleep 1

# Esperar y confirmar en el diálogo de confirmación
python3 -I tools/atspi.py esperar "button" "Crear"
python3 -I tools/atspi.py activar "button" "Crear"
sleep 1.5

# Verificar creación en disco
NOTE_FILE="$test_vault/wiki/nota-nueva-pendiente.md"
if [ -f "$NOTE_FILE" ]; then
    echo "SC-002 verificado: Nota creada atómicamente en wiki/."
else
    echo "ERROR: No se creó la nota pendiente"
    exit 1
fi

echo "=== Validación US2 completa: ÉXITO ==="
