# Orbital

Aplicación de escritorio para Linux de toma de notas en Markdown y HTML, con herramientas de
productividad.

Hecha en **Rust** con **GTK4** y **libadwaita**. Las notas son archivos `.md` y `.html` normales en
una bóveda `raw/` + `wiki/` compatible con [Open Knowledge Format v0.2](https://github.com/GoogleCloudPlatform/open-knowledge-format): el disco siempre tiene la verdad y ninguna función escribe HTML en tus notas.

## Funciones

- **Búsqueda en toda la bóveda** (Ctrl+Mayús+F): operadores `"frase"`, `-excluir`, `OR`,
  `tag:`, `path:`, `file:`, `line:`, `section:` y propiedades `[clave:valor]`; resultados agrupados
  con fragmento y clic que abre la nota en la línea.
- **Selector rápido** (Ctrl+P) y **paleta de comandos** (Ctrl+Mayús+P).
- **Etiquetas** anidadas con panel de árbol y cantidades, autocompletado de `#` y renombrado en
  toda la bóveda con Deshacer.
- **Búsquedas guardadas** con nombre en `.orbital/busquedas.yaml`.
- **Contenido generado**: marcadores `[%clave]` que muestran valores por sección (inline → YAML →
  «faltante»), índices por etiqueta (` ```indice `) y tabla de contenidos (` ```toc `); el disco
  siempre conserva el fuente.
- **Multi-cursor** en Código: Alt+clic/arrastre y columnas con Ctrl+Alt+Arriba/Abajo.
- **Exportar nota** a HTML autónomo y PDF.
- **Grafo de enlaces** de la bóveda a pantalla completa.
- **Archivos externos por URL**: asocia un archivo de `raw/` a una URL y ábrela en un webview
  embebido con sesión de red efímera.
- Retroenlaces en tiempo real, transclusiones `![[nota]]`, plantillas, diario, papelera
  recuperable y archivos auxiliares (`.base`, `.canvas`) legibles.

## Formato de la bóveda

El contrato completo está en [docs/formato-boveda.md](docs/formato-boveda.md): estructura de la
bóveda, propiedades, etiquetas, marcadores y bloques generados, `.orbital/` (configuración,
búsquedas guardadas, URL asociadas) y papelera recuperable. Los archivos auxiliares llevan
`format_version`.

## Compilar y ejecutar

```sh
cargo run
```

Requiere Rust estable y las bibliotecas de desarrollo de GTK4, libadwaita y WebKitGTK (en Arch:
`gtk4`, `libadwaita`, `webkitgtk-6.0`; en Debian/Ubuntu los paquetes `-dev` correspondientes).

## Verificación

Cada función lleva pruebas: núcleo en `crates/orbital-vault`, interfaz con GTK en proceso y
regresión AT-SPI con los scripts `tools/verificar-NNN-us*.sh` (cada uno levanta una instancia y una
bóveda temporales propias; nunca se toca la bóveda real ni se envían teclas sintéticas). Antes de
cerrar cualquier cambio: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test` y
`cargo deny check`.

## Specs

El desarrollo sigue un flujo Spec Kit: cada especificación vive en `specs/NNN-nombre/` con
`spec.md → plan.md → research.md → tasks.md`, más `quickstart.md` (resultados medidos) y
`traspaso.md` (estado exacto y lecciones aprendidas). La cola de trabajo vigente está en
[specs/cola-autonoma.md](specs/cola-autonoma.md).

## Licencia

GPL-3.0-or-later. Ver [LICENSE](LICENSE).
