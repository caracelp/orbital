#!/usr/bin/env python3
"""Comprobaciones AT-SPI US2 de la 005 (resolución por sección); solo el PID propio.

No genera teclas: teclado y lector de pantalla quedan para la usuaria.
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
assert os.environ.get("ORBITAL_TEST_VAULT", "").count("orbital-005-us2-") == 1, (
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


def esperar_texto(fragmento, descripcion):
    def chequeo():
        t = textos()
        return t if fragmento in t else None

    def falla():
        t = textos()
        return f"{descripcion}. Texto visible ({len(t)} caracteres): {t[:600]!r}"

    return wait_for(chequeo, falla())


# Escenario 2.1: dos secciones con la misma clave y distinto valor.
elegir_nota("secciones")
time.sleep(1)
texto = esperar_texto("Marcador: activo", "La sección A no muestra su valor")
esperar_texto("Marcador: pausado", "La sección B no muestra su valor")
assert "[%estado]" not in texto, "La Lectura muestra un marcador sin resolver"
print("OK 2.1: cada sección muestra su propio valor de la misma clave")

# Escenario 2.2: la sección C sin campo inline cae al YAML (global).
esperar_texto("Marcador: global", "La sección C no cayó al YAML")
print("OK 2.2: la sección sin campo inline usa la propiedad YAML")
