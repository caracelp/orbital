#!/usr/bin/env python3
"""Comprobaciones AT-SPI US4 de la 005 (tabla de contenidos); solo el PID y la bóveda temporal.

No genera teclas: teclado y lector de pantalla quedan para la usuaria. La activación de
entradas (el scroll al encabezado) no deja señal observable en el texto de AT-SPI; la cubre
la prueba GTK en proceso us5_toc_reader_lists_headings.
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
assert vault.parent.name.startswith("orbital-005-us4-"), "Solo acepta la bóveda temporal del lanzador"


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


# Escenario 4.1: el TOC sin parámetros lista los encabezados de niveles 1 a 3, sin el codeblock.
elegir_nota("principal")
time.sleep(1)
texto = esperar_texto("Primera", "El TOC no lista Primera")
esperar_texto("Adentro", "El TOC no lista Adentro (nivel 3)")
esperar_texto("Segunda", "El TOC no lista Segunda")
assert texto.count("Principal") == 2, (
    f"El nivel 1 debe verse como encabezado del cuerpo y como entrada del TOC "
    f"(apariciones: {texto.count('Principal')})"
)
assert "```" not in texto, "La Lectura muestra el codeblock sin completar"
print("OK 4.1: el TOC lista los encabezados de niveles 1 a 3, sin el codeblock visible")

# Escenario 4.3: activar una entrada desplaza hasta el encabezado, si AT-SPI expone el botón.
# El scroll no cambia el texto de la nota, así que aunque la acción exista no hay señal
# observable por AT-SPI: se prueba igual y se informa.
activado = False
for boton in (n for n in find_all("button", "Primera") if visible(n) and n.get_name() == "Primera"):
    if do_action(boton):
        time.sleep(1.0)
        activado = True
        break
if activado:
    print("OK 4.3: la entrada del TOC expone acción AT-SPI (el scroll no es observable por texto)")
else:
    print(
        "Límite AT-SPI: la activación de entradas del TOC no quedó expuesta; la cubre la "
        "prueba GTK us5_toc_reader_lists_headings"
    )

# Escenario 4.2: minLevel: 2 y maxLevel: 3 dejan fuera el nivel 1 de la lista (que sigue
# visible como encabezado del cuerpo) y mantienen los de nivel 2 y 3.
elegir_nota("filtro")
time.sleep(1)
texto = esperar_texto("Segundo", "El TOC filtrado no lista Segundo")
esperar_texto("Profundo", "El TOC filtrado no lista Profundo (nivel 3)")
assert texto.count("Topo") == 1, (
    f"El encabezado de nivel 1 no debe entrar al TOC filtrado "
    f"(apariciones de Topo: {texto.count('Topo')})"
)
assert "```" not in texto, "La Lectura muestra el codeblock filtrado sin completar"
print("OK 4.2: minLevel/maxLevel dejan fuera el nivel 1 del TOC, visible como encabezado del cuerpo")
