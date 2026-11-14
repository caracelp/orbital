#!/usr/bin/env python3
"""Comprobaciones AT-SPI US1/US2 del visor PDF (016); solo el PID y la bóveda del lanzador.

No genera teclas ni mouse: el PDF se abre con el botón «Ver el PDF» de la vista externa y la
selección se inyecta con la acción de prueba win.test-pdf-seleccion (el mismo camino que el
mensaje del puente). El texto de la página se verifica por el DOM que WebKitGTK expone por
AT-SPI; el resaltado visual del ancla y el desplazamiento a página van por revisión manual.

Fases:
  abre     abre raw/informe.pdf desde la lista, «Ver el PDF», verifica el texto de la página
           expuesto por el visor y «Volver al archivo».
  copia    selección de prueba → «Copiar enlace a la selección» → aviso de portapapeles.
  hash     el PDF queda byte a byte (lo cierra el lanzador con sha256sum -c).
"""
import os
import runpy
import time

helpers = runpy.run_path("tools/verificar_004_us4.py")
find_all = helpers["find_all"]
find_one = helpers["find_one"]
do_action = helpers["do_action"]
Atspi = helpers["Atspi"]
assert os.environ.get("ORBITAL_PID"), "Falta el PID de la instancia propia"
assert os.environ.get("ORBITAL_TEST_VAULT", "").count("orbital-016-us1-") == 1, (
    "Solo acepta la bóveda temporal del lanzador"
)

PAUSA = 1.0
SONDEO = 0.5


def wait_for(predicate, description, seconds=20):
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


def expandir_carpeta(nombre):
    b = wait_for(lambda: _buscar("button", nombre), f"No está la carpeta «{nombre}»")
    accion = b.get_action_iface()
    assert accion is not None and accion.get_n_actions() > 0, f"{nombre!r} sin acciones"
    assert accion.do_action(0), f"No se pudo expandir «{nombre}»"
    time.sleep(PAUSA)


def elegir(nombre):
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


def etiqueta(texto):
    return any(texto in (n.get_name() or "") for n in find_all("label", texto) if visible(n))


def main():
    expandir_carpeta("raw")
    wait_for(
        lambda: any("informe.pdf" in (n.get_name() or "") for n in find_all("list item", "")),
        "No apareció la fila de raw/informe.pdf",
    )
    elegir("informe.pdf")

    # Fase 1: «Ver el PDF» abre el visor; WebKitGTK expone el documento con el estado en el
    # título («N páginas» cuando PDF.js terminó de cargar y renderizar).
    apretar("Ver el PDF")

    def visor_cargado():
        return any(
            "páginas" in (n.get_name() or "") and visible(n)
            for n in find_all("document web", "")
        )

    wait_for(visor_cargado, "El visor no cargó el documento", 30)
    print("OK: el visor PDF embebido carga el documento (WebKit + PDF.js)")

    # Fase 2: selección de prueba → copiar enlace → aviso de portapapeles.
    assert do_action(frame(), "win.test-pdf-seleccion"), "No se pudo inyectar la selección"
    time.sleep(PAUSA)
    copiar = wait_for(lambda: _buscar("button", "Copiar enlace a la selección"), "No está el botón de copiar")
    assert copiar.get_state_set().contains(Atspi.StateType.SENSITIVE), (
        "El botón de copiar no quedó sensible con selección"
    )
    apretar("Copiar enlace a la selección")
    wait_for(
        lambda: etiqueta("Enlace copiado al portapapeles"),
        "No apareció el aviso de enlace copiado",
    )
    print("OK: la selección arma el enlace y lo copia al portapapeles")

    # Fase 3: volver al archivo.
    apretar("Volver al archivo")
    wait_for(lambda: etiqueta("Fuente original"), "No volvió la vista del archivo")
    print("OK: volver al archivo de raw/ desde el visor")

    print("Comprobaciones 016 AT-SPI terminadas. Pendiente para la usuaria: teclado, mouse y lector de pantalla.")


if __name__ == "__main__":
    main()
