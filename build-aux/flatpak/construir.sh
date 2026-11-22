#!/usr/bin/env bash
# Arma el Flatpak de Orbital con las herramientas básicas de `flatpak` (sin flatpak-builder) y deja
# un paquete instalable. Mismos pasos y permisos que el manifiesto io.github.caracelp.Orbital.json.
#
# Requiere (instalación de usuario, sin sudo):
#   flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
#   flatpak install --user flathub org.gnome.Sdk//51 org.gnome.Platform//51 \
#       org.freedesktop.Sdk.Extension.rust-stable//26.08
#
# Uso: build-aux/flatpak/construir.sh [carpeta de trabajo]   (por defecto, build-aux/flatpak/build)
# Resultado: <carpeta>/orbital.flatpak  →  flatpak install --user orbital.flatpak
set -euo pipefail
raiz="$(cd "$(dirname "$0")/../.." && pwd)"
trabajo="${1:-$raiz/build-aux/flatpak/build}"
mkdir -p "$trabajo"
trabajo="$(cd "$trabajo" && pwd)"
id=io.github.caracelp.Orbital

rm -rf "$trabajo/app" "$trabajo/repo" "$trabajo/src"
# Sin lo que no es parte del repo (lo ignorado en .gitignore, como target/ o el material de
# desarrollo local).
rsync -a --exclude .git --exclude build-aux/flatpak/build --filter=':- .gitignore' \
    "$raiz/" "$trabajo/src/"
flatpak build-init --sdk-extension=org.freedesktop.Sdk.Extension.rust-stable \
    "$trabajo/app" "$id" org.gnome.Sdk//51 org.gnome.Platform//51

# La compilación descarga los crates (con red); el resultado no la necesita salvo para las tarjetas
# de URL y las páginas asociadas.
flatpak build --share=network --filesystem="$trabajo" \
    --env=CARGO_HOME="$trabajo/cargo" \
    --env=PATH=/app/bin:/usr/bin:/usr/lib/sdk/rust-stable/bin \
    "$trabajo/app" sh -c "
        set -e
        cd '$trabajo/src'
        cargo build --release --locked --bin orbital
        install -Dm755 target/release/orbital /app/bin/orbital
        install -Dm644 data/$id.desktop /app/share/applications/$id.desktop
        install -Dm644 data/$id.metainfo.xml /app/share/metainfo/$id.metainfo.xml
        install -Dm644 data/icons/hicolor/scalable/apps/$id.svg /app/share/icons/hicolor/scalable/apps/$id.svg
        install -Dm644 data/icons/hicolor/symbolic/apps/$id-symbolic.svg /app/share/icons/hicolor/symbolic/apps/$id-symbolic.svg
    "

flatpak build-finish "$trabajo/app" --command=orbital \
    --socket=wayland --socket=fallback-x11 --share=ipc --device=dri \
    --share=network --filesystem=home
flatpak build-export "$trabajo/repo" "$trabajo/app"
flatpak build-bundle "$trabajo/repo" "$trabajo/orbital.flatpak" "$id"
echo "Listo: $trabajo/orbital.flatpak"
