#!/usr/bin/env python3
"""Guardar, reiniciar, ejecutar, renombrar y borrar por AT-SPI, solo el PID propio."""
import os
import runpy
import sys
import time

helpers = runpy.run_path("tools/verificar_004_us4.py")
Atspi = helpers["Atspi"]
assert os.environ.get("ORBITAL_PID"), "Falta PID de instancia propia"


def wait_for(check, message, seconds=15):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.1)
    raise AssertionError(message)


def exact(name):
    return next((node for node in helpers["find_all"]("*", name)
                 if node.get_name() == name and node.get_state_set().contains(Atspi.StateType.SHOWING)), None)


def click(name):
    # El botón puede estar insensible hasta que la consulta es válida: esperar habilitado.
    def disponible():
        node = exact(name)
        return node if node and node.get_state_set().contains(Atspi.StateType.SENSITIVE) else None

    node = wait_for(disponible, f"No aparece (o no se habilita) {name}")
    assert helpers["do_action"](node), f"Sin acción AT-SPI en {name}"


def write(name, text):
    node = wait_for(lambda: exact(name), f"No aparece campo {name}")
    assert node.get_editable_text_iface().set_text_contents(text)
    time.sleep(0.3)


def field_text(name):
    node = wait_for(lambda: exact(name), f"No aparece campo {name}")
    return helpers["get_entry_text"](node)


win = wait_for(lambda: helpers["find_one"]("frame", "Orbital", timeout=0.2), "No aparece instancia propia")
assert helpers["do_action"](win, "win.search")
wait_for(lambda: exact("Campo de búsqueda"), "No abrió búsqueda")
if sys.argv[1] == "crear":
    write("Campo de búsqueda", "tag:#fisica")
    click("Guardar búsqueda")
    write("Nombre de búsqueda guardada", "Pendientes física")
    click("Guardar")
    # El botón de ejecutar expone como nombre su texto visible (GTK lo pisa frente al
    # Property::Label); esperar por el botón de renombrar, cuyo nombre sí es el accesible.
    wait_for(lambda: exact("Renombrar búsqueda Pendientes física"), "No apareció la búsqueda guardada")
    print("OK: búsqueda guardada y visible")
else:
    click("Pendientes física")
    wait_for(lambda: field_text("Campo de búsqueda") == "tag:#fisica", "No ejecutó consulta persistida")
    click("Renombrar búsqueda Pendientes física")
    write("Nombre de búsqueda guardada", "Física renombrada")
    click("Renombrar")
    wait_for(lambda: exact("Renombrar búsqueda Física renombrada"), "No renombró búsqueda")
    click("Borrar búsqueda Física renombrada")
    click("Borrar")
    wait_for(lambda: exact("Borrar búsqueda Física renombrada") is None, "No borró búsqueda")
    print("OK: persistencia tras reinicio, ejecución, renombre y borrado")
