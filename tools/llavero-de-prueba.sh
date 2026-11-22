#!/usr/bin/env bash
# Corre un comando con un llavero (Secret Service) de prueba: gnome-keyring en una sesión de D-Bus
# propia, con sus datos en una carpeta temporal. Nunca toca el llavero de la usuaria.
#
# Uso: tools/llavero-de-prueba.sh cargo test -p orbital-drive llavero_del_sistema -- --ignored
set -euo pipefail
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export XDG_DATA_HOME="$tmp/datos" XDG_RUNTIME_DIR="$tmp/run" ORBITAL_LLAVERO_DE_PRUEBA=1
mkdir -p "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
dbus-run-session -- bash -c '
    printf "clave-de-prueba" | gnome-keyring-daemon --unlock --components=secrets >/dev/null
    "$@"' _ "$@"
