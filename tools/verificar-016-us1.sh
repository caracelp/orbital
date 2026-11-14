#!/usr/bin/env bash
# Instancia, configuración, caché y bóveda propias; sin teclas ni mouse sintéticos (016).
set -euo pipefail
cd "$(dirname "$0")/.."
test_root=$(mktemp -d -t orbital-016-us1-XXXXXX)
KEEP="${1:-}"
PID=''
cleanup() {
    if [ -n "$PID" ]; then kill "$PID" 2>/dev/null || true; wait "$PID" 2>/dev/null || true; fi
    [[ "$KEEP" == "--keep" ]] || rm -rf -- "$test_root"
}
trap cleanup EXIT
export XDG_CONFIG_HOME="$test_root/config"
export XDG_CACHE_HOME="$test_root/cache"
export ORBITAL_INSTANCIA_PROPIA=1
export ORBITAL_TEST_VAULT="$test_root/vault"
mkdir -p "$ORBITAL_TEST_VAULT/raw" "$ORBITAL_TEST_VAULT/wiki" "$XDG_CONFIG_HOME/orbital" "$XDG_CACHE_HOME"
cat > "$ORBITAL_TEST_VAULT/wiki/index.md" <<'EOF'
# atomo

# sintesis
EOF
cat > "$ORBITAL_TEST_VAULT/wiki/log.md" <<'EOF'
# Registro de la wiki
EOF
cat > "$ORBITAL_TEST_VAULT/wiki/lectura.md" <<'EOF'
---
type: atomo
title: Lectura del informe
description: Nota con un resaltado del informe.
tags: [prueba]
created: 2026-11-13T19:00:00-03:00
generated: { by: human:prueba, at: 2026-11-13T19:00:00-03:00 }
---
# Lectura del informe

La cita está en [Informe](/raw/informe.pdf#page=1&sel=0,6).
EOF
python3 - "$ORBITAL_TEST_VAULT/raw/informe.pdf" <<'EOF'
import sys

def pdf_minimo(texto):
    stream = f"BT /F1 24 Tf 72 720 Td ({texto}) Tj ET"
    objetos = [
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            "/Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>"),
        (4, f"<< /Length {len(stream)} >>\nstream\n{stream}\nendstream"),
        (5, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
    ]
    out = "%PDF-1.4\n"
    offsets = {}
    for n, cuerpo in objetos:
        offsets[n] = len(out)
        out += f"{n} 0 obj\n{cuerpo}\nendobj\n"
    xref = len(out)
    out += "xref\n0 6\n0000000000 65535 f \n"
    for n in range(1, 6):
        out += f"{offsets[n]:010d} 00000 n \n"
    out += f"trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    return out.encode("latin-1")

open(sys.argv[1], "wb").write(pdf_minimo("Informe anual del proyecto"))
EOF
sha256sum "$ORBITAL_TEST_VAULT"/raw/informe.pdf > "$test_root/fuente.sha256"
cargo test --bin orbital visor_pdf -- --nocapture
cargo build
bash -c 'exec -a orbital ./target/debug/orbital "$1"' _ "$ORBITAL_TEST_VAULT" > "$test_root/orbital.log" 2>&1 &
PID=$!
export ORBITAL_PID="$PID"
if ! python3 -I tools/verificar_016_us1.py; then
    cat "$test_root/orbital.log"
    exit 1
fi
sha256sum -c "$test_root/fuente.sha256"
printf '%s\n' 'Comprobaciones 016 ejecutadas. Pendiente para la usuaria: teclado, mouse y lector de pantalla.'
