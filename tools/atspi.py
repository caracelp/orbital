#!/usr/bin/env python3
"""Maneja Orbital por AT-SPI (la capa de accesibilidad que usa Orca), para probar la interfaz.

Uso: python3 -I tools/atspi.py <comando> [args]
  arbol                      imprime el árbol accesible de la ventana de Orbital
  activar <rol> <nombre>     ejecuta la acción principal del primer elemento con ese rol y nombre
  esperar <rol> <nombre>     espera (hasta 10 s) a que aparezca un elemento
  texto <rol> <nombre>       imprime el texto de un elemento
  escribir <rol> <nombre> <texto>  reemplaza el texto de un campo editable
  elegir <nombre>            elige la fila de la lista de la bóveda cuyo nombre contiene <nombre>

<nombre> es un fragmento del nombre accesible; <rol> es el nombre de rol de AT-SPI
(por ejemplo «push button», «list item», «label») o «*» para cualquiera.
"""
import sys
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402


def app():
    desktop = Atspi.get_desktop(0)
    for i in range(desktop.get_child_count()):
        a = desktop.get_child_at_index(i)
        if a and (a.get_name() or "").lower() in ("orbital", "io.github.caracelp.orbital"):
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


def find(role, name):
    a = app()
    if a is None:
        return None
    for node, _ in walk(a):
        try:
            r = node.get_role_name()
            nm = node.get_name() or ""
        except Exception:
            continue
        if (role == "*" or r == role) and name.lower() in nm.lower():
            return node
    return None


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    cmd = sys.argv[1]
    if cmd == "arbol":
        a = app()
        if a is None:
            print("Orbital no está corriendo (o no expone AT-SPI)")
            return 1
        for node, depth in walk(a):
            try:
                print("  " * depth + f"[{node.get_role_name()}] {node.get_name()!r}")
            except Exception:
                pass
        return 0
    if cmd == "elegir":
        lista = find("list", "Contenido de la bóveda")
        if lista is None:
            print("No encontré la lista de la bóveda")
            return 1
        # La lista es virtual: solo existen las filas visibles. Si no está, se desplaza hacia abajo
        # con la acción «scroll-to» de la última fila visible.
        for _ in range(50):
            nombres = [lista.get_child_at_index(i).get_name() for i in range(lista.get_child_count())]
            hits = [i for i, n in enumerate(nombres) if sys.argv[2].lower() in (n or "").lower()]
            if hits or not nombres:
                break
            ultima = lista.get_child_at_index(len(nombres) - 1).get_action_iface()
            if ultima is None:
                break
            antes = nombres[-1]
            ultima.do_action(0)
            time.sleep(0.15)
            if lista.get_child_at_index(lista.get_child_count() - 1).get_name() == antes:
                break
        if not hits:
            print(f"No hay ninguna fila con {sys.argv[2]!r}")
            return 1
        # La interfaz de selección usa la posición en el modelo (posinset), no el índice visible.
        fila = lista.get_child_at_index(hits[0])
        posicion = int(fila.get_attributes().get("posinset", hits[0] + 1)) - 1
        lista.get_selection_iface().select_child(posicion)
        print(f"Elegí {nombres[hits[0]]!r}")
        return 0
    role, name = sys.argv[2], sys.argv[3]
    deadline = time.time() + 10
    node = find(role, name)
    while node is None and time.time() < deadline:
        time.sleep(0.2)
        node = find(role, name)
    if node is None:
        print(f"No encontré [{role}] {name!r}")
        return 1
    if cmd == "esperar":
        print(f"Encontré [{node.get_role_name()}] {node.get_name()!r}")
    elif cmd == "activar":
        action = node.get_action_iface()
        if action is None or action.get_n_actions() == 0:
            print(f"[{node.get_role_name()}] {node.get_name()!r} no tiene acciones")
            return 1
        action.do_action(0)
        print(f"Activé [{node.get_role_name()}] {node.get_name()!r} ({Atspi.Action.get_action_name(action, 0)})")
    elif cmd == "escribir":
        editable = node.get_editable_text_iface()
        if editable is None:
            print(f"[{node.get_role_name()}] {node.get_name()!r} no es editable")
            return 1
        editable.set_text_contents(sys.argv[4])
        print(f"Escribí {sys.argv[4]!r} en {node.get_name()!r}")
    elif cmd == "texto":
        t = node.get_text_iface()
        print(t.get_text(0, t.get_character_count()) if t else node.get_name())
    return 0


if __name__ == "__main__":
    sys.exit(main())
