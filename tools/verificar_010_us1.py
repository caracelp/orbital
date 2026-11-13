#!/usr/bin/env python3
"""Comprobaciones AT-SPI de la 010 (historial de versiones); solo el PID propio del lanzador.

Valida el flujo US1 (guardar versiona automáticamente el contenido previo) y dos garantías del
diseño: las versiones no entran en la búsqueda y una nota sin historial avisa en vez de abrir
un diálogo vacío. Restaurar por AT-SPI no es accionable (el contenido de AdwDialog no se expone
en libadwaita 1.10); lo cubre la prueba GTK `us2_historial_dialogo`.
"""
import os
import runpy
import time
from pathlib import Path

helpers = runpy.run_path("tools/verificar_004_us4.py")
Atspi = helpers["Atspi"]
assert os.environ.get("ORBITAL_PID"), "Falta el PID de la instancia propia"
vault = Path(os.environ["ORBITAL_TEST_VAULT"]).resolve()
assert vault.parent.name.startswith("orbital-010-us1-"), "Solo acepta la bóveda temporal del lanzador"
historial = vault / ".orbital/historial/wiki/nota.md"
NOTA = vault / "wiki/nota.md"


def wait_for(check, message, seconds=15):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.15)
    raise AssertionError(message)


def visible(node):
    return node.get_state_set().contains(Atspi.StateType.SHOWING)


def exact(name):
    return next(
        (
            node
            for node in helpers["find_all"]("*", name)
            if node.get_name() == name and visible(node)
        ),
        None,
    )


def click_button(name):
    node = wait_for(lambda: exact(name), f"No aparece el botón {name!r}")
    assert helpers["do_action"](node), f"No se pudo accionar {name!r}"


def versiones():
    """Las versiones ordenadas por mtime (viejo → nuevo) con su contenido."""
    if not historial.exists():
        return []
    return sorted(
        ((p, p.read_text(encoding="utf-8")) for p in historial.iterdir() if p.is_file()),
        key=lambda par: par[0].stat().st_mtime,
    )


wait_for(lambda: helpers["get_app"](), "No se encontró la instancia propia de Orbital")
click_button("wiki")
frame = wait_for(
    lambda: next((n for n in helpers["find_all"]("frame", "Orbital") if visible(n)), None),
    "No hay ventana",
)
lista = wait_for(
    lambda: next((n for n in helpers["find_all"]("list", "Contenido de la bóveda") if visible(n)), None),
    "No hay lista de la bóveda",
)


def indice_fila(nombre):
    nombres = [lista.get_child_at_index(i).get_name() for i in range(lista.get_child_count())]
    return nombres.index(nombre) if nombre in nombres else None


def elegir_nota(nombre_fila):
    """Vuelve a la página Archivos y selecciona la fila (el clic no abre)."""
    global lista
    tab = next(
        (
            n
            for n in helpers["find_all"]("page tab", "Archivos")
            if n.get_name() == "Archivos" and visible(n)
        ),
        None,
    )
    if tab:
        helpers["do_action"](tab)
        time.sleep(0.4)
    lista = wait_for(
        lambda: next(
            (n for n in helpers["find_all"]("list", "Contenido de la bóveda") if visible(n)),
            None,
        ),
        "No hay lista de la bóveda",
    )
    indice = wait_for(lambda: indice_fila(nombre_fila), f"No está la fila {nombre_fila!r}")
    lista.get_selection_iface().select_child(indice)
    time.sleep(1.0)


elegir_nota("Nota nota.md")
assert helpers["do_action"](frame, "win.view-code"), "No se pudo pasar a la vista Código"
editor = wait_for(
    lambda: next(
        (
            n
            for n, _ in helpers["walk"](helpers["get_app"]())
            if visible(n)
            and (n.get_name() or "") == "Texto de la nota"
            and n.get_editable_text_iface()
        ),
        None,
    ),
    "No está el editor de código",
)

# US1: cada guardado deja el contenido previo como versión, la más nueva al final por mtime.
assert NOTA.read_text(encoding="utf-8") == "prehistórico\n", "La bóveda del lanzador no es la esperada"
ed = editor.get_editable_text_iface()
assert ed.set_text_contents("Primera edición con zanahoria\n")
wait_for(lambda: any(versiones()), "El primer guardado no creó versión")
assert ed.set_text_contents("Segunda edición\n")
wait_for(lambda: len(versiones()) >= 2, "El segundo guardado no creó versión")
v1, v2 = versiones()[0], versiones()[1]
assert v1[1] == "prehistórico\n", f"La primera versión no es el contenido previo: {v1[1]!r}"
assert v2[1] == "Primera edición con zanahoria\n", f"La segunda versión no es el contenido previo: {v2[1]!r}"
assert v1[0].stat().st_mtime <= v2[0].stat().st_mtime, "El orden por mtime no es viejo → nuevo"
print("OK US1: dos guardados → dos versiones con el contenido previo, en orden")

# Las versiones viven fuera del índice: buscar la palabra que solo existe en el historial.
assert helpers["do_action"](frame, "win.search"), "No se pudo abrir la búsqueda"
campo = wait_for(lambda: exact("Campo de búsqueda"), "No está el campo de búsqueda")
assert campo.get_editable_text_iface().set_text_contents("prehistórico")
wait_for(lambda: exact("Sin resultados"), "«prehistórico» aparece en la búsqueda: el historial entró al índice")
print("OK: el historial no entra en la búsqueda")

# Nota sin versiones: aviso en vez de diálogo. El diálogo con versiones sí abre.
elegir_nota("Nota otra.md")
assert helpers["do_action"](frame, "win.historial"), "No se pudo pedir el historial"
wait_for(
    lambda: exact("Esta nota todavía no tiene versiones en el historial"),
    "Una nota sin versiones no avisa",
)
print("OK: nota sin versiones muestra el aviso en vez de abrir el diálogo")

elegir_nota("Nota nota.md")
assert helpers["do_action"](frame, "win.historial"), "No se pudo pedir el historial"
wait_for(lambda: exact("Historial de versiones"), "El diálogo de historial no abrió")
print("OK US2 parcial: el diálogo abre; la lista y Restaurar no se exponen por AT-SPI (límite de libadwaita 1.10), cubiertos por la prueba GTK us2_historial_dialogo")
print("Pendiente para la usuaria: teclado y lector de pantalla.")
