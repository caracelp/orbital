#!/usr/bin/env python3
"""Script de verificación AT-SPI para US3 (Buscar y reemplazar en la nota abierta).

Escenarios verificados:
- 3.1: Búsqueda en Código con contador de coincidencias («1 de 3»), navegación con siguiente y anterior, y ciclo completo.
- 3.2: Reemplazo de una coincidencia y «Reemplazar todo», contador actualizado a 0 de 0, y recuperación completa con «Deshacer» (Ctrl+Z).
- 3.3: Búsqueda en vista Lectura y Visual (HTML) sin opciones de reemplazo disponibles.
"""

import os
import re
import sys
import time
import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi


def get_orbital():
    pid = os.environ.get("ORBITAL_PID")
    desktop = Atspi.get_desktop(0)
    for i in range(desktop.get_child_count()):
        a = desktop.get_child_at_index(i)
        if (
            a
            and (a.get_name() or "").lower() in ("orbital", "io.github.caracelp.orbital")
            and (not pid or str(a.get_process_id()) == pid)
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
    root = get_orbital()
    if not root:
        return []
    matches = []
    for node, _ in walk(root):
        try:
            r = node.get_role_name() or ""
            nm = node.get_name() or ""
        except Exception:
            continue
        role_hit = (
            role == "*"
            or not role
            or r.lower() == role.lower()
            or ("button" in role.lower() and "button" in r.lower())
            or (role.lower() in ("entry", "text") and r.lower() in ("entry", "text"))
        )
        name_hit = not name_substr or name_substr.lower() in nm.lower()
        if role_hit and name_hit:
            matches.append(node)
    return matches


def find_one(role, name_substr="", timeout=5.0):
    start = time.time()
    while time.time() - start < timeout:
        matches = find_all(role, name_substr)
        if matches:
            return matches[0]
        time.sleep(0.1)
    return None


def do_action(node, action_name=None):
    act = node.get_action_iface()
    if not act or act.get_n_actions() == 0:
        return False
    idx = 0
    if action_name:
        found = False
        for i in range(act.get_n_actions()):
            if act.get_action_name(i).lower() == action_name.lower():
                idx = i
                found = True
                break
        if not found:
            return False
    return act.do_action(idx)


def set_text(node, text):
    ed = node.get_editable_text_iface()
    if not ed:
        return False
    ed.set_text_contents(text)
    return True


def get_counter_text():
    for node in find_all("label"):
        try:
            name = node.get_name() or ""
            if re.search(r"\d+\s+de\s+\d+", name):
                return name.strip()
        except Exception:
            pass
    return ""


def wait_for_counter(expected, timeout=4.0):
    start = time.time()
    last = ""
    while time.time() - start < timeout:
        last = get_counter_text()
        if last == expected:
            return True, last
        time.sleep(0.1)
    return False, last


def wait_for_counter_total(expected_total, timeout=4.0):
    start = time.time()
    last = ""
    target_suffix = f"de {expected_total}"
    while time.time() - start < timeout:
        last = get_counter_text()
        if last.endswith(target_suffix):
            return True, last
        time.sleep(0.1)
    return False, last


def wait_for_counter_match(timeout=5.0):
    start = time.time()
    last = ""
    while time.time() - start < timeout:
        last = get_counter_text()
        if "de" in last and not last.startswith("0 de"):
            return True, last
        time.sleep(0.1)
    return False, last


def main():
    print("Esperando conexión con la ventana de Orbital...")
    win = find_one("frame", "Orbital", timeout=15.0)
    if not win:
        print("[ERROR] No se pudo encontrar la ventana de Orbital vía AT-SPI.")
        return 1

    print("[OK] Conectado a la ventana de Orbital vía AT-SPI.")
    # Esperar a que la bóveda cargue el árbol
    find_one("list", "Contenido de la bóveda", timeout=10.0)
    time.sleep(1.0)

    # 1. Abrir nota termodinamica.md usando quick-open
    print("\n--- Abriendo nota termodinamica.md ---")
    do_action(win, "win.quick-open")
    time.sleep(0.5)
    switcher_entry = find_one("entry", "Buscar nota", timeout=5.0)
    assert switcher_entry is not None, "Debe abrirse el selector rápido"
    set_text(switcher_entry, "termo")
    time.sleep(0.3)
    dialog = find_one("dialog", "Saltar a nota", timeout=3.0)
    assert dialog is not None, "Debe existir el diálogo del selector"
    do_action(dialog, "switcher.choose")
    time.sleep(0.8)

    # Asegurar vista Código
    do_action(win, "win.view-code")
    time.sleep(0.4)

    # =========================================================================
    # ESCENARIO 3.1: Búsqueda en Código, contador y navegación siguiente/anterior
    # =========================================================================
    print("\n=== Probando Escenario 3.1: Búsqueda y contador en Código ===")
    do_action(win, "win.find")
    time.sleep(0.5)

    search_entry = find_one("entry", "Buscar en la nota", timeout=5.0)
    assert search_entry is not None, "Debe mostrarse el campo 'Buscar en la nota'"
    print("[OK] Campo de búsqueda abierto.")

    # Escribir término de búsqueda con 3 coincidencias
    set_text(search_entry, "calor")
    ok, count = wait_for_counter("1 de 3", timeout=3.0)
    print(f"Contador tras buscar 'calor': '{count}'")
    assert ok, f"El contador debe ser '1 de 3', se obtuvo: '{count}'"
    print("[OK] Coincidencias encontradas y contador inicial: 1 de 3.")

    # Navegar siguiente
    btn_next = find_one("push button", "Coincidencia siguiente", timeout=3.0)
    assert btn_next is not None, "Debe existir el botón 'Coincidencia siguiente'"

    do_action(btn_next)
    ok, count = wait_for_counter("2 de 3", timeout=2.0)
    print(f"Contador tras 'siguiente': '{count}'")
    assert ok, f"El contador debe ser '2 de 3', se obtuvo: '{count}'"

    do_action(btn_next)
    ok, count = wait_for_counter("3 de 3", timeout=2.0)
    print(f"Contador tras segundo 'siguiente': '{count}'")
    assert ok, f"El contador debe ser '3 de 3', se obtuvo: '{count}'"

    # Vuelta completa (wrap around)
    do_action(btn_next)
    ok, count = wait_for_counter("1 de 3", timeout=2.0)
    print(f"Contador tras wrap around al inicio: '{count}'")
    assert ok, f"El contador debe volver a '1 de 3', se obtuvo: '{count}'"

    # Navegar anterior (wrap backward)
    btn_prev = find_one("push button", "Coincidencia anterior", timeout=3.0)
    assert btn_prev is not None, "Debe existir el botón 'Coincidencia anterior'"

    do_action(btn_prev)
    ok, count = wait_for_counter("3 de 3", timeout=2.0)
    print(f"Contador tras 'anterior': '{count}'")
    assert ok, f"El contador debe retroceder a '3 de 3', se obtuvo: '{count}'"
    print("[OK] Escenario 3.1 verificado con éxito.")

    # =========================================================================
    # ESCENARIO 3.2: Reemplazo individual, Reemplazar todo y Deshacer (Ctrl+Z)
    # =========================================================================
    print("\n=== Probando Escenario 3.2: Reemplazar y Deshacer ===")
    do_action(win, "win.replace")
    time.sleep(0.5)

    replace_entry = find_one("entry", "Reemplazar con", timeout=5.0)
    assert replace_entry is not None, "Debe mostrarse el campo 'Reemplazar con'"
    set_text(replace_entry, "energía")
    time.sleep(0.2)

    btn_replace = find_one("push button", "Reemplazar", timeout=3.0)
    assert btn_replace is not None, "Debe existir el botón 'Reemplazar'"

    # Reemplazar una coincidencia
    print("Ejecutando 'Reemplazar' (coincidencia individual)...")
    do_action(btn_replace)
    ok, count = wait_for_counter("1 de 2", timeout=3.0)
    print(f"Contador tras reemplazar 1: '{count}'")
    assert ok, f"El contador debe ser '1 de 2' tras reemplazar uno, se obtuvo: '{count}'"
    print("[OK] Reemplazo individual verificado.")

    # Reemplazar todas las coincidencias restantes
    btn_replace_all = find_one("push button", "Reemplazar todo", timeout=3.0)
    assert btn_replace_all is not None, "Debe existir el botón 'Reemplazar todo'"

    print("Ejecutando 'Reemplazar todo'...")
    do_action(btn_replace_all)
    ok, count = wait_for_counter("0 de 0", timeout=3.0)
    print(f"Contador tras 'Reemplazar todo': '{count}'")
    assert ok, f"El contador debe ser '0 de 0' tras reemplazar todo, se obtuvo: '{count}'"
    print("[OK] Reemplazar todo verificado.")

    # Deshacer con Ctrl+Z / win.undo: un solo paso debe restaurar las reemplazadas
    print("Ejecutando Deshacer (win.undo / Ctrl+Z)...")
    do_action(win, "win.undo")
    ok, count = wait_for_counter_total(2, timeout=3.0)
    print(f"Contador tras primer Deshacer: '{count}'")
    assert ok, f"El contador debe terminar en 'de 2' tras Deshacer, se obtuvo: '{count}'"

    # Deshacer el primer reemplazo individual
    do_action(win, "win.undo")
    ok, count = wait_for_counter_total(3, timeout=3.0)
    print(f"Contador tras segundo Deshacer: '{count}'")
    assert ok, f"El texto original debe quedar 100% restaurado (terminar en 'de 3'), se obtuvo: '{count}'"
    print("[OK] Escenario 3.2 verificado con éxito (deshacer en un solo paso).")

    # =========================================================================
    # ESCENARIO 3.3: Búsqueda en Lectura y Visual (sin reemplazo)
    # =========================================================================
    print("\n=== Probando Escenario 3.3: Búsqueda en Lectura y Visual ===")
    # Cambiar a vista Lectura
    do_action(win, "win.view-read")
    time.sleep(0.8)

    # Verificar que el botón y la fila de reemplazo NO están visibles
    btn_rep_check = find_one("push button", "Reemplazar todo", timeout=0.5)
    btn_showing = btn_rep_check and btn_rep_check.get_state_set().contains(
        Atspi.StateType.SHOWING
    )
    assert not btn_showing, "En vista Lectura no deben estar visibles los controles de reemplazo"
    print("[OK] Controles de reemplazo ocultos en Lectura.")

    # La búsqueda se mantiene en la vista formateada
    ok, count = wait_for_counter("1 de 3", timeout=3.0)
    print(f"Contador en vista Lectura: '{count}'")
    assert ok, f"En Lectura el contador debe ser '1 de 3', se obtuvo: '{count}'"

    # Siguiente en Lectura
    do_action(btn_next)
    ok, count = wait_for_counter("2 de 3", timeout=2.0)
    print(f"Contador tras 'siguiente' en Lectura: '{count}'")
    assert ok, f"En Lectura el contador debe ser '2 de 3', se obtuvo: '{count}'"
    print("[OK] Búsqueda y navegación en Lectura verificada.")

    # Abrir nota HTML en Visual
    print("Abriendo nota HTML en vista Visual...")
    do_action(win, "win.quick-open")
    time.sleep(0.5)
    switcher_entry = find_one("entry", "Buscar nota", timeout=5.0)
    assert switcher_entry is not None
    set_text(switcher_entry, "pagina")
    time.sleep(0.3)
    dialog = find_one("dialog", "Saltar a nota", timeout=3.0)
    assert dialog is not None
    do_action(dialog, "switcher.choose")
    time.sleep(1.0)

    # Asegurar que se busca en la nota HTML abierta
    do_action(win, "win.find")
    time.sleep(0.5)
    search_entry = find_one("entry", "Buscar en la nota", timeout=3.0)
    assert search_entry is not None
    set_text(search_entry, "calor")
    time.sleep(0.5)

    # En HTML 'calor' aparece al menos 1 vez
    ok, count_html = wait_for_counter_match(timeout=5.0)
    print(f"Contador en vista Visual (HTML): '{count_html}'")
    assert ok, f"En HTML debe encontrar coincidencias de 'calor', se obtuvo: '{count_html}'"
    print("[OK] Búsqueda en vista Visual (WebKit) verificada.")

    # Cerrar la barra de búsqueda
    btn_close = find_one("push button", "Cerrar búsqueda", timeout=3.0)
    assert btn_close is not None, "Debe existir el botón 'Cerrar búsqueda'"
    do_action(btn_close)
    time.sleep(0.5)

    # Verificar que la barra ya no está en modo de búsqueda
    search_entry_after = find_one("entry", "Buscar en la nota", timeout=0.5)
    bar_showing = search_entry_after and search_entry_after.get_state_set().contains(
        Atspi.StateType.SHOWING
    )
    assert not bar_showing, "La barra de búsqueda debe cerrarse tras hacer clic en 'Cerrar búsqueda'"
    print("[OK] Barra de búsqueda cerrada correctamente.")

    print("\n=== ¡VALIDACIÓN US3 COMPLETA CON ÉXITO! ===")
    return 0


if __name__ == "__main__":
    sys.exit(main())
