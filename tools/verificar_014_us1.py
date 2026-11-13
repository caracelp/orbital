#!/usr/bin/env python3
"""Comprobaciones AT-SPI US1/US3 de OnStage (014); solo el PID y la bóveda temporal del lanzador.

No genera teclas ni usa diálogos (con libadwaita 1.10 el contenido de un AdwDialog deja de
exponerse por AT-SPI): la nota se abre desde la lista de la bóveda (expandir carpeta + elegir
fila) y OnStage se abre con la acción de ventana win.onstage.

El contenido de cada diapositiva lo verifica el test GTK en proceso
(us1_ventana_portada_navegacion_e_indice): leer el texto del TextView por AT-SPI con sondeos
frecuentes tumba el bridge de GTK 4.24. Aquí se verifican las transiciones con los rótulos
(contador, botones, toggles), que son nombres accesibles, no lecturas de texto.

Fases:
  presenta  abre a.md desde la lista, abre OnStage y verifica portada, contador, navegación por
            botones, índice y toggles.
  html      con una nota .html abierta, «Presentar» no abre una ventana nueva de OnStage.
"""
import os
import re
import runpy
import time

helpers = runpy.run_path("tools/verificar_004_us4.py")
find_all = helpers["find_all"]
find_one = helpers["find_one"]
do_action = helpers["do_action"]
Atspi = helpers["Atspi"]
assert os.environ.get("ORBITAL_PID"), "Falta el PID de la instancia propia"
assert os.environ.get("ORBITAL_TEST_VAULT", "").count("orbital-014-us1-") == 1, (
    "Solo acepta la bóveda temporal del lanzador"
)

# Pausa entre acciones y sondeo lento: el bridge AT-SPI de GTK 4.24 se queda con el árbol de la
# ventana si se lo consulta en ráfaga mientras la vista redibuja.
PAUSA = 1.0
SONDEO = 0.5


def wait_for(predicate, description, seconds=12):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(SONDEO)
    raise AssertionError(f"{description} (esperé {seconds}s)")


def visible(node):
    return node.get_state_set().contains(Atspi.StateType.SHOWING)


def frame():
    return wait_for(
        lambda: find_one("frame", "Orbital", timeout=0.2),
        "No aparece la instancia propia",
    )


def ventana_onstage():
    return next((v for v in find_all("frame", "OnStage") if visible(v)), None)


def activar(rol, nombre, timeout=10.0):
    node = wait_for(
        lambda: next((n for n in find_all(rol, nombre) if visible(n)), None),
        f"No está [{rol}] {nombre!r}",
        timeout,
    )
    accion = node.get_action_iface()
    assert accion is not None and accion.get_n_actions() > 0, f"{nombre!r} sin acciones"
    assert accion.do_action(0), f"No se pudo accionar {nombre!r}"
    time.sleep(PAUSA)


def elegir(nombre):
    """Elige la fila de la lista de la bóveda cuyo nombre contiene <nombre> (lista virtual)."""
    lista = wait_for(
        lambda: find_one("list", "Contenido de la bóveda", timeout=0.5),
        "No está la lista de la bóveda",
    )
    hits = []
    for _ in range(50):
        nombres = [
            lista.get_child_at_index(i).get_name() or ""
            for i in range(lista.get_child_count())
        ]
        hits = [i for i, n in enumerate(nombres) if nombre.lower() in n.lower()]
        if hits or not nombres:
            break
        ultima = lista.get_child_at_index(len(nombres) - 1).get_action_iface()
        if ultima is None:
            break
        antes = nombres[-1]
        ultima.do_action(0)
        time.sleep(SONDEO)
        if lista.get_child_at_index(lista.get_child_count() - 1).get_name() == antes:
            break
    assert hits, f"No hay ninguna fila con {nombre!r}"
    lista.get_selection_iface().select_child(hits[0])
    time.sleep(PAUSA)


def nota_abierta():
    return not any(
        "Elegí una nota de la lista" in (n.get_name() or "") and visible(n)
        for n in find_all("label", "Elegí una nota de la lista")
    )


def etiqueta(texto):
    return any(texto in (n.get_name() or "") for n in find_all("label", texto) if visible(n))


def _buscar(rol, nombre):
    for b in find_all(rol, ""):
        if not visible(b):
            continue
        nombre_atr = b.get_name() or ""
        if nombre_atr == nombre or nombre in nombre_atr:
            return b
    return None


def apretar(nombre):
    b = wait_for(lambda: _buscar("button", nombre), f"No está el botón «{nombre}»", 5)
    assert do_action(b), f"No se pudo accionar «{nombre}»"
    time.sleep(PAUSA)


def alternar(nombre):
    """Alterna un toggle button de la barra (Portada, Índice, Calm)."""
    t = wait_for(lambda: _buscar("toggle button", nombre), f"No está el toggle «{nombre}»", 5)
    assert do_action(t), f"No se pudo accionar el toggle «{nombre}»"
    time.sleep(PAUSA)


def contador():
    for n in find_all("label", " de "):
        nombre = (n.get_name() or "").strip()
        if visible(n) and re.fullmatch(r"\d+ de \d+", nombre):
            return nombre
    return None


