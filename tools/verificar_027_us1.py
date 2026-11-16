#!/usr/bin/env python3
"""Comprobaciones AT-SPI de la 027 US1 (Favoritos); solo el PID propio del lanzador.

Fase «agregar»: abre a.md, menú «Más opciones de la nota» → «Agregar a favoritos», espera la fila
«Abrir a.md» en Favoritos, comprueba `.orbital/listas.yaml` y que el menú diga «Quitar de favoritos».
Fase «reabrir» (otra instancia): la fila sigue; «Quitar de favoritos» la saca del archivo.

Si el menú no se puede abrir por AT-SPI (los menús necesitan la ventana activa), lo dice y usa la
acción `win.alternar-favorito` de la ventana, que es la misma que llama el botón del menú.
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
assert vault.parent.name.startswith("orbital-027-us1-"), "Solo acepta la bóveda temporal del lanzador"
LISTAS = vault / ".orbital/listas.yaml"
fase = sys.argv[1] if len(sys.argv) > 1 else "agregar"


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


def fila_favorita(nombre):
    """La fila `nombre` dentro de la lista «Notas favoritas» (Recientes usa el mismo nombre)."""
    lista = next(
        (n for n in helpers["find_all"]("*", "Notas favoritas") if n.get_name() == "Notas favoritas"),
        None,
    )
    if lista is None:
        return None
    return next(
        (n for n, _ in helpers["walk"](lista) if n.get_name() == nombre and visible(n)), None
    )


def elegir(fila):
    salida = subprocess.run(
        [sys.executable, "-I", "tools/atspi.py", "elegir", fila], capture_output=True, text=True
    )
    assert salida.returncode == 0, f"No se pudo elegir {fila!r}: {salida.stdout}{salida.stderr}"
    time.sleep(1.0)


def opcion_del_menu(nombre):
    """Abre «Más opciones de la nota» y acciona `nombre`. Devuelve si se pudo por el menú."""
    boton = wait_for(lambda: exact("Más opciones de la nota"), "No está «Más opciones de la nota»")
    helpers["do_action"](boton)
    try:
        opcion = wait_for(lambda: exact(nombre), f"El menú no muestra «{nombre}»", seconds=4)
    except AssertionError:
        return False
    assert helpers["do_action"](opcion), f"No se pudo accionar «{nombre}»"
    return True


def menu_dice(nombre):
    """El menú abierto muestra `nombre` (None si el menú no se abre por AT-SPI)."""
    boton = exact("Más opciones de la nota")
    helpers["do_action"](boton)
    try:
        wait_for(lambda: exact(nombre), "", seconds=3)
        ok = True
    except AssertionError:
        ok = None if not any(visible(n) for n in helpers["find_all"]("*", "Renombrar o mover")) else False
    helpers["do_action"](boton)  # cerrarlo
    time.sleep(0.3)
    return ok


def alternar(nombre, frame):
    if opcion_del_menu(nombre):
        print(f"OK: «{nombre}» desde el menú de la nota")
        return
    print(
        f"AVISO: el menú no se abrió por AT-SPI (ventana no activa); «{nombre}» se accionó con "
        "win.alternar-favorito, la acción del mismo botón. Queda para la usuaria probar el menú."
    )
    assert helpers["do_action"](frame, "win.alternar-favorito"), "No se pudo accionar la acción"


wait_for(lambda: helpers["get_app"](), "No se encontró la instancia propia de Orbital")
frame = wait_for(
    lambda: next((n for n in helpers["find_all"]("frame", "Orbital") if visible(n)), None),
    "No hay ventana",
)
seccion = wait_for(lambda: exact("Favoritos"), "No está la sección Favoritos")
node = wait_for(lambda: exact("wiki"), "No aparece la carpeta wiki")

if fase == "agregar":
    assert not (vault / ".orbital").exists(), "Abrir la bóveda creó .orbital/"
    wait_for(
        lambda: exact("Agregá notas desde el menú ⋯ de la nota"),
        "Favoritos vacío no muestra el texto de ayuda",
    )
    print("OK: Favoritos vacío con su texto de ayuda y sin .orbital/")
    assert helpers["do_action"](node)
    time.sleep(0.5)
    elegir("a.md")
    alternar("Agregar a favoritos", frame)
    wait_for(lambda: fila_favorita("Abrir a.md"), "No aparece la fila «Abrir a.md» en Favoritos")
    print("OK: «Abrir a.md» en Favoritos")
    wait_for(
        lambda: LISTAS.exists() and "  - wiki/a.md\n" in LISTAS.read_text(encoding="utf-8"),
        "listas.yaml no tiene wiki/a.md",
    )
    print("OK: .orbital/listas.yaml con wiki/a.md")
    dice = menu_dice("Quitar de favoritos")
    if dice is None:
        print("AVISO: no se pudo abrir el menú para leer «Quitar de favoritos» (queda para la usuaria)")
    else:
        assert dice, "El menú no dice «Quitar de favoritos»"
        print("OK: el menú dice «Quitar de favoritos»")
elif fase == "reabrir":
    fila = wait_for(
        lambda: fila_favorita("Abrir a.md"), "Al reabrir no está «Abrir a.md» en Favoritos"
    )
    print("OK: al reabrir, a.md sigue en Favoritos")
    # La fila abre la nota (las filas de GtkListBox exponen «activate»).
    if helpers["do_action"](fila):
        wait_for(
            lambda: next(
                (n for n in helpers["find_all"]("label", "wiki/a.md") if visible(n)), None
            ),
            "La fila de Favoritos no abrió a.md",
        )
        print("OK: la fila de Favoritos abre a.md")
    else:
        print("AVISO: la fila no expone acciones por AT-SPI; abrirla queda en la prueba GTK en proceso")
        assert helpers["do_action"](node)
        time.sleep(0.5)
        elegir("a.md")
    alternar("Quitar de favoritos", frame)
    wait_for(
        lambda: "favoritos: []" in LISTAS.read_text(encoding="utf-8"),
        f"listas.yaml todavía tiene a.md: {LISTAS.read_text(encoding='utf-8')!r}",
    )
    wait_for(lambda: fila_favorita("Abrir a.md") is None, "La fila no se fue de Favoritos")
    print("OK: «Quitar de favoritos» la saca de la sección y del archivo")
else:
    sys.exit(f"Fase desconocida: {fase}")
