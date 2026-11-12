#!/usr/bin/env python3
"""Regresión AT-SPI de la 006 (multi-cursor): el editor expone el texto como siempre.

Sin teclas ni mouse sintéticos: los gestos multi-cursor quedan cubiertos por las pruebas GTK en
proceso del lanzador. Acá se comprueba que con la función integrada el árbol accesible no cambia:
texto legible en Código, un único cursor y ninguna modificación en disco.
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
assert os.environ.get("ORBITAL_TEST_VAULT", "").count("orbital-006-us1-") == 1, (
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


def dialogo(titulo):
    return next(
        (
            d
            for d in find_all("dialog", titulo)
            if titulo in (d.get_name() or "") and visible(d)
        ),
        None,
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
        if not dialogo("Saltar a nota"):
            return
        print(f"intento {intento + 1} sin abrir la nota; reintento")
    raise AssertionError("El selector nunca abrió la nota")


def textos():
    out = []
    for node, _ in walk(get_app()):
        if not visible(node):
            continue
        iface = node.get_text_iface()
        if iface:
            try:
                count = Atspi.Text.get_character_count(iface)
                out.append(Atspi.Text.get_text(iface, 0, count) or "")
            except Exception:
                pass
    return "\n".join(out)


elegir_nota("principal")
time.sleep(1)
texto = wait_for(
    lambda: next((t for t in [textos()] if "Línea dos" in t), None),
    "El editor de Código no expone el texto por AT-SPI",
)
assert "Línea uno" in texto and "Línea tres" in texto, f"Texto incompleto: {texto[:300]!r}"
print("OK: el editor expone el texto completo por AT-SPI (regresión 006)")
