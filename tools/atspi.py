#!/usr/bin/env python3
"""Maneja Orbital por AT-SPI (la capa de accesibilidad que usa Orca), para probar la interfaz.

Uso: ORBITAL_PID=<pid> python3 -I tools/atspi.py <comando> [args]
La app de prueba se lanza con ORBITAL_INSTANCIA_PROPIA=1 para no tocar la del usuario.
  arbol                      imprime el árbol accesible de la ventana de Orbital
  activar <rol> <nombre>     ejecuta la acción principal del primer elemento con ese rol y nombre
  esperar <rol> <nombre>     espera (hasta 10 s) a que aparezca un elemento
  texto <rol> <nombre>       imprime el texto de un elemento
  escribir <rol> <nombre> <texto>  reemplaza el texto de un campo editable
  elegir <nombre>            elige la fila de la lista de la bóveda cuyo nombre contiene <nombre>

<nombre> es un fragmento del nombre accesible («=Nombre» exige el nombre exacto); <rol> es el nombre de rol de AT-SPI
(por ejemplo «push button», «list item», «label») o «*» para cualquiera.
"""
import sys
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402


def app():
    """La instancia de Orbital de prueba.

    Exige ORBITAL_PID: el usuario puede tener su propio Orbital abierto, y las pruebas nunca deben
    tocarlo. Lanzar la app de prueba con ORBITAL_INSTANCIA_PROPIA=1 y pasar su PID.
    """
    import os

    pid = os.environ.get("ORBITAL_PID")
    if not pid:
        sys.exit("Falta ORBITAL_PID: el PID de la instancia de prueba (nunca la del usuario).")
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


def limpiar_cache(nodo):
    """El cliente AT-SPI guarda en caché los hijos: después de desplazar una lista virtual, sin
    limpiarla se siguen viendo las filas de antes (y parece que no se movió)."""
    try:
        nodo.clear_cache()
    except Exception:
        pass


def find(role, name):
    a = app()
    if a is None:
        return None
    # El cliente AT-SPI guarda en caché los hijos de cada nodo: sin limpiarla, una espera que
    # empezó antes de que apareciera un diálogo nunca lo ve.
    try:
        a.clear_cache()
    except Exception:
        pass
    for node, _ in walk(a):
        try:
            r = node.get_role_name()
            nm = node.get_name() or ""
        except Exception:
            continue
        exact = name.startswith("=")
        hit = nm == name[1:] if exact else name.lower() in nm.lower()
        role_hit = role == "*" or r == role or (role == "button" and "button" in r)
        if role_hit and hit:
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
    if cmd == "tecla":
        name = sys.argv[2]
        mapping = {
            "tab": (0xFF09, Atspi.KeySynthType.SYM),
            "enter": (0xFF0D, Atspi.KeySynthType.SYM),
            "return": (0xFF0D, Atspi.KeySynthType.SYM),
            "space": (0x0020, Atspi.KeySynthType.SYM),
            "escape": (0xFF1B, Atspi.KeySynthType.SYM),
            "up": (0xFF52, Atspi.KeySynthType.SYM),
            "down": (0xFF54, Atspi.KeySynthType.SYM),
            "left": (0xFF51, Atspi.KeySynthType.SYM),
            "right": (0xFF53, Atspi.KeySynthType.SYM),
        }
        val, synth = mapping.get(name.lower(), (0, None))
        if synth:
            Atspi.generate_keyboard_event(val, None, Atspi.KeySynthType.PRESS | synth)
            time.sleep(0.05)
            Atspi.generate_keyboard_event(val, None, Atspi.KeySynthType.RELEASE | synth)
        else:
            Atspi.generate_keyboard_event(0, name, Atspi.KeySynthType.STRING)
        return 0
    if cmd == "elegir":
        lista = find("list", "Contenido de la bóveda")
        if lista is None:
            print("No encontré la lista de la bóveda")
            return 1
        # La lista es virtual: solo existen las filas visibles. Primero se sube hasta el principio
        # (así el índice visible es la posición absoluta); si la fila no está, se baja con la
        # acción «scroll-to» de la última fila visible.
        for _ in range(50):
            if lista.get_child_count() == 0:
                break
            primera = lista.get_child_at_index(0)
            antes = primera.get_name()
            accion = primera.get_action_iface()
            if accion is None or (primera.get_attributes() or {}).get("posinset") == "1":
                break
            accion.do_action(0)
            time.sleep(0.1)
            limpiar_cache(lista)
            if lista.get_child_at_index(0).get_name() == antes:
                break
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
            limpiar_cache(lista)
            if lista.get_child_at_index(lista.get_child_count() - 1).get_name() == antes:
                break
        if not hits:
            print(f"No hay ninguna fila con {sys.argv[2]!r}")
            return 1
        # La interfaz de selección usa la posición absoluta en la lista. GTK informa `posinset`
        # dentro del nivel del árbol, así que no sirve con carpetas abiertas: se usa el índice
        # visible, que coincide con la posición absoluta mientras la lista no esté desplazada.
        # Con la lista desplazada, el índice visible no es la posición absoluta: se elige esa
        # posición, se mira qué fila visible quedó elegida y de ahí sale el desplazamiento real.
        seleccion = lista.get_selection_iface()
        objetivo = nombres[hits[0]]
        # «Nota x.md», «Archivo x.pdf, solo lectura», «Carpeta wiki» → «x.md».
        archivo = objetivo.split(" ", 1)[-1].split(",")[0]

        def abierta():
            for n, _ in walk(app()):
                try:
                    if n.get_role_name() == "label" and (n.get_name() or "").split(" · ")[0].endswith("/" + archivo):
                        return True
                except Exception:
                    continue
            return False

        # Con la lista desplazada, el índice visible no es la posición absoluta (que es mayor o
        # igual): se prueba hasta que la barra de la nota muestre el archivo buscado.
        desplazada = (lista.get_child_at_index(0).get_attributes() or {}).get("posinset") != "1"
        for extra in range(0, 80 if desplazada else 1):
            seleccion.select_child(hits[0] + extra)
            for _ in range(10):
                time.sleep(0.05)
                if abierta():
                    break
            if abierta() or objetivo.startswith("Carpeta"):
                break
        else:
            if desplazada:
                print(f"No pude elegir {objetivo!r}")
                return 1
        print(f"Elegí {objetivo!r}")
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
        act_idx = 0
        if len(sys.argv) > 4:
            target_act = sys.argv[4]
            for i in range(action.get_n_actions()):
                if action.get_action_name(i).lower() == target_act.lower():
                    act_idx = i
                    break
        if not action.do_action(act_idx):
            print(f"AT-SPI rechazó la acción de [{node.get_role_name()}] {node.get_name()!r}")
            return 1
        print(f"Activé [{node.get_role_name()}] {node.get_name()!r} ({action.get_action_name(act_idx)})")
    elif cmd == "escribir":
        editable = node.get_editable_text_iface()
        if editable is None:
            print(f"[{node.get_role_name()}] {node.get_name()!r} no es editable")
            return 1
        editable.set_text_contents(sys.argv[4])
        print(f"Escribí {sys.argv[4]!r} en {node.get_name()!r}")
    elif cmd == "texto":
        t = node.get_text_iface()
        print(Atspi.Text.get_text(t, 0, Atspi.Text.get_character_count(t)) if t else node.get_name())
    elif cmd == "foco":
        comp = node.get_component_iface()
        if comp is None:
            print(f"[{node.get_role_name()}] {node.get_name()!r} no es un componente")
            return 1
        comp.grab_focus()
        print(f"Di foco a [{node.get_role_name()}] {node.get_name()!r}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
