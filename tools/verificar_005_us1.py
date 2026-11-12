#!/usr/bin/env python3
"""Comprobaciones AT-SPI US1 de la 005 (interpolación); solo el PID y la bóveda temporal del lanzador.

No genera teclas: la revisión de teclado y lector de pantalla queda para la usuaria; este script
no cierra T012 por sí solo.
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
assert vault.parent.name.startswith("orbital-005-us1-"), "Solo acepta la bóveda temporal del lanzador"
NOTA = vault / "wiki/principal.md"


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
    """Abre la nota por el selector rápido (switcher.choose, que AT-SPI sí expone).

    Con reintentos: si la bóveda todavía no terminó de cargar, elegir puede cerrar el
    diálogo sin abrir nada."""
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
    """Texto de todos los nodos de texto visibles (Lectura o Código)."""
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

    wait_for(chequeo, falla())


if sys.argv[1] == "lectura":
    elegir_nota("principal")

    # 1.1 y 1.2: la Lectura muestra los valores (inline de la sección y YAML).
    esperar_texto("pausado", "El marcador con valor en YAML no se interpoló")
    wait_for(lambda: "«faltante»" in textos(), "El rótulo «faltante» no aparece")
    wait_for(
        lambda: "[%estado]" not in textos() and "[%otra]" not in textos(),
        "La Lectura muestra el marcador sin resolver",
    )
    print("OK 1.1/1.2: Lectura muestra inline, YAML y «faltante», sin marcadores planos")
elif sys.argv[1] == "codigo":
    # 1.3: en Código el marcador queda plano. Las esperas dejan asentarse el árbol AT-SPI:
    # lecturas inmediatas devuelven el estado anterior (nota no abierta).
    elegir_nota("principal")
    time.sleep(1)
    assert do_action(frame(), "win.view-code"), "No se pudo cambiar a vista Código"
    time.sleep(1)
    esperar_texto("[%estado]", "Código no muestra el marcador plano")
    esperar_texto("[%nada]", "Código no muestra el marcador sin valor")
    print("OK 1.3: Código muestra los marcadores planos")
else:
    raise SystemExit(f"Fase desconocida: {sys.argv[1]}")
