#!/usr/bin/env python3
"""Validación AT-SPI de la User Story 4 (especificación 004).

Escenarios:
- 4.1: Filtros de lugar (`tag:`, `path:`, `file:`) combinados.
- 4.2: Coincidencias en la misma línea (`line:(a b)`) o misma sección (`section:(a b)`).
- 4.3: Propiedades en bloque y campos inline (`[clave:valor]`, `[clave]`), el bloque manda.
- 4.4: Errores sintácticos (paréntesis sin cerrar, operador desconocido) muestran aviso sin borrar texto; botón de ayuda «?».
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


def get_entry_text(node):
    t = node.get_text_iface()
    if t:
        count = Atspi.Text.get_character_count(t)
        return Atspi.Text.get_text(t, 0, count)
    return node.get_name() or ""


def switch_to_search():
    tab = find_one("page tab", "Búsqueda", timeout=5.0)
    if tab:
        do_action(tab)
        time.sleep(0.4)
    entry = find_one("entry", "Campo de búsqueda", timeout=5.0)
    if not entry:
        entry = find_one("text", "Campo de búsqueda", timeout=5.0)
    return entry


def get_search_results_text():
    list_view = find_one("list", "Resultados de búsqueda", timeout=3.0)
    if not list_view:
        return []
    items = []
    for i in range(list_view.get_child_count()):
        c = list_view.get_child_at_index(i)
        if c:
            names = []
            for node, _ in walk(c):
                nm = node.get_name() or ""
                if nm:
                    names.append(nm)
            items.append(" | ".join(names))
    return items


def wait_for_results(expected_notes, forbidden_notes=None, timeout=4.0):
    deadline = time.time() + timeout
    forbidden = forbidden_notes or []
    last_res = []
    while time.time() < deadline:
        res = get_search_results_text()
        last_res = res
        all_exp = all(any(exp in r for r in res) for exp in expected_notes)
        any_forb = any(any(f in r for r in res) for f in forbidden)
        if all_exp and not any_forb and (not expected_notes or len(res) > 0):
            return True, res
        time.sleep(0.15)
    return False, last_res


def main():
    print("=== Iniciando validación AT-SPI de US4 (Buscar por lugar y propiedades) ===")
    app = find_one("frame", "Orbital", timeout=12.0)
    if not app:
        sys.exit("ERROR: No se encontró la ventana de Orbital")

    search_entry = switch_to_search()
    if not search_entry:
        sys.exit("ERROR: No se encontró el campo de búsqueda")
    time.sleep(0.5)

    # =========================================================================
    # ESCENARIO 4.1: tag:, path:, file:
    # =========================================================================
    print("\n--- Probando Escenario 4.1: Filtros de lugar (tag:, path:, file:) ---")

    # 1. tag:#fisica debe incluir fisica/termodinamica (entropia.md)
    print("Buscando 'tag:#fisica'...")
    set_text(search_entry, "tag:#fisica")
    ok, res = wait_for_results(["entropia"], ["reacciones", "celula"])
    assert ok, f"tag:#fisica debe encontrar solo entropia.md, se obtuvo: {res}"
    print("[OK] tag:#fisica verificado.")

    # 2. path:quimica debe encontrar reacciones.html
    print("Buscando 'path:quimica'...")
    set_text(search_entry, "path:quimica")
    ok, res = wait_for_results(["reacciones"], ["entropia", "celula"])
    assert ok, f"path:quimica debe encontrar solo reacciones.html, se obtuvo: {res}"
    print("[OK] path:quimica verificado.")

    # 3. file:celula debe encontrar celula.md
    print("Buscando 'file:celula'...")
    set_text(search_entry, "file:celula")
    ok, res = wait_for_results(["celula"], ["entropia", "reacciones"])
    assert ok, f"file:celula debe encontrar solo celula.md, se obtuvo: {res}"
    print("[OK] file:celula verificado.")

    # 4. Combinación: tag:#fisica path:fisica file:entropia
    print("Buscando 'tag:#fisica path:fisica file:entropia'...")
    set_text(search_entry, "tag:#fisica path:fisica file:entropia")
    ok, res = wait_for_results(["entropia"], ["reacciones", "celula"])
    assert ok, f"Filtros combinados deben encontrar entropia.md, se obtuvo: {res}"
    print("[OK] Filtros combinados verificados.")

    # =========================================================================
    # ESCENARIO 4.2: line:(a b) y section:(a b)
    # =========================================================================
    print("\n--- Probando Escenario 4.2: line:(a b) y section:(a b) ---")

    # 1. line:(calor energía) -> están en la misma línea en entropia.md
    print("Buscando 'line:(calor energía)'...")
    set_text(search_entry, "line:(calor energía)")
    ok, res = wait_for_results(["entropia"], ["reacciones", "celula"])
    assert ok, f"line:(calor energía) debe encontrar entropia.md, se obtuvo: {res}"
    print("[OK] line:(calor energía) encontró coincidencia en misma línea.")

    # 2. line:(calor transferencia) -> en sección segunda están en líneas distintas -> no coincide
    print("Buscando 'line:(calor transferencia)' (en líneas distintas)...")
    set_text(search_entry, "line:(calor transferencia)")
    time.sleep(0.5)
    res = get_search_results_text()
    assert not any("entropia" in r for r in res), f"line:(calor transferencia) no debe coincidir si están en líneas distintas, se obtuvo: {res}"
    print("[OK] line:(calor transferencia) no coincide al estar en líneas distintas.")

    # 3. section:(calor transferencia) -> en sección segunda están bajo el mismo encabezado -> coincide
    print("Buscando 'section:(calor transferencia)' (misma sección)...")
    set_text(search_entry, "section:(calor transferencia)")
    ok, res = wait_for_results(["entropia"], ["reacciones", "celula"])
    assert ok, f"section:(calor transferencia) debe encontrar entropia.md, se obtuvo: {res}"
    print("[OK] section:(calor transferencia) verificado con éxito.")

    # =========================================================================
    # ESCENARIO 4.3: [clave] y [clave:valor] (bloque manda sobre inline)
    # =========================================================================
    print("\n--- Probando Escenario 4.3: Propiedades y campos inline ([clave:valor], [clave]) ---")

    # 1. [type:atomo]
    print("Buscando '[type:atomo]'...")
    set_text(search_entry, "[type:atomo]")
    ok, res = wait_for_results(["entropia"], ["reacciones", "celula"])
    assert ok, f"[type:atomo] debe encontrar entropia.md, se obtuvo: {res}"
    print("[OK] [type:atomo] verificado.")

    # 2. [type] (cualquiera con propiedad type: entropia y celula)
    print("Buscando '[type]'...")
    set_text(search_entry, "[type]")
    ok, res = wait_for_results(["entropia", "celula"], ["reacciones"])
    assert ok, f"[type] debe encontrar entropia.md y celula.md, se obtuvo: {res}"
    print("[OK] [type] verificado.")

    # 3. [tipo:enlace] (campo inline en HTML)
    print("Buscando '[tipo:enlace]'...")
    set_text(search_entry, "[tipo:enlace]")
    ok, res = wait_for_results(["reacciones"], ["entropia", "celula"])
    assert ok, f"[tipo:enlace] debe encontrar reacciones.html, se obtuvo: {res}"
    print("[OK] [tipo:enlace] verificado.")

    # 4. [estado:activo]:
    # En entropia.md es inline: 'estado:: activo' (válido).
    # En celula.md es inline: 'estado:: activo', pero frontmatter tiene 'estado: archivado'.
    # El bloque manda, por lo que celula.md NO debe coincidir con 'activo'.
    print("Buscando '[estado:activo]' (el bloque manda sobre inline)...")
    set_text(search_entry, "[estado:activo]")
    ok, res = wait_for_results(["entropia"], ["celula"])
    assert ok, f"[estado:activo] debe encontrar solo entropia.md (bloque en celula manda), se obtuvo: {res}"
    print("[OK] Precedencia de bloque sobre inline verificada.")

    # 5. [estado:archivado]
    print("Buscando '[estado:archivado]'...")
    set_text(search_entry, "[estado:archivado]")
    ok, res = wait_for_results(["celula"], ["entropia", "reacciones"])
    assert ok, f"[estado:archivado] debe encontrar celula.md, se obtuvo: {res}"
    print("[OK] [estado:archivado] verificado.")

    # 6. Test independiente completo de US4:
    # [type:atomo] [estado:activo] calor
    print("Buscando '[type:atomo] [estado:activo] calor'...")
    set_text(search_entry, "[type:atomo] [estado:activo] calor")
    ok, res = wait_for_results(["entropia"], ["reacciones", "celula"])
    assert ok, f"Triple condición debe encontrar únicamente entropia.md, se obtuvo: {res}"
    print("[OK] Test independiente de US4 verificado con éxito.")

    # =========================================================================
    # ESCENARIO 4.4: Aviso de error sin borrar texto y botón de ayuda
    # =========================================================================
    print("\n--- Probando Escenario 4.4: Errores sintácticos y Ayuda con operadores ---")

    # 1. Paréntesis sin cerrar: error_banner revelado y texto conservado
    bad_query_paren = "line:(calor energía"
    print(f"Escribiendo consulta con error sintáctico: '{bad_query_paren}'...")
    set_text(search_entry, bad_query_paren)
    time.sleep(0.5)

    # Comprobar que el texto sigue en el campo
    current_entry_text = get_entry_text(search_entry)
    assert current_entry_text == bad_query_paren, f"El campo no debe borrar el texto ante error, tiene: '{current_entry_text}'"

    # Comprobar aviso de error
    banner_label = find_one("*", "Paréntesis sin cerrar", timeout=5.0)
    assert banner_label is not None, "Debe mostrarse el aviso de error con la posición del problema"
    print(f"[OK] Aviso de error mostrado: '{banner_label.get_name()}' sin borrar el texto.")

    # 2. Operador desconocido
    bad_query_op = "operador_invalido:termino"
    print(f"Escribiendo operador desconocido: '{bad_query_op}'...")
    set_text(search_entry, bad_query_op)
    time.sleep(0.8)

    current_entry_text = get_entry_text(search_entry)
    assert current_entry_text == bad_query_op, f"El campo no debe borrar el texto ante operador desconocido, tiene: '{current_entry_text}'"

    banner_label_op = find_one("*", "Operador desconocido", timeout=5.0)
    assert banner_label_op is not None, "Debe mostrarse el aviso de error para operador desconocido"
    print(f"[OK] Aviso de operador desconocido mostrado: '{banner_label_op.get_name()}'.")

    # 3. Botón de ayuda «?» con popover de operadores
    print("Probando botón de ayuda «?»...")
    help_btn = find_one("push button", "?", timeout=3.0)
    if not help_btn:
        help_btn = find_one("button", "operadores", timeout=3.0)
    assert help_btn is not None, "Debe existir el botón de ayuda «?» con operadores"

    do_action(help_btn)
    time.sleep(0.5)

    # Verificar que el popover muestra los operadores
    popover_label = find_one("*", "Operadores de búsqueda", timeout=3.0)
    assert popover_label is not None, "El popover de ayuda debe mostrarse con el título de Operadores"
    print("[OK] Popover de operadores de búsqueda abierto y accesible.")

    print("\n=== ¡VALIDACIÓN US4 COMPLETA CON ÉXITO! ===")
    return 0


if __name__ == "__main__":
    sys.exit(main())
