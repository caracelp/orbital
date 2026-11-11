#!/usr/bin/env bash
# Prueba de validación AT-SPI para US1 (Búsqueda en toda la bóveda)
set -euo pipefail
cd "$(dirname "$0")/.."

test_config=$(mktemp -d -t orbital-004-us1-config-XXXXXX)
test_vault=$(mktemp -d -t orbital-004-us1-vault-XXXXXX)

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
mkdir -p "$test_vault/wiki/fisica" "$test_vault/wiki/quimica" "$test_vault/wiki/ciencia" "$test_vault/raw"

cat << "EOF" > "$test_vault/wiki/fisica/entropia.md"
---
type: atomo
title: Entropía y termodinámica
tags: [fisica, energia]
---
# Entropía

La física cuántica estudia la energía cinética y la entropía.

Aquí tenemos la conservacion de la masa en sistemas aislados.

Otra mención de energía térmica.
EOF

cat << "EOF" > "$test_vault/wiki/fisica/mecanica.md"
---
type: atomo
title: Mecánica clásica
tags: [fisica]
---
# Mecánica clásica

La física newtoniana describe la energía potencial.

Línea clásica sin términos modernos.
EOF

cat << "EOF" > "$test_vault/wiki/quimica/reaccion.md"
---
type: atomo
title: Reacción química
tags: [quimica]
---
# Reacción química

Proceso con energía de activación pero sin la otra palabra.
EOF

cat << "EOF" > "$test_vault/wiki/ciencia/termodinamica.html"
<!doctype html>
<html lang="es">
<head>
  <meta charset="utf-8">
  <title>Termodinámica química</title>
</head>
<body>
<h1>Termodinámica</h1>
<p>El estudio de la física térmica y la energía en sistemas cerrados.</p>
<div class="span-etiqueta-ignorar">Texto del cuerpo html</div>
</body>
</html>
EOF

cat << "EOF" > "$test_vault/raw/apuntes-fisica.txt"
Apuntes en bruto sobre física y energía en el laboratorio.
EOF

echo "=== Lanzando Orbital en segundo plano ==="
./target/release/orbital "$test_vault" &
PID=$!
export ORBITAL_PID="$PID"
sleep 1.5

echo "=== Ejecutando pruebas AT-SPI ==="
python3 -I tools/verificar_004_us1.py

echo "=== Prueba de US1 completada con ÉXITO ==="
