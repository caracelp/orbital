#!/usr/bin/env bash
# Enlaza un material de desarrollo local (especificaciones, instrucciones para asistentes y
# herramientas de flujo) que vive en su propio repositorio, clonado en `desarrollo/`.
#
# No es necesario para compilar, probar ni contribuir: si `desarrollo/` no existe, no hace nada.
# Todo lo que crea está en .gitignore y nunca reemplaza un archivo real (solo enlaces simbólicos).
#
# Uso:
#   tools/preparar-desarrollo.sh            # en la carpeta del repo (o en un git worktree)
#   tools/preparar-desarrollo.sh --quitar   # borra los enlaces que creó este script
#
# En un `git worktree`, si el worktree no tiene `desarrollo/`, se enlaza el del checkout principal.
set -euo pipefail
raiz="$(cd "$(dirname "$0")/.." && pwd)"
cd "$raiz"

# Lo que se enlaza en la raíz: nombre en la raíz -> ruta dentro de desarrollo/.
enlaces=(
    "specs:specs"
    ".specify:.specify"
    "AGENTS.md:AGENTS.md"
)
# Dentro de .claude/ (la carpeta queda real: ahí también van los worktrees y lo local).
enlaces_claude=(
    "settings.json:.claude/settings.json"
    "hooks:.claude/hooks"
)

quitar_enlace() {
    [ -L "$1" ] && rm "$1" && echo "quitado: $1"
    return 0
}

if [ "${1:-}" = "--quitar" ]; then
    for par in "${enlaces[@]}"; do quitar_enlace "${par%%:*}"; done
    for par in "${enlaces_claude[@]}"; do quitar_enlace ".claude/${par%%:*}"; done
    for s in .claude/skills/*; do [ -L "$s" ] && quitar_enlace "$s"; done
    if [ -f CLAUDE.local.md ] && grep -qx '@desarrollo/CLAUDE.md' CLAUDE.local.md; then
        rm CLAUDE.local.md && echo "quitado: CLAUDE.local.md"
    fi
    [ -L desarrollo ] && quitar_enlace desarrollo
    exit 0
fi

# En un worktree sin desarrollo/, usar el del checkout principal (si lo tiene).
if [ ! -e desarrollo ] && comun="$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null)"; then
    principal="$(dirname "$comun")"
    if [ "$principal" != "$raiz" ] && [ -d "$principal/desarrollo" ]; then
        ln -s "$principal/desarrollo" desarrollo
        echo "enlazado: desarrollo -> $principal/desarrollo"
    fi
fi

if [ ! -d desarrollo ]; then
    exit 0 # sin material de desarrollo: nada que hacer
fi

enlazar() { # enlazar <enlace> <destino relativo al enlace>
    local enlace="$1" destino="$2"
    if [ -L "$enlace" ]; then
        [ "$(readlink "$enlace")" = "$destino" ] && return 0
        rm "$enlace"
    elif [ -e "$enlace" ]; then
        echo "aviso: «$enlace» existe y no es un enlace; no se toca" >&2
        return 0
    fi
    ln -s "$destino" "$enlace"
    echo "enlazado: $enlace -> $destino"
}

for par in "${enlaces[@]}"; do
    nombre="${par%%:*}" origen="${par#*:}"
    [ -e "desarrollo/$origen" ] && enlazar "$nombre" "desarrollo/$origen"
done

if [ -d desarrollo/.claude ]; then
    mkdir -p .claude/skills
    for par in "${enlaces_claude[@]}"; do
        nombre="${par%%:*}" origen="${par#*:}"
        [ -e "desarrollo/$origen" ] && enlazar ".claude/$nombre" "../desarrollo/$origen"
    done
    # Una entrada por habilidad (los enlaces por habilidad son los que el asistente documenta).
    for s in desarrollo/.claude/skills/*/; do
        [ -d "$s" ] || continue
        h="$(basename "$s")"
        enlazar ".claude/skills/$h" "../../desarrollo/.claude/skills/$h"
    done
fi

# Instrucciones del asistente: CLAUDE.local.md (ignorado) importa las del material de desarrollo.
if [ -f desarrollo/CLAUDE.md ]; then
    if [ ! -e CLAUDE.local.md ]; then
        printf '%s\n' '@desarrollo/CLAUDE.md' >CLAUDE.local.md
        echo "creado: CLAUDE.local.md (importa desarrollo/CLAUDE.md)"
    elif ! grep -qx '@desarrollo/CLAUDE.md' CLAUDE.local.md; then
        echo "aviso: CLAUDE.local.md ya existe; agregale la línea @desarrollo/CLAUDE.md" >&2
    fi
fi
