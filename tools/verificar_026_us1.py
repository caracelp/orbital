#!/usr/bin/env python3
"""Comprobaciones AT-SPI de la 026 (ids de bloque al guardar); solo el PID propio del lanzador.

Escribe una nota con los siete tipos de bloque, cambia de nota y comprueba que el archivo quedó
con un id por bloque y que la vista Lectura no los muestra.
"""
import os
import re
import runpy
import subprocess
import sys
import time
from pathlib import Path

helpers = runpy.run_path("tools/verificar_004_us4.py")
Atspi = helpers["Atspi"]
assert os.environ.get("ORBITAL_PID"), "Falta el PID de la instancia propia"
vault = Path(os.environ["ORBITAL_TEST_VAULT"]).resolve()
assert vault.parent.name.startswith("orbital-026-us1-"), "Solo acepta la bóveda temporal del lanzador"
NOTA = vault / "wiki/nota.md"

TEXTO = """---
type: nota
---
# Título

Un párrafo.

- primer ítem
- segundo ítem

> una cita

> [!note] Aviso
> un callout

| a | b |
|---|---|
| 1 | 2 |

```rust
fn main() {}
```

$$
x^2
$$

Último párrafo.
"""
BLOQUES = 9  # párrafo, 2 ítems, cita, callout, tabla, código, matemática, último párrafo
ID = re.compile(r"(?:^| )\^([a-z0-9]{6})$", re.M)


def wait_for(check, message, seconds=15):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.15)
    raise AssertionError(message)


def visible(node):
    return node.get_state_set().contains(Atspi.StateType.SHOWING)


def exact(name):
    return next(
        (n for n in helpers["find_all"]("*", name) if n.get_name() == name and visible(n)),
        None,
    )


def nodo(nombre, iface):
    return next(
        (
            n
            for n, _ in helpers["walk"](helpers["get_app"]())
            if visible(n) and (n.get_name() or "") == nombre and getattr(n, iface)()
        ),
        None,
    )


wait_for(lambda: helpers["get_app"](), "No se encontró la instancia propia de Orbital")
node = wait_for(lambda: exact("wiki"), "No aparece la carpeta wiki")
assert helpers["do_action"](node)
frame = wait_for(
    lambda: next((n for n in helpers["find_all"]("frame", "Orbital") if visible(n)), None),
    "No hay ventana",
)


def elegir_nota(fila):
    """Vuelve a la página Archivos y selecciona la fila (el clic no abre)."""
    tab = next(
        (
            n
            for n in helpers["find_all"]("toggle button", "Explorador de archivos")
            if n.get_name() == "Explorador de archivos" and visible(n)
        ),
        None,
    )
    if tab:
        helpers["do_action"](tab)
        time.sleep(0.4)
    # La lista es virtual: `atspi.py elegir` se desplaza hasta la fila.
    salida = subprocess.run(
        [sys.executable, "-I", "tools/atspi.py", "elegir", fila], capture_output=True, text=True
    )
    assert salida.returncode == 0, f"No se pudo elegir {fila!r}: {salida.stdout}{salida.stderr}"
    time.sleep(1.0)


elegir_nota("nota.md")
assert helpers["do_action"](frame, "win.view-code"), "No se pudo pasar a la vista Código"
editor = wait_for(lambda: nodo("Texto de la nota", "get_editable_text_iface"), "No está el editor")
assert editor.get_editable_text_iface().set_text_contents(TEXTO)

# Dejar la nota: el guardado pone id a todos los bloques, también al del cursor.
elegir_nota("otra.md")
disco = wait_for(
    lambda: (t := NOTA.read_text(encoding="utf-8")) and len(ID.findall(t)) == BLOQUES and t,
    f"El archivo no tiene {BLOQUES} ids: {NOTA.read_text(encoding='utf-8')!r}",
)
assert len(set(ID.findall(disco))) == BLOQUES, "Hay ids repetidos"
assert "# Título ^" not in disco and "type: nota ^" not in disco, "Encabezado o frontmatter con id"
quitado = re.sub(r" \^[a-z0-9]{6}$", "", disco, flags=re.M)
quitado = re.sub(r"^\^[a-z0-9]{6}\n", "", quitado, flags=re.M)
assert quitado == TEXTO, f"Cambió algo además de los ids:\n{quitado}"
print(f"OK: al dejar la nota, {BLOQUES} bloques con id y el resto del texto idéntico")

# Lectura no muestra los ids.
elegir_nota("nota.md")
assert helpers["do_action"](frame, "win.view-read"), "No se pudo pasar a Lectura"
lectura = wait_for(lambda: nodo("Nota formateada", "get_text_iface"), "No está la vista Lectura")
texto = wait_for(
    lambda: (t := Atspi.Text.get_text(lectura, 0, -1)) and "Último párrafo" in t and t,
    "Lectura no muestra la nota",
)
for id_ in ID.findall(disco):
    assert id_ not in texto, f"Lectura muestra el id {id_}: {texto!r}"
print("OK: Lectura no muestra los ids")

# Una nota de raw/ (solo lectura) abierta y dejada no cambia (lo comprueba sha256sum el lanzador).
node = wait_for(lambda: exact("raw"), "No aparece la carpeta raw")
assert helpers["do_action"](node)
time.sleep(0.6)
elegir_nota("fuente.md")
node = wait_for(lambda: exact("wiki"), "No aparece la carpeta wiki")
assert helpers["do_action"](node)
time.sleep(1.0)
print("Pendiente para la usuaria: teclado y lector de pantalla.")
