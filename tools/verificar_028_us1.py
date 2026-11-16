#!/usr/bin/env python3
"""Comprobaciones AT-SPI de la 028 US1/US3 (plegado); solo el PID propio del lanzador.

Fase «plegar»: abre outline.md, acciona `win.plegar-todo` (la acción de la ventana que usan el menú
y la paleta) y espera `.orbital/plegado.yaml` con las claves de los cuatro elementos plegables.
Fase «reabrir» (otra instancia): abrir la nota no reescribe el archivo; `win.desplegar-todo` saca
la nota del archivo.

AT-SPI no puede ver qué está oculto: el texto accesible de una vista de texto incluye el texto
invisible (research R14). Eso lo comprueban las pruebas GTK en proceso.
"""
import os
import runpy
import subprocess
import sys
import time
from pathlib import Path

helpers = runpy.run_path("tools/verificar_004_us4.py")
Atspi = helpers["Atspi"]
assert os.environ.get("ORBITAL_PID"), "Falta el PID de la instancia propia"
vault = Path(os.environ["ORBITAL_TEST_VAULT"]).resolve()
assert vault.parent.name.startswith("orbital-028-us1-"), "Solo acepta la bóveda temporal del lanzador"
PLEGADO = vault / ".orbital/plegado.yaml"
fase = sys.argv[1] if len(sys.argv) > 1 else "plegar"
ESPERADO = """format_version: 1
notas:
  - ruta: wiki/outline.md
    plegados:
      - encabezado: Nota
      - encabezado: Historia
      - item: Frutas
      - encabezado: Ejemplos
"""


def wait_for(check, message, seconds=15):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.15)
    raise AssertionError(message)


def visible(node):
    try:
        return node.get_state_set().contains(Atspi.StateType.SHOWING)
    except Exception:
        return False


def exact(name, role="*"):
    return next(
        (n for n in helpers["find_all"](role, name) if n.get_name() == name and visible(n)),
        None,
    )


def elegir(fila):
    salida = subprocess.run(
        [sys.executable, "-I", "tools/atspi.py", "elegir", fila], capture_output=True, text=True
    )
    assert salida.returncode == 0, f"No se pudo elegir {fila!r}: {salida.stdout}{salida.stderr}"
    time.sleep(1.0)


def abrir_outline():
    node = wait_for(lambda: exact("wiki"), "No aparece la carpeta wiki")
    assert helpers["do_action"](node)
    time.sleep(0.5)
    elegir("outline.md")
    wait_for(
        lambda: next((n for n in helpers["find_all"]("label", "wiki/outline.md") if visible(n)), None),
        "No se abrió outline.md",
    )


def accion(frame, nombre):
    assert helpers["do_action"](frame, nombre), f"No se pudo accionar {nombre}"


wait_for(lambda: helpers["get_app"](), "No se encontró la instancia propia de Orbital")
frame = wait_for(
    lambda: next((n for n in helpers["find_all"]("frame", "Orbital") if visible(n)), None),
    "No hay ventana",
)

if fase == "plegar":
    abrir_outline()
    time.sleep(1.0)
    assert not (vault / ".orbital").exists(), "Abrir una nota creó .orbital/"
    print("OK: abrir la nota no crea .orbital/")
    accion(frame, "win.plegar-todo")
    wait_for(
        lambda: PLEGADO.exists() and PLEGADO.read_text(encoding="utf-8") == ESPERADO,
        f"plegado.yaml no quedó como se esperaba: "
        f"{PLEGADO.read_text(encoding='utf-8') if PLEGADO.exists() else '(no existe)'!r}",
    )
    print("OK: «Plegar todo» escribe .orbital/plegado.yaml con los cuatro elementos")
elif fase == "reabrir":
    antes = PLEGADO.stat().st_mtime_ns
    abrir_outline()
    time.sleep(2.0)
    assert PLEGADO.read_text(encoding="utf-8") == ESPERADO, "Al reabrir cambió plegado.yaml"
    assert PLEGADO.stat().st_mtime_ns == antes, "Abrir la nota plegada reescribió plegado.yaml"
    print("OK: al reabrir, plegado.yaml sigue igual y no se reescribe")
    accion(frame, "win.desplegar-todo")
    wait_for(
        lambda: PLEGADO.read_text(encoding="utf-8") == "format_version: 1\nnotas: []\n",
        f"«Desplegar todo» no sacó la nota: {PLEGADO.read_text(encoding='utf-8')!r}",
    )
    print("OK: «Desplegar todo» saca la nota del archivo")
else:
    sys.exit(f"Fase desconocida: {fase}")
