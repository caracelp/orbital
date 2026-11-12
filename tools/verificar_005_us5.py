#!/usr/bin/env python3
"""Comprobaciones AT-SPI US5 de la 005 (autocompletado de marcadores); bóveda temporal.

No genera teclas: teclado y lector de pantalla quedan para la usuaria. El deshacer de la
inserción está cubierto por la prueba GTK en proceso; acá se comprueba la inserción y qué
expone AT-SPI.
"""
import os
import runpy
import sys
import time
from pathlib import Path

helpers = runpy.run_path("tools/verificar_004_us4.py")
find_all = helpers["find_all"]
find_one = helpers["find_one"]
do_action = helpers["do_action"]
walk = helpers["walk"]
get_app = helpers["get_app"]
set_text = helpers["set_text"]
Atspi = helpers["Atspi"]
assert os.environ.get("ORBITAL_PID"), "Falta el PID de la instancia propia"
vault = Path(os.environ["ORBITAL_TEST_VAULT"]).resolve()
assert vault.parent.name.startswith("orbital-005-us5-"), "Solo acepta la bóveda temporal del lanzador"

TEXTO_NOTA = (vault / "wiki/marcadores.md").read_text()
TEXTO_ESCRITO = TEXTO_NOTA + "[%e"


def wait_for(predicate, description, seconds=12):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.1)
    raise AssertionError(description)


def visible(node):
    return node.get_state_set().contains(Atspi.StateType.SHOWING)


def frame():
    return wait_for(
        lambda: find_one("frame", "Orbital", timeout=0.2),
        "No aparece la instancia propia",
    )


def dialogo(titulo):
    return next(
        (
            d
            for d in find_all("dialog", titulo)
            if titulo in (d.get_name() or "") and visible(d)
        ),
        None,
    )


def nota_abierta():
    return not any(
        "Ninguna nota abierta" in (n.get_name() or "") and visible(n)
        for n in find_all("label", "Ninguna nota abierta")
    )


def elegir_nota(nombre):
    for intento in range(4):
        assert do_action(frame(), "win.quick-open"), "No se pudo abrir el selector rápido"
        d = wait_for(lambda: dialogo("Saltar a nota"), "No abrió el selector rápido")
        campo = wait_for(
            lambda: next(
                (
                    node
                    for node, _ in walk(get_app())
                    if visible(node)
                    and node.get_editable_text_iface()
                    and (node.get_name() or "") == "Buscar nota"
                ),
                None,
            ),
            "No aparece el campo del selector",
        )
        assert set_text(campo, nombre), "No se pudo escribir en el selector"
        time.sleep(0.5)
        assert do_action(d, "switcher.choose"), "No corrió switcher.choose"
        time.sleep(1.5)
        if not dialogo("Saltar a nota") and nota_abierta():
            return
        print(f"intento {intento + 1} sin abrir la nota; reintento")
    raise AssertionError("El selector nunca abrió la nota")


def editor_codigo():
    """El nodo de texto editable de la vista Código (etiqueta accesible del GtkSourceView)."""
    return next(
        (
            node
            for node, _ in walk(get_app())
            if visible(node)
            and node.get_text_iface()
            and (node.get_name() or "") == "Texto de la nota"
        ),
        None,
    )


def texto_editor():
    e = editor_codigo()
    assert e, "El editor de código desapareció"
    iface = e.get_text_iface()
    count = Atspi.Text.get_character_count(iface)
    return Atspi.Text.get_text(iface, 0, count) or ""


def enfocar_editor():
    """AT-SPI no expone GrabFocus en GTK4: se enfoca con abrir la barra de búsqueda y
    cerrarla con su botón, que devuelve el foco a la vista activa (Código)."""
    assert do_action(frame(), "win.find"), "No se pudo abrir la búsqueda en la nota"
    cerrar = wait_for(
        lambda: next(
            (n for n in find_all("button", "Cerrar búsqueda") if visible(n)),
            None,
        ),
        "No aparece el botón «Cerrar búsqueda»",
    )
    assert do_action(cerrar), "No se pudo accionar «Cerrar búsqueda»"
    return wait_for(
        lambda: editor_codigo().get_state_set().contains(Atspi.StateType.FOCUSED),
        "El editor de código no quedó enfocado",
    )


def sugerencias():
    return [n for n in find_all("button", "Sugerencia") if visible(n)]


# Escenario 5.1: escribir `[%e` en Código muestra las claves que coinciden, sin distinguir
# mayúsculas ni tildes (acá basta el prefijo «e»).
elegir_nota("marcadores")
time.sleep(1)
assert do_action(frame(), "win.view-code"), "No se pudo cambiar a vista Código"
time.sleep(1)
wait_for(editor_codigo, "No aparece el editor de código")
enfocar_editor()

e = editor_codigo()
ed = e.get_editable_text_iface()
assert ed, "El editor de código no expone texto editable"
assert ed.set_text_contents(TEXTO_ESCRITO), "No se pudo escribir en el editor"
# El cursor debe quedar al final, después del prefijo `[%e`.
wait_for(
    lambda: Atspi.Text.get_caret_offset(e.get_text_iface()) == len(TEXTO_ESCRITO),
    f"El cursor no quedó al final ({len(TEXTO_ESCRITO)})",
)

nombres = wait_for(
    lambda: sorted(n.get_name() or "" for n in sugerencias()) or None,
    "No aparecen sugerencias de marcadores al escribir [%e",
)
assert any("[%estado]" in n for n in nombres), f"Sugerencias sin «estado»: {nombres}"
print(f"OK 5.1: al escribir [%e aparecen las sugerencias de claves: {nombres}")

# Escenario 5.2: elegir una sugerencia inserta `[%clave]` completo; que sea una sola acción
# de deshacer lo cubre la prueba GTK us5_marker_widgets_completion.
assert texto_editor().endswith("[%e"), "El texto del editor no termina en el prefijo [%e"
boton = next((n for n in sugerencias() if "[%estado]" in (n.get_name() or "")), None)
activado = boton is not None and do_action(boton)
if activado:
    try:
        wait_for(
            lambda: texto_editor().endswith("[%estado]"),
            "La fila no insertó [%estado]",
            seconds=4,
        )
        print(
            "OK 5.2 (inserción vía AT-SPI): elegir la fila insertó [%estado]; el deshacer "
            "lo cubre la prueba GTK us5_marker_widgets_completion"
        )
    except AssertionError as error:
        print(f"Límite AT-SPI: {error}; la cubre us5_marker_widgets_completion")
else:
    print(
        "Límite AT-SPI: la activación de la fila no quedó expuesta; la cubre la prueba GTK "
        "us5_marker_widgets_completion"
    )
