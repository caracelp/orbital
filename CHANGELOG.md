# Cambios

Resumen por bloques de trabajo. El detalle está en el historial de commits. Las fechas son las de
la fusión en `main`.

## Sin publicar

### Sincronización y arranque (2026-11-22)

- **Google Drive**: espejo de la bóveda con una carpeta del Drive de la persona, a través de su
  propio proyecto de Google Cloud. Nunca borra (papelera en los dos lados), fusiona los cambios de
  texto que no se pisan y deja elegir en los conflictos; reglas «solo en este equipo»;
  credenciales en el llavero del sistema.
- **Arranque**: la bóveda se muestra en el primer cuadro; con 5000 notas, el primer cuadro útil
  pasa de 522 a 425 ms en el equipo de referencia.
- **Tabla de contenidos** con valores por defecto configurables e «Insertar índice…»; **índices
  por etiqueta** (` ```indice `) que se actualizan solos sin escribir en el disco.

### Medios y diagramas (2026-11-20 a 2026-11-22)

- **Audio y video** incrustados con reproductores nativos (GStreamer), que no abren el archivo
  hasta que se reproduce.
- **Diagramas Mermaid** dibujados sin navegador, con el tema de la app; ampliar y copiar como
  imagen, emoji y diagramas de Gantt.
- **Carrusel** de medios, **galería en la nota** y **galería lateral** con visor a pantalla
  completa e «Ir al embed».
- Varios embeds en un mismo renglón.

### Visual Math y editor (2026-11-19)

- **Fórmulas LaTeX** en línea y en bloque, dibujadas de forma nativa, en Lectura, Código,
  OnStage, notas HTML y al exportar.
- **Editor de ecuaciones** (`Ctrl+M`), **química** (`\ce`) y **unidades** (`\qty`, `\SI`, `\pu`).
- **Gráficos 2D y polares** y **gráficos 3D** interactivos con deslizadores.
- Números de línea en Código, resaltado de código en Lectura y la ventana que abre maximizada.

### Organización y conocimiento (2026-11-16 a 2026-11-18)

- **Favoritos** y listas **Guardados**.
- **Grafo** con física Barnes-Hut en segundo plano y grafo local de 1 a 3 saltos; **brújula** de
  la nota abierta.
- **Canvas** (`.canvas`) y **mapa mental** desde una nota.
- **Ramas libres** con identificadores jerárquicos de bloques, y notas en tres partes
  (propiedades, cuerpo y conexiones).
- **Outliner plegable** en Código y Lectura con enfoque y miga de pan.
- **Bases** (`.base`): tablas y tarjetas sobre las propiedades, editables en el lugar, y
  renombrado de propiedades con vista previa.
- `tipo` como alias de `type` e identificadores de nota únicos en la bóveda.
- Transclusiones rediseñadas en Lectura y **tarjetas de estudio**.
- **Consultas vivas** en la Lectura y **conexiones con tipo y motivo**.
- Lo plegado queda guardado, casillas en listas con renglones vacíos, puntero según contexto.

## 0.1.0 (2026-11-15)

### Interfaz y distribución

- Interfaz nueva: riel de navegación, pestañas que recuerdan la vista y la posición, barra
  derecha con propiedades, contexto y grafo local, barra de estado y paleta rosa y negra.
- **Flatpak** (GNOME 51) y CI con las pruebas GTK en proceso.
- Enlaces a bloques (`[[nota#^id]]`).
- Escritura sin demoras en notas grandes (de 30 ms a 1,9 ms por tecla con 100 KB).

### Base (2026-11-09 a 2026-11-14)

- **Formato de la bóveda** (`raw/` + `wiki/`, compatible con Open Knowledge Format) con «Abrir» y
  «Abrir como bóveda de Orbital».
- **Editor** con vistas Código, Lectura y Dividida; notas HTML con vista Visual; autoguardado.
- **Enlaces**: wikilinks, retroenlaces, transclusiones y reescritura de enlaces al renombrar.
- **Búsqueda** con operadores, etiquetas anidadas y búsquedas guardadas.
- Contenido generado (marcadores y codeblocks), **varios cursores**, **exportar** a HTML y PDF,
  **grafo de enlaces**, URL asociadas a archivos, **historial de versiones**, outliner, notas al
  pie, tarjetas de URL, **OnStage**, **importar** PDF, DOCX y HTML, y anotar PDFs con citas.
