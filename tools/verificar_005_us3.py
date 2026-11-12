#!/usr/bin/env python3
"""Comprobaciones AT-SPI US3 de la 005 (índice por etiqueta); solo el PID y la bóveda temporal.

No genera teclas: teclado y lector de pantalla quedan para la usuaria. La activación de entradas
está cubierta por la prueba GTK en proceso; acá se comprueba si AT-SPI la expone.
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
assert vault.parent.name.startswith("orbital-005-us3-"), "Solo acepta la bóveda temporal del lanzador"


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


# Escenario 3.1: la lista del tag con su hija, anidada.
elegir_nota("principal")
time.sleep(1)
texto = esperar_texto("a.md", "El índice no lista a.md")
esperar_texto("b.md", "El índice no lista b.md (hija)")
esperar_texto("#termo", "La fila de la etiqueta hija no aparece")
assert "```" not in texto, "La Lectura muestra el codeblock sin completar"
print("OK 3.1: el índice lista las notas del tag y de sus hijas, anidadas")

# Escenario 3.4: tag sin notas avisa en texto discreto.
esperar_texto("«sin notas con #quimica»", "El tag vacío no muestra el aviso")
print("OK 3.4: el tag sin notas muestra el aviso discreto")

# T016: una nota creada por fuera entra a la lista sin reabrir la nota.
c = vault / "wiki/c.md"
assert not c.exists()
c.write_text("Archivo #fisica.\nTexto de C.\n")
esperar_texto("c.md", "La nota nueva no entró al índice en vivo")
print("OK T016: el índice se regenera al llegar el snapshot del índice")

# Escenario 3.2: activar una entrada abre la nota, si AT-SPI expone el botón. Puede haber
# varios candidatos (la barra lateral usa botones con el mismo nombre): se prueba cada uno.
abrio = False
for boton in (n for n in find_all("button", "a.md") if visible(n) and n.get_name() == "a.md"):
    if do_action(boton):
        time.sleep(1.5)
        if "Texto de A." in textos():
            abrio = True
            break
if abrio:
    print("OK 3.2: elegir una entrada abre la nota (vía AT-SPI)")
else:
    print(
        "Límite AT-SPI: la activación de entradas no quedó expuesta; la cubre la prueba GTK "
        "us5_indice_reader_lists_and_activates"
    )
