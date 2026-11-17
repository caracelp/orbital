#!/usr/bin/env python3
"""Verificación de bases sobre la instancia propia. Toda acción ausente falla explícitamente."""
import os
import runpy
import subprocess
import sys
import time
from pathlib import Path

helpers = runpy.run_path('tools/verificar_004_us4.py')
Atspi = helpers['Atspi']
assert os.environ.get('ORBITAL_PID'), 'Falta ORBITAL_PID'
vault = Path(os.environ['ORBITAL_TEST_VAULT']).resolve()
assert vault.parent.name.startswith('orbital-029-us1-'), 'Bóveda ajena al lanzador'
phase = sys.argv[1]


def wait(check, message, seconds=25):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        result = check()
        if result:
            return result
        time.sleep(.15)
    raise AssertionError(message)


def visible(node):
    try:
        node.clear_cache()
        return node.get_state_set().contains(Atspi.StateType.SHOWING)
    except Exception:
        return False


def exact(name):
    # La celda de la tabla hereda el nombre de su botón y su acción es «desplazar hasta»:
    # entre dos nodos con el mismo nombre se prefiere el botón.
    found = [n for n in helpers['find_all']('*', name)
             if n.get_name() == name and visible(n)]
    found.sort(key=lambda n: 'button' not in n.get_role_name())
    return found[0] if found else None


def click(name):
    node = wait(lambda: exact(name), f'No aparece {name}')
    assert helpers['do_action'](node), f'{name} no permite activarse por AT-SPI'


def text(name, value):
    node = wait(lambda: exact(name), f'No aparece {name}')
    assert helpers['set_text'](node, value), f'{name} no permite editarse por AT-SPI'


def open_base():
    # Desplegar la carpeta solo si está plegada (activarla de nuevo la plegaría). La fila de la
    # base puede quedar fuera de lo dibujado: `elegir` se desplaza hasta ella.
    folder = wait(lambda: exact('wiki'), 'No aparece wiki')
    folder.clear_cache()
    if not folder.get_state_set().contains(Atspi.StateType.EXPANDED):
        assert helpers['do_action'](folder, 'listitem.expand'), 'No se puede expandir wiki'
        wait(lambda: exact('wiki').get_state_set().contains(Atspi.StateType.EXPANDED),
             'wiki no se desplegó')
    result = subprocess.run([sys.executable, '-I', 'tools/atspi.py', 'elegir', 'precios.base'],
                            capture_output=True, text=True)
    assert result.returncode == 0, result.stdout + result.stderr
    wait(lambda: exact('Tabla de notas de la base'), 'No se abrió la tabla')


wait(helpers['get_app'], 'No aparece la instancia de prueba')
if phase == 'editar':
    original_md = (vault/'wiki/a.md').read_text()
    original_html = (vault/'wiki/b.html').read_text()
    click('Crear base de datos')
    text('Nombre de la base', 'nueva')
    click('Crear')
    wait(lambda: list(vault.rglob('nueva.base')), 'Crear no guardó nueva.base')
    wait(lambda: exact('Tabla de notas de la base'), 'La base nueva no se abrió')
    print('OK: DB crea un archivo .base')
    open_base()
    click('Editar Precio de Alfa: 10')
    text('Valor de la propiedad', '12')
    click('Guardar')
    wait(lambda: 'precio: 12' in (vault/'wiki/a.md').read_text(), 'Celda MD no guardada')
    assert (vault/'wiki/a.md').read_text() == original_md.replace('precio: 10', 'precio: 12')
    # La tabla se vuelve a cargar tras cada edición; un diálogo abierto sobre la versión
    # anterior rechaza el guardado, así que se espera a la tabla nueva.
    wait(lambda: exact('Editar Precio de Alfa: 12'), 'La tabla no mostró el valor nuevo')
    click('Editar Precio de Beta: 20')
    text('Valor de la propiedad', '22')
    click('Guardar')
    wait(lambda: 'precio: 22' in (vault/'wiki/b.html').read_text(), 'Celda HTML no guardada')
    assert (vault/'wiki/b.html').read_text() == original_html.replace('precio: 20', 'precio: 22')
    print('OK: edición MD/HTML conserva todo lo ajeno a la propiedad')
    path = vault/'wiki/a.md'
    path.write_text(path.read_text().replace('estado: pendiente', 'estado: terminado'))
    wait(lambda: not exact('Editar Precio de Alfa: 12'), 'Cambio externo no quitó Alfa del filtro')
    wait(lambda: exact('Editar Precio de Beta: 22'), 'Se perdió Beta al actualizar')
    assert (vault/'raw/fuente.txt').read_text() == 'Fuente intocable\n'
    print('OK: cambio externo actualiza la selección; raw intacto')
elif phase == 'reabrir':
    open_base()
    wait(lambda: exact('Editar Precio de Beta: 22'), 'Reabrir perdió datos o filtro')
    assert not exact('Editar Precio de Alfa: 12'), 'Reabrir ignoró filtro'
    print('OK: al reabrir conserva datos y configuración')
else:
    raise AssertionError(f'Fase desconocida: {phase}')
