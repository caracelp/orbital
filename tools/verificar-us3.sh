#!/usr/bin/env bash
# Prueba explícita US3: autocompletado de enlaces al escribir [[ (FR-008, FR-009, SC-002).
set -euo pipefail
cd "$(dirname "$0")/.."
test_config=$(mktemp -d -t orbital-us3-config-XXXXXX)
test_vault=$(mktemp -d -t orbital-us3-vault-XXXXXX)
mkdir -p target/us3-validacion

cleanup() {
    if [ -n "${PID:-}" ]; then
        kill -9 "$PID" 2>/dev/null || true
    fi
    rm -rf -- "$test_config" "$test_vault"
}
trap cleanup EXIT

export XDG_CONFIG_HOME="$test_config"
export ORBITAL_INSTANCIA_PROPIA=1

echo "=== 1. Pruebas unitarias de autocompletado ==="
cargo test --bin orbital filter_candidates -- --nocapture 2>&1 | tee target/us3-validacion/resultado.log
cargo test --bin orbital code_completion_helpers -- --nocapture 2>&1 | tee -a target/us3-validacion/resultado.log

echo "=== 2. Preparación de bóveda de prueba ==="
cargo run -p orbital-vault --example generar-boveda -- "$test_vault" 5

echo "=== 3. Lanzar Orbital e interactuar por AT-SPI ==="
./target/debug/orbital "$test_vault" &
PID=$!
export ORBITAL_PID="$PID"

python3 -I tools/atspi.py esperar frame "Orbital"
python3 -I tools/atspi.py activar "button" "wiki"
sleep 1
python3 -I tools/atspi.py elegir "nota-00001"
sleep 1

# Cambiar a vista Código si no lo está
python3 -I tools/atspi.py activar frame "Orbital" win.view-code || true
sleep 0.5

# AT-SPI no da foco en GTK4 y el autocompletado solo se abre con el editor enfocado: abrir y
# cerrar la búsqueda en la nota devuelve el foco a la vista Código (como en la 005 US5).
python3 -I tools/atspi.py activar frame "Orbital" win.find
python3 -I tools/atspi.py activar "button" "Cerrar búsqueda"
sleep 0.5

# Escribir texto con trigger [[ al final
python3 -I tools/atspi.py escribir "text" "Texto de la nota" "# Título\n\nTexto con enlace a [["
sleep 1

# Verificar que el popover o la lista de sugerencias aparece
echo "Esperando popover de autocompletado..."
python3 -I tools/atspi.py esperar "*" "Sugerencias de enlaces"

# Activar la sugerencia seleccionada
python3 -I tools/atspi.py activar "*" "Sugerencia: "
sleep 1.5

# Verificar que el texto de la nota ahora contiene el wikilink cerrado ]]
TEXTO=$(python3 -I tools/atspi.py texto "text" "Texto de la nota")
echo "Texto resultante en el editor:"
echo "$TEXTO"

if echo "$TEXTO" | grep -q "\[\[nota-.*\]\]"; then
    echo "SC-002 verificado: Enlace wikilink insertado y cerrado automáticamente con ]]."
else
    echo "ERROR: No se encontró el enlace wikilink insertado"
    exit 1
fi

echo "=== Validación US3 completa: ÉXITO ==="
