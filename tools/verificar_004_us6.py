#!/usr/bin/env python3
"""Comprobaciones AT-SPI US6; solo el PID y la bóveda temporal del lanzador.

No genera teclas. El test GTK del lanzador (us6_tag_rename_buffers) cubre buffers,
flush y Deshacer con edición abierta. La navegación física con teclado y lector de
pantalla queda explícitamente pendiente; este script no marca T039.
"""
import os
import runpy
import time
from pathlib import Path

helpers = runpy.run_path("tools/verificar_004_us4.py")
find_all = helpers["find_all"]
do_action = helpers["do_action"]
walk = helpers["walk"]
get_app = helpers["get_app"]
get_entry_text = helpers["get_entry_text"]
set_text = helpers["set_text"]
Atspi = helpers["Atspi"]
assert os.environ.get("ORBITAL_PID"), "Falta el PID de la instancia propia"
vault = Path(os.environ["ORBITAL_TEST_VAULT"]).resolve()
assert vault.parent.name.startswith("orbital-004-us6-"), "Solo acepta la bóveda temporal del lanzador"

UNO = vault / "wiki/uno.md"
DOS = vault / "wiki/dos.html"
UNO_ORIGINAL = UNO.read_bytes()
DOS_ORIGINAL = DOS.read_bytes()
assert b"#fisica" in UNO_ORIGINAL and b"fisica/termodinamica" in DOS_ORIGINAL, "La bóveda del lanzador no es la esperada"


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


def button(name):
    return next(
        (
            node
            for node in find_all("button", name)
            if node.get_name() == name and visible(node)
        ),
        None,
    )


def click(name):
    node = wait_for(lambda: button(name), f"No aparece el botón {name!r}", 6)
    assert do_action(node, "click"), f"No se pudo accionar {name!r}"
    return node


def label_text(fragment):
    for node in find_all("label", fragment):
        if fragment in (node.get_name() or "") and visible(node):
            return node.get_name()
    return None


wait_for(get_app, "No se encontró la instancia propia de Orbital")
# 025: las etiquetas son una sección del explorador (siempre a la vista en modo Explorador).
tab = next(node for node in find_all("toggle button", "Explorador de archivos") if visible(node))
do_action(tab, "click")

# Diálogo de renombre: nombre nuevo accesible y con el viejo precargado.
click("Renombrar #fisica")
wait_for(lambda: label_text("Renombrar etiqueta"), "No abrió el diálogo de renombrar etiqueta")
entry = next(
    node
    for node, _ in walk(get_app())
    if visible(node)
    and node.get_editable_text_iface()
    and (node.get_name() or "") == "Nombre nuevo de etiqueta"
)
pre = get_entry_text(entry)
assert pre == "fisica", f"El campo no viene precargado con el nombre viejo: {pre!r}"
assert set_text(entry, "ciencia"), "No se pudo escribir el nombre nuevo"
time.sleep(0.3)
click("Continuar")

# Confirmación: cantidad de notas y aviso de fusión con el destino existente.
cuerpo = wait_for(
    lambda: label_text("«#fisica» y sus hijas pasarán a «#ciencia»"),
    "No apareció la confirmación del renombre",
)
assert "en 2 notas" in cuerpo, f"La confirmación no indica 2 notas: {cuerpo!r}"
assert "fusionarán" in cuerpo, f"La confirmación no avisa la fusión: {cuerpo!r}"
click("Renombrar")

# Aplicación en disco: la hija y el padre cambian; parecidas y código se conservan.
def aplicado():
    uno = UNO.read_bytes()
    dos = DOS.read_bytes()
    return (
        b"tags: [ciencia, otra]" in uno
        and b"Texto #ciencia #fisicas `#fisica`." in uno
        and b"tags: [ciencia/termodinamica, ciencia]" in dos
        and b"<p>#ciencia/hija</p>" in dos
        and b"<code>#fisica</code>" in dos
    )

wait_for(aplicado, "Los archivos no reflejan el renombre")
print("OK 6.1/6.2: confirmación con cantidad y fusión; padre/hijas renombradas, parecidas y código intactos")

# El árbol se actualiza con el renombre.
wait_for(lambda: button("Renombrar #ciencia"), "El árbol no muestra la etiqueta nueva")
wait_for(lambda: button("Renombrar #fisica") is None, "El árbol conserva la etiqueta vieja")

# Deshacer desde el toast y verificación byte a byte.
undo = wait_for(lambda: button("Deshacer"), "No apareció el aviso con Deshacer", 8)
assert do_action(undo, "click"), "No se pudo accionar Deshacer"
wait_for(lambda: UNO.read_bytes() == UNO_ORIGINAL and DOS.read_bytes() == DOS_ORIGINAL,
         "Deshacer no restauró los archivos byte a byte")
print("OK 6.3: Deshacer del toast restaura los archivos byte a byte")
print("Pendiente comprobación manual con nota abierta/editada, teclado y lector de pantalla. T039 sigue pendiente hasta revisar estos resultados.")
