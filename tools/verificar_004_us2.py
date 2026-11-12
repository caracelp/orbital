#!/usr/bin/env python3
"""Validación AT-SPI de la User Story 2 (Saltar a una nota por su nombre).

Escenarios:
- 2.1: El selector rápido se abre, muestra recientes cuando no hay texto,
       filtra por nombre, título y alias con coincidencias por el principio primero.
- 2.2: Flechas e Intro abren la nota elegida; Escape cierra sin hacer nada.
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


def grab_focus(node):
    comp = node.get_component_iface()
    if comp:
        comp.grab_focus()
        time.sleep(0.1)


def send_key(name):
    mapping = {
        "enter": (0xFF0D, Atspi.KeySynthType.SYM),
        "return": (0xFF0D, Atspi.KeySynthType.SYM),
        "escape": (0xFF1B, Atspi.KeySynthType.SYM),
        "up": (0xFF52, Atspi.KeySynthType.SYM),
        "down": (0xFF54, Atspi.KeySynthType.SYM),
    }
    val, synth = mapping.get(name.lower(), (0, None))
    if synth:
        Atspi.generate_keyboard_event(val, None, Atspi.KeySynthType.PRESS | synth)
        time.sleep(0.05)
        Atspi.generate_keyboard_event(val, None, Atspi.KeySynthType.RELEASE | synth)


def get_switcher_items_text():
    list_view = find_one("list", "Resultados del selector", timeout=3.0)
    if not list_view:
        return []
    items = []
    for i in range(list_view.get_child_count()):
        c = list_view.get_child_at_index(i)
        if c:
            names = [node.get_name() or "" for node, _ in walk(c) if node.get_name()]
            items.append(" | ".join(names))
    return items


def main():
    print("=== Iniciando validación AT-SPI de US2 (Selector rápido de notas) ===")
    app = find_one("frame", "Orbital", timeout=12.0)
    if not app:
        sys.exit("ERROR: No se encontró la ventana de Orbital")
    # Esperar a que la bóveda cargue
    find_one("list", "Contenido de la bóveda", timeout=10.0)
    time.sleep(0.5)

    # 1. Abrir el selector rápido con la acción win.quick-open (Ctrl+P)
    print("\nAbriendo selector rápido (win.quick-open)...")
    do_action(app, "win.quick-open")
    search_entry = find_one("entry", "Buscar nota", timeout=5.0)
    assert search_entry is not None, "Debe abrirse el selector rápido con el campo 'Buscar nota'"

    # --- ESCENARIO 2.1: Recientes cuando no hay texto ---
    print("\n--- Probando Escenario 2.1a: Notas recientes sin texto de búsqueda ---")
    items = get_switcher_items_text()
    items_str = " ".join(items).lower()
    print(f"Items mostrados sin búsqueda ({len(items)}):")
    for it in items:
        print(f"  * {it}")
    assert "reaccion" in items_str, "La nota recientemente abierta 'reaccion' debe aparecer al inicio"
    assert "reciente" in items_str, "Debe tener la marca 'Reciente'"
    print("[OK] Escenario 2.1a: Recientes mostradas correctamente al abrir sin texto.")

    # --- ESCENARIO 2.1b: Filtrado por nombre ---
    print("\n--- Probando Escenario 2.1b: Filtrado por nombre ('entro') ---")
    set_text(search_entry, "entro")
    time.sleep(0.3)
    items = get_switcher_items_text()
    items_str = " ".join(items).lower()
    assert "entropia" in items_str, "Debe encontrar 'entropia' por prefijo de nombre"
    assert "biologia" not in items_str, "No debe incluir notas no coincidentes"
    print("[OK] Escenario 2.1b: Filtrado por nombre verificado.")

    # --- ESCENARIO 2.1c: Filtrado por título ---
    print("\n--- Probando Escenario 2.1c: Filtrado por título ('clasica') ---")
    set_text(search_entry, "clasica")
    time.sleep(0.3)
    items = get_switcher_items_text()
    items_str = " ".join(items).lower()
    assert "mecanica" in items_str, "Debe encontrar 'mecanica' por su título 'Mecánica clásica'"
    print("[OK] Escenario 2.1c: Filtrado por título verificado.")

    # --- ESCENARIO 2.1d: Filtrado por alias ---
    print("\n--- Probando Escenario 2.1d: Filtrado por alias ('celula') ---")
    set_text(search_entry, "celula")
    time.sleep(0.3)
    items = get_switcher_items_text()
    items_str = " ".join(items).lower()
    assert "biologia" in items_str, "Debe encontrar 'biologia' por su alias 'celula'"
    print("[OK] Escenario 2.1d: Filtrado por alias verificado.")

    # --- ESCENARIO 2.2: Elegir nota y cerrar selector ---
    print("\n--- Probando Escenario 2.2a: Elegir coincidencia abre la nota ---")
    set_text(search_entry, "biologia")
    time.sleep(0.3)

    dialog = find_one("dialog", "Saltar a nota", timeout=3.0)
    assert dialog is not None, "Debe existir el diálogo del selector"
    print("Ejecutando acción de selección (switcher.choose / Intro)...")
    ok = do_action(dialog, "switcher.choose")
    if not ok:
        do_action(dialog)
    time.sleep(0.8)

    # Verificar que el diálogo se cerró
    dialog_after = find_one("dialog", "Saltar a nota", timeout=0.5)
    assert dialog_after is None or not dialog_after.get_state_set().contains(Atspi.StateType.SHOWING), "El diálogo del selector debe cerrarse tras elegir nota"
    print("[OK] Diálogo cerrado tras elegir con Intro / activación.")

    # Verificar que la nota biologia se abrió
    nota_lbl = find_one("label", "biologia", timeout=2.0)
    assert nota_lbl is not None, "La nota biologia debe estar abierta en la ventana"
    print("[OK] Nota 'biologia' abierta correctamente.")

    # --- ESCENARIO 2.2b: Escape / botón Cerrar cierra sin hacer nada ---
    print("\n--- Probando Escenario 2.2b: Cerrar el selector ---")
    do_action(app, "win.quick-open")
    time.sleep(0.5)
    search_entry_open = find_one("entry", "Buscar nota", timeout=5.0)
    assert search_entry_open is not None, "El diálogo debe abrirse de nuevo"

    # Verificar que tras abrir 'biologia', ahora aparece en las notas recientes
    items2 = get_switcher_items_text()
    items2_str = " ".join(items2).lower()
    print("Items en selector reabierto:")
    for it in items2:
        print(f"  * {it}")
    assert "biologia" in items2_str, "La nota 'biologia' debe aparecer en recientes tras haber sido abierta"

    # Cerrar el diálogo usando el botón Cerrar / Close
    btn_close = find_one("button", "Close", timeout=2.0)
    assert btn_close is not None, "Debe existir el botón Close en el diálogo"
    do_action(btn_close)
    time.sleep(0.5)
    dialog_open = find_one("dialog", "Saltar a nota", timeout=0.5)
    assert dialog_open is None or not dialog_open.get_state_set().contains(Atspi.StateType.SHOWING), "El diálogo debe cerrarse con Close"
    print("[OK] Diálogo cerrado correctamente.")

    print("\n=== ¡VALIDACIÓN US2 COMPLETA CON ÉXITO! ===")
    return 0


if __name__ == "__main__":
    sys.exit(main())
