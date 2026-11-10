#!/usr/bin/env bash
# Prueba explícita: instancia GTK independiente y bóveda temporal, sin tocar Orbital abierto.
set -euo pipefail
cd "$(dirname "$0")/.."
test_config=$(mktemp -d -t orbital-us4-config-XXXXXX)
trap 'rm -rf -- "$test_config"' EXIT
export XDG_CONFIG_HOME="$test_config"
export ORBITAL_INSTANCIA_PROPIA=1
mkdir -p target/us4-validacion
cargo test --offline --bin orbital us4_webkit -- --ignored --nocapture --test-threads=1 2>&1 | tee target/us4-validacion/resultado.log
