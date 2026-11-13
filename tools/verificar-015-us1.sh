#!/usr/bin/env bash
# Instancia, configuración, caché y bóveda propias; sin teclas ni mouse sintéticos (015).
set -euo pipefail
cd "$(dirname "$0")/.."
test_root=$(mktemp -d -t orbital-015-us1-XXXXXX)
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
cat > "$ORBITAL_TEST_VAULT/raw/articulo.html" <<'EOF'
<!doctype html><html><head><title>Artículo importado</title></head><body>
<h1>Artículo importado</h1>
<p>Texto del artículo con <strong>énfasis</strong> y una <a href="https://ejemplo.com">fuente</a>.</p>
<ul><li>primero</li><li>segundo</li></ul>
</body></html>
EOF
printf 'PNG' > "$ORBITAL_TEST_VAULT/raw/imagen.png"
python3 - <<'EOF'
import os, zipfile
docx = os.path.join(os.environ["ORBITAL_TEST_VAULT"], "raw/informe.docx")
document = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">'
    '<w:body>'
    '<w:p><w:r><w:t>Informe anual</w:t></w:r></w:p>'
    '<w:p><w:r><w:t>Texto del informe.</w:t></w:r></w:p>'
    '</w:body></w:document>')
with zipfile.ZipFile(docx, "w") as z:
    z.writestr("[Content_Types].xml", '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>')
    z.writestr("word/document.xml", document)
EOF
sha256sum "$ORBITAL_TEST_VAULT"/raw/articulo.html "$ORBITAL_TEST_VAULT"/raw/informe.docx > "$test_root/fuente.sha256"
cargo test --bin orbital importar -- --nocapture
cargo build
bash -c 'exec -a orbital ./target/debug/orbital "$1"' _ "$ORBITAL_TEST_VAULT" > "$test_root/orbital.log" 2>&1 &
PID=$!
export ORBITAL_PID="$PID"
if ! python3 -I tools/verificar_015_us1.py; then
    cat "$test_root/orbital.log"
    exit 1
fi
sha256sum -c "$test_root/fuente.sha256"
printf '%s\n' 'Comprobaciones 015 ejecutadas. Pendiente para la usuaria: teclado, mouse y lector de pantalla.'
