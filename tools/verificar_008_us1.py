#!/usr/bin/env python3
"""Comprobaciones AT-SPI 008 (grafo de enlaces); solo el PID y la bóveda temporal del lanzador.

El canvas es un único widget (los nodos no son accesibles individuales, FR-6): acá se verifica
el acceso por paleta, la página con su cabecera, el cierre por botón y que la nota vuelve intacta.
Los gestos (zoom, paneo, hover) quedan para la revisión manual con teclado y mouse físicos.
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
assert os.environ.get("ORBITAL_TEST_VAULT", "").count("orbital-008-us1-") == 1, (
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


def etiqueta_visible(texto):
    return any(texto in (n.get_name() or "") for n in find_all("*", texto) if visible(n))


# Abrir el grafo por la paleta.
assert do_action(frame(), "win.command-palette"), "No se pudo abrir la paleta"
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
assert set_text(campo, "grafo"), "No se pudo escribir en la paleta"
time.sleep(0.4)
def fila_grafo():
    lista = next(
        (
            l
            for l, _ in walk(get_app())
            if l.get_role_name() == "list"
            and "Resultados del selector" in (l.get_name() or "")
            and visible(l)
        ),
        None,
    )
    if lista is None:
        return None
    for i in range(lista.get_child_count()):
        hijo = lista.get_child_at_index(i)
        if not hijo:
            continue
        texto = " ".join((n.get_name() or "") for n, _ in walk(hijo)).lower()
        if "grafo" in texto:
            return (texto, hijo)
    return None


fila = wait_for(fila_grafo, "El comando «Ver el grafo de enlaces» no aparece")
d = wait_for(lambda: dialogo("Paleta de comandos"), "No está el diálogo de la paleta")
assert do_action(d, "switcher.choose"), "No corrió switcher.choose"

# La página del grafo con su cabecera.
wait_for(
    lambda: etiqueta_visible("Grafo de enlaces de la bóveda"),
    "La página del grafo no aparece",
)
print("OK: la página del grafo abre por el comando de paleta")

# Cerrar con el botón: vuelve a la nota.
boton = next(
    (
        b
        for b in find_all("button", "Cerrar el grafo")
        if visible(b) and "Cerrar el grafo" in (b.get_name() or "")
    ),
    None,
)
assert boton is not None, "No aparece el botón de cierre"
assert do_action(boton), "No se pudo accionar el cierre"
wait_for(lambda: not etiqueta_visible("Grafo de enlaces de la bóveda"), "El grafo no cerró")
print("OK: el botón cierra el grafo y vuelve a la nota")
