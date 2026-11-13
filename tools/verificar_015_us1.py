#!/usr/bin/env python3
"""Comprobaciones AT-SPI US1/US2 de la importación (015); solo el PID y la bóveda del lanzador.

No genera teclas ni usa diálogos (con libadwaita 1.10 el contenido de un AdwDialog deja de
exponerse por AT-SPI): el archivo de raw/ se abre desde la lista de la bóveda y la importación
se acciona con el botón «Importar a Markdown» de la vista externa o con la acción de ventana.
El contenido de la nota creada se verifica contra el DISCO (leer el texto del editor por AT-SPI
con sondeos tumba el bridge de GTK 4.24; la conversión la cubren los tests del núcleo).

Fases:
  importa  abre raw/articulo.html desde la lista, importa con el botón y verifica la nota nueva
           en disco (frontmatter con sources, cuerpo convertido) y que quedó abierta.
  repite   la segunda importación va a articulo-2.md sin pisar la primera (acción de ventana).
  no-soportado  un .png en raw/ no se importa (aviso discreto, sin nota nueva).
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
assert os.environ.get("ORBITAL_TEST_VAULT", "").count("orbital-015-us1-") == 1, (
    "Solo acepta la bóveda temporal del lanzador"
)
BOVEDA = os.environ["ORBITAL_TEST_VAULT"]

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


def _buscar(rol, nombre):
    for b in find_all(rol, ""):
        if not visible(b):
            continue
        nombre_atr = b.get_name() or ""
        if nombre_atr == nombre or nombre in nombre_atr:
            return b
    return None


def apretar(nombre):
    fin = time.monotonic() + 5
    b = None
    while b is None:
        b = _buscar("button", nombre)
        if b is not None or time.monotonic() >= fin:
            break
        time.sleep(SONDEO)
    if b is None:
        raise AssertionError(
            f"No está el botón «{nombre}»; en el árbol hay: "
            f"{[(x.get_name() or '', visible(x)) for x in find_all('button', '')][:20]}"
        )
    assert do_action(b), f"No se pudo accionar «{nombre}»"
    time.sleep(PAUSA)


def expandir_carpeta(nombre):
    b = wait_for(lambda: _buscar("button", nombre), f"No está la carpeta «{nombre}»")
    accion = b.get_action_iface()
    assert accion is not None and accion.get_n_actions() > 0, f"{nombre!r} sin acciones"
    assert accion.do_action(0), f"No se pudo expandir «{nombre}»"
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


def verificar_nota(nombre, cuerpo):
    texto = open(os.path.join(BOVEDA, nombre), encoding="utf-8").read()
    assert texto.startswith("---\n"), f"«{nombre}» no tiene frontmatter: {texto[:80]!r}"
    assert re.search(r"resource: /raw/", texto), (
        f"Sin la fuente en sources: {texto[:200]!r}"
    )
    assert "type: atomo" in texto, f"Sin type: {texto[:200]!r}"
    assert cuerpo in texto, f"Sin el cuerpo convertido: {texto[:300]!r}"


def main():
    expandir_carpeta("raw")
    wait_for(
        lambda: any("articulo.html" in (n.get_name() or "") for n in find_all("list item", "")),
        "No apareció la fila de raw/articulo.html",
    )

    # Fase 1: HTML de raw/ (editor de solo lectura) importado con la acción de ventana.
    elegir("articulo.html")
    wait_for(lambda: etiqueta("solo lectura"), "El HTML no abrió como fuente de solo lectura")
    assert do_action(frame(), "win.importar-md"), "No se pudo accionar win.importar-md"
    wait_for(
        lambda: os.path.exists(os.path.join(BOVEDA, "wiki/articulo.md")),
        "La nota wiki/articulo.md no se creó",
    )
    wait_for(lambda: nota_abierta(), "No volvió la lista de notas tras crear la nota")
    wait_for(lambda: etiqueta("articulo.md"), "La nota creada no se abrió en el editor")
    verificar_nota("wiki/articulo.md", "# Artículo importado")
    print("OK: el HTML de raw/ se importa a una nota con sources y queda abierta")

    # Fase 2: la segunda importación (misma fuente) va a -2 sin pisar.
    elegir("articulo.html")
    assert do_action(frame(), "win.importar-md"), "No se pudo accionar win.importar-md"
    wait_for(
        lambda: os.path.exists(os.path.join(BOVEDA, "wiki/articulo-2.md")),
        "La segunda importación no creó articulo-2.md",
    )
    verificar_nota("wiki/articulo-2.md", "# Artículo importado")
    print("OK: la segunda importación va a -2 sin pisar la primera")

    # Fase 3: DOCX de raw/ (vista externa) importado con el botón.
    elegir("informe.docx")
    apretar("Importar a Markdown")
    wait_for(
        lambda: os.path.exists(os.path.join(BOVEDA, "wiki/informe.md")),
        "La nota wiki/informe.md no se creó",
    )
    wait_for(lambda: etiqueta("informe.md"), "La nota del DOCX no se abrió")
    verificar_nota("wiki/informe.md", "Informe anual")
    print("OK: el botón «Importar a Markdown» importa el DOCX de la vista externa")

    # Fase 4: un .png no se importa (el botón no se ofrece; la acción no crea nada).
    elegir("imagen.png")
    time.sleep(1.5)
    assert _buscar("button", "Importar a Markdown") is None, (
        "El botón de importar se ofrece para un .png"
    )
    assert do_action(frame(), "win.importar-md"), "No se pudo accionar win.importar-md"
    time.sleep(1.5)
    assert not os.path.exists(os.path.join(BOVEDA, "wiki/imagen.md")), "Importó un .png"
    print("OK: los formatos no soportados no se importan")

    print("Comprobaciones 015 AT-SPI terminadas. Pendiente para la usuaria: teclado y lector de pantalla.")


if __name__ == "__main__":
    main()