def re_presentar():
    """Vuelve a abrir OnStage: árbol AT-SPI fresco y estado conocido (portada, 1 de 3)."""
    assert do_action(frame(), "win.onstage"), "No se pudo (re)presentar"
    wait_for(lambda: contador() == "1 de 3", f"El contador no volvió a 1 de 3 ({contador()})")
    # Dejar terminar el volcado de la diapositiva y el ajuste de fuente antes de accionar.
    time.sleep(2.0)


def fase(nombre, cuerpo):
    """Corre una fase con hasta 3 intentos: el bridge AT-SPI de GTK 4.24 se queda con el árbol
    de la ventana de forma intermitente; con árbol fresco y pausa, el reintento pasa."""
    for intento in range(3):
        try:
            re_presentar()
            cuerpo()
            print(f"OK: {nombre}")
            return
        except AssertionError as e:
            print(f"intento {intento + 1} de «{nombre}» falló: {e}")
    raise AssertionError(f"«{nombre}» falló en los 3 intentos")


def fase_navegacion():
    apretar("Diapositiva siguiente")
    wait_for(lambda: contador() == "2 de 3", f"El contador no llegó a 2 de 3 ({contador()})")
    apretar("Diapositiva siguiente")
    wait_for(lambda: contador() == "3 de 3", f"El contador no llegó a 3 de 3 ({contador()})")


def fase_indice():
    alternar("Índice")
    wait_for(lambda: contador() == "1 de 4", f"El contador no es 1 de 4 ({contador()})")
    apretar("Diapositiva siguiente")
    wait_for(lambda: contador() == "2 de 4", f"El contador no es 2 de 4 ({contador()})")
    wait_for(lambda: etiqueta("1. Uno"), "El índice no lista «Uno»")
    wait_for(lambda: etiqueta("2. Dos"), "El índice no lista «Dos»")
    apretar("2. Dos")
    wait_for(lambda: contador() == "4 de 4", f"El contador no llegó a 4 de 4 ({contador()})")


def fase_portada():
    alternar("Portada")
    wait_for(lambda: contador() == "1 de 2", f"Sin portada el contador no es 1 de 2 ({contador()})")
    alternar("Portada")
    wait_for(lambda: contador() == "1 de 3", f"Con portada el contador no es 1 de 3 ({contador()})")


def fase_calm():
    # Calm va a pantalla completa: la barra con los controles se oculta (US3). Se verifica que
    # los toggles dejan de verse. La salida de pantalla completa no se acciona por AT-SPI: la
    # barra oculta no expone el toggle, y en GTK 4.24 el unfullscreen además deja el árbol
    # AT-SPI de la ventana sin la barra para siempre. La salida la cubre el test GTK en proceso
    # (F11/alternar) y la revisión manual de teclado.
    alternar("Calm")
    time.sleep(1.0)
    ocultos = [
        n for n in ("Portada", "Índice", "Calm")
        if _buscar("toggle button", n) is None or not visible(_buscar("toggle button", n))
    ]
    assert len(ocultos) == 3, f"La barra no se ocultó en modo calm: {ocultos}"


def main():
    # Fase 1: abrir a.md desde la lista (expandir wiki + elegir la fila) y presentar.
    activar("button", "wiki")
    wait_for(
        lambda: any("Nota a.md" in (n.get_name() or "") for n in find_all("list item", "")),
        "No apareció la fila de a.md",
    )
    elegir("a.md")
    wait_for(nota_abierta, "La nota no se abrió desde la lista")
    assert do_action(frame(), "win.onstage"), "No se pudo accionar win.onstage"
    wait_for(lambda: ventana_onstage(), "No abrió la ventana OnStage")
    wait_for(lambda: contador() == "1 de 3", f"El contador no llegó a 1 de 3 ({contador()})")
    wait_for(lambda: etiqueta("Órbitas"), "La portada no muestra el título del frontmatter")
    print("OK: OnStage abre con la portada (1 de 3)")

    fase("navegación por botones entre diapositivas con contador", fase_navegacion)
    fase("índice de encabezados con salto a diapositiva", fase_indice)
    fase("toggle de portada reconstruye la lista sin perder la diapositiva", fase_portada)
    fase("modo calm oculta los controles a pantalla completa", fase_calm)

    # Una nota .html no se presenta (aviso discreto, sin ventana nueva).
    elegir("nota-html")
    wait_for(nota_abierta, "La nota .html no se abrió desde la lista")
    antes = 1 if ventana_onstage() else 0
    assert do_action(frame(), "win.onstage"), "No se pudo accionar win.onstage con .html"
    time.sleep(1.5)
    ventanas = [v for v in find_all("frame", "OnStage") if visible(v)]
    assert len(ventanas) == antes, "Con .html cambió la cantidad de ventanas de OnStage"
    print("OK: OnStage no presenta notas .html")

    print("Comprobaciones 014 AT-SPI terminadas. Pendiente para la usuaria: teclado y lector de pantalla.")


if __name__ == "__main__":
    main()
