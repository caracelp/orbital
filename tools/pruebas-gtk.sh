#!/usr/bin/env bash
# Corre las pruebas GTK en proceso (las `#[ignore]` del binario), cada una en su propio proceso:
# GTK se inicializa una sola vez por proceso y el arnés de Rust corre cada prueba en un hilo nuevo.
#
# Quedan afuera las mediciones y capturas (no son pruebas de comportamiento) y las que manejan la
# app por AT-SPI (necesitan el bus de accesibilidad de una sesión real: van por tools/verificar-*).
#
# Uso: tools/pruebas-gtk.sh [--release]
set -uo pipefail
cd "$(dirname "$0")/.."

perfil=()
[ "${1:-}" = "--release" ] && perfil=(--release)

# Nombres (o partes) de pruebas que no corren acá, con el motivo.
excluir=(
    "sc_cuadro_5000"          # medición del grafo
    "sc3_cambio_pestana"      # medición de pestañas
    "sc1_cursor_latency"      # medición de multi-cursor
    "command_filter_latency"  # medición de la paleta
    "captura_modos_panel"     # captura de desarrollo
    "us4_webkit"              # maneja la barra de formato por AT-SPI (tools/verificar-us4.sh)
)

cargo build -q --tests --bin orbital "${perfil[@]}" || exit 1
mapfile -t pruebas < <(cargo test -q --bin orbital "${perfil[@]}" -- --ignored --list 2>/dev/null |
    sed -n 's/: test$//p')

pasan=0
fallan=()
for prueba in "${pruebas[@]}"; do
    saltar=0
    for e in "${excluir[@]}"; do
        [[ "$prueba" == *"$e"* ]] && saltar=1
    done
    [ $saltar = 1 ] && continue
    salida=$(timeout 300 cargo test -q --bin orbital "${perfil[@]}" "$prueba" -- --ignored --exact 2>&1)
    if grep -q 'test result: ok. 1 passed' <<<"$salida"; then
        pasan=$((pasan + 1))
    else
        fallan+=("$prueba")
        echo "=== FALLA $prueba"
        tail -25 <<<"$salida"
    fi
done

echo "Pruebas GTK: $pasan pasan, ${#fallan[@]} fallan"
[ ${#fallan[@]} -eq 0 ]
