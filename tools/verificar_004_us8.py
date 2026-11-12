#!/usr/bin/env python3
"""Comprobaciones AT-SPI US8 (paleta de comandos); solo el PID y la bóveda temporal del lanzador.

No genera teclas: correr un comando con Intro físico y navegar con flechas lo
cubre la prueba GTK en proceso (us8_palette_widgets_choose_and_disabled) y la
revisión manual de teclado y lector de pantalla. Este script no cierra T045
por sí solo.

Fases:
  sinnota  paleta sin nota abierta: comandos con atajo y filtrado (8.1); los
           que requieren nota aparecen desactivados y no corren (8.2); un
           comando habilitado corre y cierra la paleta.
  connota  con una nota abierta por el selector rápido: el mismo comando
           queda habilitado (8.2).
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
assert os.environ.get("ORBITAL_TEST_VAULT", "").count("orbital-004-us8-") == 1, (
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


def filas_comando():
    """Filas de la paleta con su texto visible concatenado.

    El nombre accesible de fila queda vacío: GTK pisa el Property::Label del
    contenedor con sus hijos (mismo límite documentado en T034 para US5), así
    que se lee el texto de las etiquetas internas (nombre, atajo, estado).
    """
    lista = wait_for(
        lambda: find_one("list", "Resultados del selector", timeout=0.2),
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


def campo_dialogo(nombre):
    return next(
        (
            node
            for node, _ in walk(get_app())
            if visible(node)
            and node.get_editable_text_iface()
            and (node.get_name() or "") == nombre
        ),
        None,
    )


def escribir(nombre_campo, texto):
    campo = wait_for(lambda: campo_dialogo(nombre_campo), f"No aparece el campo {nombre_campo!r}")
    assert set_text(campo, texto), f"No se pudo escribir en {nombre_campo!r}"
    time.sleep(0.3)


def abrir_paleta():
    assert do_action(frame(), "win.command-palette"), "No se pudo abrir la paleta"
    wait_for(lambda: dialogo("Paleta de comandos"), "No abrió la paleta de comandos")
    wait_for(lambda: filas_comando(), "No aparecieron comandos en la paleta")


def elegir(titulo_dialogo):
    """Corre la acción switcher.choose del diálogo (como Intro, sin teclas)."""
    d = dialogo(titulo_dialogo)
    assert d is not None, f"No está el diálogo {titulo_dialogo!r}"
    assert do_action(d, "switcher.choose"), f"No corrió switcher.choose en {titulo_dialogo!r}"


def etiqueta_visible(texto):
    return any(texto in (n.get_name() or "") for n in find_all("label", texto) if visible(n))


if sys.argv[1] == "sinnota":
    abrir_paleta()

    # Escenario 8.1: comandos con su nombre y atajo en etiquetas internas.
    # La lista es virtualizada: solo hay filas visibles en el árbol.
    filas = filas_comando()
    textos = [texto for texto, _ in filas]
    assert any("Nota nueva" in t and "Ctrl+N" in t for t in textos), textos
    assert any("Diario de hoy" in t for t in textos), textos
    con_atajo = [
        t for t in textos if "Ctrl+" in t or " F2 " in t or " Supr " in t
    ]
    assert len(con_atajo) >= 5, f"Faltan atajos visibles: {textos}"
    print(f"OK 8.1 parcial: {len(filas)} filas visibles con atajos (lista virtualizada)")

    # Escenario 8.2: sin nota abierta, los comandos que la requieren salen
    # desactivados y no se ejecutan.
    escribir("Buscar comando", "archi")
    restantes = filas_comando()
    assert len(restantes) == 1, f"El filtro no dejó solo «Archivar la nota»: {restantes}"
    texto, nodo = restantes[0]
    assert "Archivar la nota" in texto, texto
    assert "No disponible" in texto, f"Sin aviso de desactivado: {texto}"
    assert "Ctrl+Mayús+A" in texto, f"Sin atajo: {texto}"
    # El ítem de lista puede seguir sensible; el contenedor interno es el gris.
    contenedor = nodo.get_child_at_index(0)
    assert contenedor is not None and not contenedor.get_state_set().contains(
        Atspi.StateType.SENSITIVE
    ), "La fila desactivada no debe estar sensible"
    elegir("Paleta de comandos")
    time.sleep(0.5)
    assert dialogo("Paleta de comandos") is not None, (
        "Elegir un comando desactivado no debe cerrar ni correr nada"
    )
    print("OK 8.2 parcial: «Archivar la nota» desactivada sin nota abierta, y no corre")

    # El filtro también encuentra por contenido, no solo por prefijo.
    escribir("Buscar comando", "retroenlaces")
    restantes = filas_comando()
    assert len(restantes) == 1 and "retroenlaces" in restantes[0][0].lower(), restantes
    print("OK 8.1: filtrado en vivo por prefijo y por contenido")

    # Un comando habilitado corre y cierra la paleta.
    escribir("Buscar comando", "toda la bóveda")
    restantes = filas_comando()
    assert len(restantes) == 1 and "Buscar en toda la bóveda" in restantes[0][0], restantes
    elegir("Paleta de comandos")
    wait_for(
        lambda: campo_dialogo("Campo de búsqueda") and not dialogo("Paleta de comandos"),
        "Correr «Buscar en toda la bóveda» no enfocó el panel de búsqueda",
    )
    print("OK: comando habilitado corre y cierra la paleta")
elif sys.argv[1] == "connota":
    # Abrir una nota por el selector rápido (US2): filtrar y elegir con
    # switcher.choose, que AT-SPI sí expone en el diálogo.
    assert do_action(frame(), "win.quick-open"), "No se pudo abrir el selector rápido"
    d = wait_for(lambda: dialogo("Saltar a nota"), "No abrió el selector rápido")
    escribir("Buscar nota", "nota")
    wait_for(lambda: len(filas_comando()) >= 1, "El selector no encontró la nota")
    elegir("Saltar a nota")
    wait_for(lambda: not dialogo("Saltar a nota"), "El selector no se cerró al elegir")
    wait_for(
        lambda: not etiqueta_visible("Ninguna nota abierta"),
        "La nota no abrió en el editor",
    )

    # Con nota abierta, el mismo comando queda habilitado (8.2).
    abrir_paleta()
    escribir("Buscar comando", "archi")
    restantes = filas_comando()
    assert len(restantes) == 1, f"El filtro no dejó solo «Archivar la nota»: {restantes}"
    texto, nodo = restantes[0]
    assert "No disponible" not in texto, f"Sigue desactivada con nota abierta: {texto}"
    contenedor = nodo.get_child_at_index(0)
    assert contenedor is not None and contenedor.get_state_set().contains(
        Atspi.StateType.SENSITIVE
    ), "La fila debe quedar sensible con nota abierta"
    print("OK 8.2: «Archivar la nota» habilitada con nota abierta")
else:
    raise SystemExit(f"Fase desconocida: {sys.argv[1]}")
