<div align="center">

# Orbital

**Notas en Markdown y HTML para Linux, hechas en Rust con GTK4 y libadwaita.**

El disco siempre tiene la verdad: tus notas son archivos `.md` y `.html` normales en una bóveda
[Open Knowledge Format](https://github.com/GoogleCloudPlatform/open-knowledge-format) `raw/` + `wiki/`,
compatible con otros editores. Ninguna función escribe HTML en tus notas.

[![CI](https://github.com/caracelp/orbital/actions/workflows/ci.yml/badge.svg)](https://github.com/caracelp/orbital/actions/workflows/ci.yml)
[![Licencia: GPL-3.0-or-later](https://img.shields.io/badge/licencia-GPL--3.0--or--later-ff8cc6)](LICENSE)

<img src="docs/img/lectura.png" alt="Orbital: vista Lectura de una nota con propiedades, árbol de archivos y tabla de contenidos generada" width="820">

[Características](#características) · [Atajos](#atajos) · [Compilar](#compilar-y-ejecutar) · [Verificación](#verificación) · [Contribuir](CONTRIBUTING.md)

</div>

## Características

**Núcleo**

- Bóveda `raw/` (fuentes inmutables) + `wiki/` (trabajo) según OKF v0.2; una carpeta cualquiera se
  abre con **Abrir** sin modificarse.
- Notas `.md` (átomos) y `.html` (síntesis) de primera clase: wikilinks, etiquetas, retroenlaces,
  propiedades y búsqueda funcionan igual en las dos.
- Transclusiones `![[nota]]` y `![[nota#sección]]` vivas, plantillas con variables, nota diaria,
  archivado y papelera recuperable con deshacer.
- Retroenlaces en tiempo real en el panel lateral.
- Renombrar y mover actualiza los enlaces que resuelven a la nota, conservando el estilo de cada uno.

**Editor**

- Tres vistas de la misma nota: **Código** (Ctrl+1), **Lectura** (Ctrl+2) y **Dividida** (Ctrl+3).
- **Multi-cursor** en Código: Alt+clic, Alt+arrastre y columnas con Ctrl+Alt+Arriba/Abajo; Esc
  vuelve a un cursor.
- **Outliner de listas**: Enter continúa la lista, Tab/Shift+Tab con subítems, mover ítems entre
  hermanos.
- Autocompletado de wikilinks, etiquetas y estructuras; búsqueda y reemplazo en la nota.
- **Notas al pie** con tooltip en Lectura y comando «Renumerar notas al pie».

**Búsqueda y organización**

- Búsqueda en toda la bóveda (Ctrl+Mayús+F): `"frase"`, `-excluir`, `OR`, `tag:`, `path:`, `file:`,
  `line:`, `section:` y propiedades `[clave:valor]`; resultados con fragmento y clic que abre la nota
  en la línea.
- Etiquetas anidadas con panel de árbol y cantidades, renombrado en toda la bóveda con deshacer.
- Búsquedas guardadas en `.orbital/busquedas.yaml`.
- Selector rápido (Ctrl+P) y paleta de comandos (Ctrl+Mayús+P).

**Contenido generado** (el disco siempre conserva el fuente)

- Marcadores `[%clave]` que muestran valores por sección (inline → YAML → «faltante»).
- Índices por etiqueta (` ```indice `) y tabla de contenidos (` ```toc `).
- **Tarjetas de URL**: ` ```cardlink ` con metadatos og:; se pega una URL y Lectura la muestra como
  tarjeta clicable.

**Conocimiento**

- **Grafo de enlaces** de la bóveda a pantalla completa, con arrastre de nodos y vecinos resaltados.
- **OnStage**: la nota como diapositivas en una ventana separada, con portada, índice y modo calm;
  sigue la edición en vivo mientras presentás.
- **Historial de versiones**: snapshot automático al guardar, 20 versiones por nota con diff y
  restaurar, en `.orbital/historial/`.
- **Exportar la nota abierta** a HTML autónomo y PDF.
- **Archivos externos por URL**: asocia un archivo de `raw/` a una URL y ábrelo en un webview
  embebido con sesión de red efímera.

Todo funciona sin conexión; la red solo entra donde lo pedís explícitamente (metadatos de tarjetas
de URL), con sesión efímera.

## Atajos

| Acción | Atajo |
|--------|-------|
| Nueva nota / abrir carpeta | `Ctrl+N` / `Ctrl+O` |
| Selector rápido / paleta de comandos | `Ctrl+P` / `Ctrl+Mayús+P` |
| Búsqueda global / buscar en la nota / reemplazar | `Ctrl+Mayús+F` / `Ctrl+F` / `Ctrl+H` |
| Vistas Código / Lectura / Dividida | `Ctrl+1` / `Ctrl+2` / `Ctrl+3` |
| Retroenlaces | `Ctrl+Mayús+B` |
| Exportar HTML | `Ctrl+Mayús+E` |
| Nota diaria | `Ctrl+Alt+D` |
| Pegar imagen | `Ctrl+Mayús+V` |
| Renombrar / enviar a papelera | `F2` / `Supr` |
| Multi-cursor (Código) | `Alt+clic`, `Ctrl+Alt+Arriba/Abajo` |
| Preferencias / lista de atajos | `Ctrl+,` / `Ctrl+?` |

La lista completa está en la app, en **Ayuda → Atajos** (`Ctrl+?`).

## Compilar y ejecutar

```sh
cargo run
```

Requiere Rust estable y las bibliotecas de desarrollo de GTK4, libadwaita y WebKitGTK 6.0:

- **Arch:** `gtk4`, `libadwaita`, `webkitgtk-6.0`
- **Debian/Ubuntu:** los paquetes `-dev` correspondientes

Herramientas de desarrollo:

```sh
cargo test -p orbital-vault                # núcleo sin GTK
cargo bench -p orbital-vault               # benchmarks del núcleo
cargo run -p orbital-vault --example generar-boveda -- /tmp/boveda N   # bóveda de prueba
orbital --captura salida.png [carpeta] [nota]                          # captura de la ventana
orbital --medir-escritura <carpeta> <nota>                             # latencia por tecla
orbital --salir-al-mostrar <carpeta>                                   # medir el arranque
```

## Verificación

Cada función lleva pruebas: núcleo en `crates/orbital-vault`, interfaz con GTK en proceso y
regresión AT-SPI con los scripts `tools/verificar-NNN-us*.sh` (cada uno levanta una instancia y una
bóveda temporales propias; nunca se toca la bóveda real del usuario ni se envían teclas sintéticas).

Antes de cerrar cualquier cambio, el gate:

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
cargo deny check
```

La CI de GitHub corre el mismo gate en un contenedor de Arch (por WebKitGTK 6.0).

## Formato de la bóveda

El contrato completo está en [docs/formato-boveda.md](docs/formato-boveda.md): estructura, propiedades,
etiquetas, enlaces, marcadores y bloques generados, y todo lo que vive en `.orbital/` (configuración,
búsquedas guardadas, URL asociadas, historial y papelera recuperable). Es la fuente de verdad: si el
código y el documento no coinciden, el error está en el código.

## Specs y desarrollo

El desarrollo sigue un flujo Spec Kit: cada especificación vive en `specs/NNN-nombre/` con
`spec.md → plan.md → research.md → tasks.md`, más `quickstart.md` (resultados medidos) y
`traspaso.md` (estado exacto y lecciones aprendidas). Las terminadas están fusionadas en `main`;
la cola de trabajo vigente está en [specs/cola-autonoma.md](specs/cola-autonoma.md).

Los principios del proyecto (rendimiento primero, la bóveda es del usuario, nativa de GNOME, local y
privada) están en [.specify/memory/constitution.md](.specify/memory/constitution.md).

## Contribuir

Leé [CONTRIBUTING.md](CONTRIBUTING.md): entorno, gate de verificación, convenciones y flujo de specs.
Errores y pedidos de función van por [issues](https://github.com/caracelp/orbital/issues) con las
plantillas.

## Licencia

GPL-3.0-or-later. Ver [LICENSE](LICENSE).
