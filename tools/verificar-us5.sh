#!/usr/bin/env bash
# Prueba explícita US5: instancia GTK independiente y bóveda temporal, sin tocar Orbital abierto.
set -euo pipefail
cd "$(dirname "$0")/.."
test_config=$(mktemp -d -t orbital-us5-config-XXXXXX)
test_vault=$(mktemp -d -t orbital-us5-vault-XXXXXX)
trap 'rm -rf -- "$test_config" "$test_vault"' EXIT
export XDG_CONFIG_HOME="$test_config"
export ORBITAL_INSTANCIA_PROPIA=1
mkdir -p target/us5-validacion

echo "=== 1. Prueba de escenarios y respaldos con GTK ==="
cargo test --offline --bin orbital us5_conflict -- --ignored --nocapture --test-threads=1 2>&1 | tee target/us5-validacion/resultado.log

echo "=== 2. Prueba interactiva por AT-SPI ==="
cargo run -p orbital-vault --example generar-boveda -- "$test_vault" 3

./target/debug/orbital "$test_vault" &
PID=$!
cleanup_app() {
    kill -9 "$PID" 2>/dev/null || true
    rm -rf -- "$test_config" "$test_vault"
}
trap cleanup_app EXIT

export ORBITAL_PID="$PID"
python3 -I tools/atspi.py esperar frame "Orbital"
python3 -I tools/atspi.py activar "button" "wiki"
sleep 1
python3 -I tools/atspi.py elegir "nota-00001"
sleep 1
python3 -I tools/atspi.py activar "radio button" "Código"
sleep 1
python3 -I tools/atspi.py escribir "text" "Texto de la nota" "Cambio verificado por AT-SPI\n"
sleep 0.5

NOTE_FILE="$test_vault/wiki/nota-00001.md"
echo "Versión externa en disco" > "$NOTE_FILE"

# Esperar el diálogo de conflicto y activar Comparar
python3 -I tools/atspi.py esperar "button" "Comparar"
python3 -I tools/atspi.py activar "button" "Comparar"
sleep 1

# Ventana de comparación lado a lado
python3 -I tools/atspi.py esperar "frame" "Comparar versiones"
python3 -I tools/atspi.py activar "button" "Conservar la mía"
sleep 1

CONTENT=$(cat "$NOTE_FILE")
if [[ "$CONTENT" == *"Cambio verificado por AT-SPI"* ]]; then
    echo "SC-005 verificado: Versión conservada en disco."
else
    echo "ERROR: Falló conservación"
    exit 1
fi

BACKUPS=$(find "$test_vault/.orbital/respaldo" -type f | wc -l)
if [ "$BACKUPS" -ge 1 ]; then
    echo "SC-005 verificado: Versión descartada respaldada ($BACKUPS respaldos)."
else
    echo "ERROR: Falta respaldo"
    exit 1
fi

echo "=== Validación US5 completa: ÉXITO ==="
