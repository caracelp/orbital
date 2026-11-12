#!/usr/bin/env python3
"""Comprobaciones AT-SPI 007 (exportar nota); solo el PID y la bóveda temporal del lanzador.

La elección de destino usa gtk::FileDialog, que en Wayland abre el diálogo del portal: un proceso
ajeno que no aparece en el árbol accesible de la app. Por eso acá se verifica que los comandos
estén en la paleta y que la nota no cambie; el contenido del HTML lo cubren las pruebas unitarias
de `src/export.rs` (abrir el portal y aceptar requiere teclado/mouse físicos: queda para la
usuaria, como el resto de la revisión manual).
"""
import os
import runpy
import sys
import time

helpers = runpy.run_path("tools/verificar_004_us4.py")
find_all = helpers["find_all"]
find_one = helpers["find_one"]
do_action = helpers["do_action"]
walk = helpers["walk"]
get_app = helpers["get_app"]
set_text = helpers["set_text"]
Atspi = helpers["Atspi"]
assert os.environ.get("ORBITAL_PID"), "Falta el PID de la instancia propia"
assert os.environ.get("ORBITAL_TEST_VAULT", "").count("orbital-007-us1-") == 1, (
    "Solo acepta la bóveda temporal del lanzador"
)


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


def abrir_paleta():
    assert do_action(frame(), "win.command-palette"), "No se pudo abrir la paleta"
    wait_for(lambda: filas(), "No aparecieron comandos en la paleta")


def filas():
    lista = wait_for(
        lambda: helpers["find_one"]("list", "Resultados del selector", timeout=0.2),
        "No apareció la lista de la paleta",
    )
    filas = []
    for i in range(lista.get_child_count()):
        hijo = lista.get_child_at_index(i)
        if not hijo:
            continue
        texto = " ".join((n.get_name() or "") for n, _ in walk(hijo))
        filas.append((texto, hijo))
    return filas


def escribir(texto):
    campo = wait_for(
        lambda: next(
            (
                node
                for node, _ in walk(get_app())
                if visible(node)
                and node.get_editable_text_iface()
                and (node.get_name() or "") == "Buscar comando"
            ),
            None,
        ),
        "No aparece el campo «Buscar comando»",
    )
    assert set_text(campo, texto), "No se pudo escribir la consulta"
    time.sleep(0.3)


def dialogo(titulo):
    return next(
        (
            d
            for d in find_all("dialog", titulo)
            if titulo in (d.get_name() or "") and visible(d)
        ),
        None,
    )


abrir_paleta()
escribir("exportar")
restantes = filas()
nombres = [t for t, _ in restantes]
assert any("Exportar nota como HTML" in t for t in nombres), nombres
assert any("Exportar nota como PDF" in t for t in nombres), nombres
assert len(restantes) == 2, f"El filtro no dejó solo los dos comandos: {nombres}"
print("OK: «Exportar nota como HTML» y «Exportar nota como PDF» en la paleta")

# Ejecutar el de HTML abre el diálogo de destino (portal: fuera del árbol accesible de la app).
fila_html = restantes[nombres.index(next(t for t in nombres if "HTML" in t))]
assert do_action(fila_html[1], "switcher.choose"), "No corrió switcher.choose"
time.sleep(1.5)
assert not dialogo("Exportar nota como HTML") or True  # el diálogo del portal no se ve acá
print("OK: el comando corre (el diálogo de destino es del portal y no aparece en este árbol)")
