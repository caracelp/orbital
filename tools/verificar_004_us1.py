#!/usr/bin/env python3
"""Validación AT-SPI de la User Story 1 (especificación 004).

Escenarios:
- 1.1: multi-palabra sin tildes ni mayúsculas (`fisica energia`), resultados aparecen mientras se escribe.
- 1.2: frase exacta `"..."`, `-excluir`, `a OR b`.
- 1.3: elegir coincidencia abre la nota con la línea a la vista.
- 1.4: nota HTML encuentra texto del cuerpo, ignora etiquetas.
- 1.5: texto en `raw/` marcado con badge 'fuente', checkbox oculta/muestra.
"""

import os
import sys
import time
import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi


def get_app():
    pid = os.environ.get("ORBITAL_PID")
    if not pid:
        sys.exit("Falta ORBITAL_PID")
    desktop = Atspi.get_desktop(0)
    for i in range(desktop.get_child_count()):
        a = desktop.get_child_at_index(i)
        if (
            a
            and (a.get_name() or "").lower() in ("orbital", "io.github.caracelp.orbital")
            and str(a.get_process_id()) == pid
        ):
            return a
    return None


def walk(node, depth=0):
    yield node, depth
    try:
        n = node.get_child_count()
    except Exception:
        return
    for i in range(n):
        c = node.get_child_at_index(i)
        if c is not None:
            yield from walk(c, depth + 1)


def find_all(role, name_substr=""):
    a = get_app()
    if a is None:
        return []
    matches = []
    for node, _ in walk(a):
        try:
            r = node.get_role_name()
            nm = node.get_name() or ""
        except Exception:
            continue
        role_hit = role == "*" or r == role or (role == "button" and "button" in r)
        name_hit = not name_substr or name_substr.lower() in nm.lower()
        if role_hit and name_hit:
            matches.append(node)
    return matches


def find_one(role, name_substr="", timeout=10.0):
    deadline = time.time() + timeout
    while time.time() < deadline:
        matches = find_all(role, name_substr)
        if matches:
            return matches[0]
        time.sleep(0.15)
    return None


def do_action(node, action_name=None):
    act = node.get_action_iface()
    if not act or act.get_n_actions() == 0:
        return False
    idx = 0
    if action_name:
        for i in range(act.get_n_actions()):
            if act.get_action_name(i).lower() == action_name.lower():
                idx = i
                break
    return act.do_action(idx)


def set_text(node, text):
    ed = node.get_editable_text_iface()
    if not ed:
        return False
    ed.set_text_contents(text)
    return True


def switch_to_search():
    tab = find_one("toggle button", "Búsqueda", timeout=5.0)
    if tab:
        do_action(tab)
        time.sleep(0.4)
    entry = find_one("entry", "Campo de búsqueda", timeout=5.0)
    if not entry:
        entry = find_one("text", "Campo de búsqueda", timeout=5.0)
    return entry


def get_search_results_text():
    """Nombres de todas las filas de la lista de resultados. La lista es virtual (solo existen
    las filas visibles): se recorre bajando con la acción «scroll-to» de la última fila visible
    y se vuelve arriba al terminar."""
    list_view = find_one("list", "Resultados de búsqueda", timeout=3.0)
    if not list_view:
        return []

    def visibles():
        out = []
        for i in range(list_view.get_child_count()):
            c = list_view.get_child_at_index(i)
            if c:
                names = [node.get_name() for node, _ in walk(c) if node.get_name()]
                out.append(" | ".join(names))
        return out

    def scroll_to(index):
        c = list_view.get_child_at_index(index)
        act = c.get_action_iface() if c else None
        if act and act.get_n_actions():
            act.do_action(0)
            time.sleep(0.12)

    items = []
    for _ in range(30):
        filas = visibles()
        for f in filas:
            if f not in items:
                items.append(f)
        if not filas:
            break
        antes = filas[-1]
        scroll_to(len(filas) - 1)
        if visibles()[-1:] == [antes]:
            break
    for _ in range(30):
        filas = visibles()
        if not filas:
            break
        antes = filas[0]
        scroll_to(0)
        if visibles()[:1] == [antes]:
            break
    return items


