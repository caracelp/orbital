#!/usr/bin/env python3
"""Comprobaciones AT-SPI de la 028 US4 (enfocar y miga de pan); solo el PID propio del lanzador."""
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
assert vault.parent.name.startswith("orbital-028-us4-"), "Solo acepta la bóveda temporal del lanzador"


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


wait_for(lambda: helpers["get_app"](), "No se encontró la instancia propia de Orbital")
frame = wait_for(
    lambda: next((n for n in helpers["find_all"]("frame", "Orbital") if visible(n)), None),
    "No hay ventana",
)

node = wait_for(lambda: exact("wiki"), "No aparece la carpeta wiki")
assert helpers["do_action"](node)
time.sleep(0.5)
elegir("outline.md")
wait_for(
    lambda: next((n for n in helpers["find_all"]("label", "wiki/outline.md") if visible(n)), None),
    "No se abrió outline.md",
)

print("AVISO: AT-SPI no posiciona el cursor en GtkSourceView; se prueba win.enfocar sobre la nota abierta")
assert helpers["do_action"](frame, "win.enfocar"), "No se pudo accionar win.enfocar"

migas = wait_for(
    lambda: exact("Miga de pan del enfoque"),
    "No apareció la miga de pan del enfoque",
)
salir = wait_for(
    lambda: exact("Salir del enfoque", "push button"),
    "No apareció el botón Salir del enfoque",
)
btn_nota = wait_for(
    lambda: exact("outline", "push button"),
    "No apareció el botón del nivel de la nota en la miga",
)
assert migas is not None and btn_nota is not None
assert helpers["do_action"](salir), "No se pudo accionar Salir del enfoque"
wait_for(
    lambda: exact("Salir del enfoque", "push button") is None,
    "La miga de pan no se ocultó al salir del enfoque",
)
print("OK: win.enfocar muestra la miga de pan y Salir del enfoque la oculta")
