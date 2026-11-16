#!/usr/bin/env python3
"""Comprobaciones AT-SPI US5; solo el PID y la bóveda temporal del lanzador.

No genera teclas. Las pruebas GTK del lanzador cubren inserción/deshacer, foco,
recorrido de sugerencias y enlaces del lector. La navegación física con teclado
queda explícitamente pendiente; este script no marca T034.
"""
import os
import runpy
import time
import unicodedata
from pathlib import Path

helpers = runpy.run_path("tools/verificar_004_us4.py")
find_all = helpers["find_all"]
do_action = helpers["do_action"]
walk = helpers["walk"]
get_app = helpers["get_app"]
Atspi = helpers["Atspi"]
assert os.environ.get("ORBITAL_PID"), "Falta el PID de la instancia propia"
vault = Path(os.environ["ORBITAL_TEST_VAULT"]).resolve()
assert vault.parent.name.startswith("orbital-004-us5-"), "Solo acepta la bóveda temporal del lanzador"


def wait_for(predicate, description):
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.1)
    raise AssertionError(description)


def visible(node):
    return node.get_state_set().contains(Atspi.StateType.SHOWING)


def fold(text):
    # Igual que orbital_vault::fold para Latin-1: minúsculas sin tildes ni diéresis.
    decomposed = unicodedata.normalize("NFD", text.lower())
    return "".join(c for c in decomposed if not unicodedata.combining(c)).replace("ß", "ss")


def exact(name):
    return next((node for node in find_all("*", name) if node.get_name() == name and visible(node)), None)


def tag_button(name, count):
    """La fila del árbol expone el botón '#<forma más usada> (<notas>)'; comparar plegado."""
    wanted = fold(f"#{name} ({count})")
    return next(
        (
            node
            for node in find_all("button", "#")
            if node.get_name().startswith("#")
            and "Renombrar" not in node.get_name()
            and fold(node.get_name()) == wanted
            and visible(node)
        ),
        None,
    )


def activate(node):
    # Un label puede colgar de la fila que expone la acción. Nunca ejecutar scroll-to
    # como sustituto de activación ni enviar teclas globales.
    while node is not None and str(node.get_process_id()) == os.environ["ORBITAL_PID"]:
        action = node.get_action_iface()
        if action:
            for index in range(action.get_n_actions()):
                name = action.get_action_name(index).lower()
                if name in ("activate", "click", "press", "toggle"):
                    assert action.do_action(index)
                    return
        node = node.get_parent()
    raise AssertionError("El elemento no expone activación AT-SPI; verificar con la prueba GTK y registrar este límite")


wait_for(get_app, "No se encontró la instancia propia de Orbital")
# 025: las etiquetas son una sección del explorador; basta con el modo Explorador.
activate(wait_for(lambda: exact("Explorador de archivos"), "No aparece el modo Explorador"))
# 027: en una ventana baja, Etiquetas es lo primero que cede su lugar (src/reparto.rs). Se pliega
# la sección Explorador, como haría la usuaria, para que el árbol de etiquetas tenga su alto.
activate(wait_for(lambda: exact("Explorador"), "No aparece la sección Explorador"))
root = wait_for(lambda: tag_button("fisica", 2), "Falta raíz Física con conteo único de 2 notas")
wait_for(lambda: tag_button("biologia", 1), "No se indexó la nota HTML Unicode")
for ignored in ("#123", "#ff00ff", "#codigo", "#oculta"):
    assert not any(fold(node.get_name()).startswith(fold(ignored)) for node in find_all("button", "#")), ignored
print("OK 5.1/5.4 parcial: raíces, conteos y exclusiones MD/HTML visibles")

# Jerarquía: expandir/contraer la raíz por AT-SPI y ver a la hija con su conteo.
act = root.get_action_iface()
names = [act.get_action_name(i) for i in range(act.get_n_actions())]
expand_idx = next((i for i, n in enumerate(names) if n in ("listitem.toggle-expand", "listitem.expand")), None)
assert expand_idx is not None, f"La fila no expone expansión: {names}"
if root.get_state_set().contains(Atspi.StateType.EXPANDED):
    assert act.do_action(expand_idx), "toggle-expand (contraer) falló"
    wait_for(lambda: tag_button("termodinamica", 1) is None, "La hija sigue visible tras contraer")
assert act.do_action(expand_idx), "toggle-expand (expandir) falló"
wait_for(lambda: tag_button("termodinamica", 1), "La hija Termodinámica no aparece al expandir")
print("OK jerarquía: contraer/expandir por AT-SPI muestra #Termodinámica (1)")

# 5.2 (elegir abre la búsqueda): GTK 4.24 no expone la activación de fila de ListView
# por AT-SPI (solo listitem.*); la cubre la prueba GTK us5_tag_widgets_panel, que
# comprueba que elegir produce la consulta tag:#fisica con sus hijas.
if any(n in ("activate", "click", "press") for n in names):
    activate(root)
    entry = wait_for(lambda: exact("Campo de búsqueda"), "Elegir etiqueta no abrió búsqueda")
    wait_for(lambda: entry.get_text_iface().get_text(0, -1) == "tag:#fisica", "La consulta de la etiqueta no coincide")
    wait_for(lambda: exact("2 notas"), "La búsqueda del padre no incluye las notas hijas")
    print("OK 5.2: elegir Física abre tag:#fisica e incluye sus hijas")
else:
    print("Límite AT-SPI: activación de fila no expuesta; 5.2 cubierto por la prueba GTK en proceso")

# Actualización externa: se elimina una etiqueta y se agrega otra, sin reabrir bóveda.
(vault / "wiki/uno.md").write_text("Texto #nueva.\n", encoding="utf-8")
activate(wait_for(lambda: exact("Explorador de archivos"), "No aparece el modo Explorador"))
wait_for(lambda: tag_button("nueva", 1), "El árbol no se actualizó ante cambio externo")
wait_for(lambda: tag_button("fisica", 1), "El conteo del padre quedó desactualizado")
print("OK: actualización incremental del panel por cambio externo")
print("Pendiente comprobación manual de flechas/Intro/Escape y lector de pantalla. T034 sigue pendiente hasta revisar estos resultados.")