def main():
    print("=== Iniciando validación AT-SPI de US1 (Búsqueda en toda la bóveda) ===")
    app = find_one("frame", "Orbital", timeout=12.0)
    if not app:
        sys.exit("ERROR: No se encontró la ventana de Orbital")

    # 1. Cambiar a la vista de Búsqueda
    search_entry = switch_to_search()
    if not search_entry:
        sys.exit("ERROR: No se encontró el campo de búsqueda")

    # --- ESCENARIO 1.1: Multi-palabra sin tildes ni mayúsculas ---
    print("\n--- Probando Escenario 1.1: 'fisica energia' ---")
    set_text(search_entry, "fisica energia")
    time.sleep(0.4)
    results = get_search_results_text()
    results_str = " ".join(results).lower()
    print(f"Resultados encontrados ({len(results)} filas):")
    for r in results:
        print(f"  * {r}")

    assert "entropia" in results_str, "Debe contener entropia"
    assert "mecanica" in results_str, "Debe contener mecanica"
    assert "termodinamica" in results_str, "Debe contener termodinamica"
    assert "reaccion" not in results_str, "No debe contener reaccion (no tiene 'fisica')"
    print("[OK] Escenario 1.1: Búsqueda multi-palabra sin tildes ni mayúsculas verificada.")

    # --- ESCENARIO 1.2: Frase exacta, -excluir, OR ---
    print("\n--- Probando Escenario 1.2a: Frase exacta '\"conservacion de la masa\"' ---")
    set_text(search_entry, '"conservacion de la masa"')
    time.sleep(0.4)
    results = get_search_results_text()
    results_str = " ".join(results).lower()
    assert "entropia" in results_str, "Debe contener entropia"
    assert "mecanica" not in results_str, "No debe contener mecanica"
    print("[OK] Escenario 1.2a: Frase exacta verificada.")

    print("\n--- Probando Escenario 1.2b: Negación 'fisica -cuantica' ---")
    set_text(search_entry, "fisica -cuantica")
    time.sleep(0.4)
    results = get_search_results_text()
    results_str = " ".join(results).lower()
    assert "mecanica" in results_str, "Debe contener mecanica"
    assert "entropia" not in results_str, "Entropía tiene 'cuántica', debe ser excluida"
    print("[OK] Escenario 1.2b: Negación (-término) verificada.")

    print("\n--- Probando Escenario 1.2c: Alternativa 'cuantica OR quimica' ---")
    set_text(search_entry, "cuantica OR quimica")
    time.sleep(0.4)
    results = get_search_results_text()
    results_str = " ".join(results).lower()
    assert "entropia" in results_str, "Debe contener entropia (cuántica)"
    assert "reaccion" in results_str, "Debe contener reaccion (química)"
    print("[OK] Escenario 1.2c: Alternativa OR verificada.")

    # --- ESCENARIO 1.3: Elegir coincidencia abre nota en esa línea ---
    print("\n--- Probando Escenario 1.3: Abrir coincidencia ---")
    set_text(search_entry, "conservacion de la masa")
    time.sleep(0.4)
    list_view = find_one("list", "Resultados de búsqueda", timeout=3.0)
    assert list_view is not None, "Debe encontrarse la lista de resultados"
    match_row = None
    for i in range(list_view.get_child_count()):
        c = list_view.get_child_at_index(i)
        if c:
            text = " ".join(node.get_name() or "" for node, _ in walk(c))
            if "conservacion" in text.lower():
                match_row = c
                break
    assert match_row is not None, "Debe existir una fila de coincidencia de línea"
    # GTK 4.24 solo expone `listitem.scroll-to` en las filas de un GtkListView: AT-SPI no puede
    # activarlas (antes este escenario «pasaba» sin abrir nada). La activación real (clic o Intro)
    # y la línea a la vista los cubre la prueba GTK en proceso `us1_open_match_same_note`:
    #   cargo test --bin orbital search_open_tests -- --ignored
    print(f"Fila de coincidencia presente: {match_row.get_role_name()}")
    print("[LÍMITE] Escenario 1.3: la activación de la fila no es accionable por AT-SPI; "
          "la cubre la prueba GTK us1_open_match_same_note.")

    # --- ESCENARIO 1.4: Nota HTML busca texto del cuerpo e ignora etiquetas ---
    print("\n--- Probando Escenario 1.4: Búsqueda en notas HTML ---")
    search_entry = switch_to_search()
    assert search_entry is not None, "Debe encontrarse el campo de búsqueda"

    # Buscar texto visible del cuerpo
    set_text(search_entry, "sistemas cerrados")
    time.sleep(0.4)
    results = get_search_results_text()
    results_str = " ".join(results).lower()
    assert "termodinamica" in results_str, "Debe encontrar 'termodinamica' por texto del cuerpo HTML"

    # Buscar atributo/clase interna de HTML
    set_text(search_entry, "span-etiqueta-ignorar")
    time.sleep(0.4)
    results = get_search_results_text()
    assert len(results) == 0, f"No debe coincidir con etiquetas o atributos HTML, pero obtuvo: {results}"
    empty_status = find_one("label", "Sin resultados")
    assert empty_status is not None, "Debe mostrar 'Sin resultados' al buscar atributo HTML"
    print("[OK] Escenario 1.4: HTML busca texto visible del cuerpo e ignora etiquetas y atributos.")

    # --- ESCENARIO 1.5: raw/ con badge 'fuente' y filtro para ocultar/mostrar ---
    print("\n--- Probando Escenario 1.5: Filtro y fuentes raw/ ---")
    set_text(search_entry, "laboratorio")
    time.sleep(0.4)
    results = get_search_results_text()
    assert len(results) == 0, f"Por defecto raw/ está desactivado, no debe mostrar apuntes-fisica. Resultados: {results}"

    raw_check = find_one("check box", "Incluir fuentes (raw/)", timeout=3.0)
    assert raw_check is not None, "Debe existir el checkbox 'Incluir fuentes (raw/)'"

    # Activar checkbox
    print("Activando filtro 'Incluir fuentes (raw/)'...")
    do_action(app, "win.toggle-search-raw")
    time.sleep(0.4)
    results = get_search_results_text()
    results_str = " ".join(results).lower()
    assert "apuntes-fisica" in results_str, f"Al activar raw/, debe aparecer apuntes-fisica. Resultados: {results}"
    assert "raw" in results_str or "fuente" in results_str, "Debe contener indicador de fuente/raw"

    # Desactivar checkbox
    print("Desactivando filtro 'Incluir fuentes (raw/)'...")
    do_action(app, "win.toggle-search-raw")
    time.sleep(0.4)
    results = get_search_results_text()
    assert len(results) == 0, f"Al desmarcar raw/, apuntes-fisica debe ocultarse. Resultados: {results}"

    # Reactivar checkbox
    print("Reactivando filtro 'Incluir fuentes (raw/)'...")
    do_action(app, "win.toggle-search-raw")
    time.sleep(0.4)
    results = get_search_results_text()
    results_str = " ".join(results).lower()
    assert "apuntes-fisica" in results_str, "Al volver a marcar raw/, apuntes-fisica debe reaparecer"
    print("[OK] Escenario 1.5: Fuentes raw/ marcadas con 'fuente' y filtro oculta/muestra correctamente.")

    print("\n=== ¡VALIDACIÓN US1 COMPLETA CON ÉXITO! ===")
    return 0


if __name__ == "__main__":
    sys.exit(main())
