# Contrato: formato en disco de una bóveda de Orbital (versión 1)

Documento público del formato (FR-032). Es la fuente de verdad: si el código y este documento no coinciden,
el error está en el código. Define lo que cualquier herramienta
(otro editor, un agente, un script) puede esperar de una bóveda de Orbital y lo que tiene que respetar.

## 1. Estructura

```
<bóveda>/
├── raw/          fuentes inmutables
├── wiki/         notas y archivos de trabajo
│   ├── index.md  reservado
│   └── log.md    reservado
├── AGENTS.md     opcional
└── .orbital/     propio de Orbital
```

- Una carpeta sin esta estructura sigue siendo legible: Orbital la abre con **Abrir** y no la cambia.
- `raw/`: Orbital nunca modifica, renombra ni borra nada de esta carpeta. Solo agrega archivos copiados.
- `wiki/`: todo lo demás. Las subcarpetas son libres. Las funciones identifican las notas por `type`, no por
  carpeta.
- `.orbital/`: `config.yaml`, `respaldo/`, `papelera/` (solo se usa si el disco no tiene papelera del
  sistema, por ejemplo tmpfs o FAT), `busquedas.yaml`, `enlaces.yaml` e `historial/` (secciones 9 y
  12.1). Ninguna otra herramienta debería escribir aquí. Borrarla no pierde contenido de las notas.

## 2. Elementos ocultos

Cualquier archivo o carpeta cuyo nombre empiece con `.` (por ejemplo `.orbital/` o `.git/`) queda fuera de
toda lista, búsqueda, enlace, índice y embedding.

## 3. Notas atómicas (`.md`)

UTF-8, con frontmatter YAML 1.2 entre dos líneas `---` al inicio (después de un BOM opcional), según OKF v0.2
§4. Cuerpo en Markdown.

```markdown
---
type: atomo
title: Entropía
description: Medida del desorden de un sistema.
tags: [fisica, termodinamica]
created: 2026-11-09T18:30:00-03:00
generated: { by: human:ejemplo, at: 2026-11-09T18:30:00-03:00 }
sources:
  - id: callen
    resource: /raw/callen-termodinamica.pdf
---
# Entropía
...
```

## 4. Notas de síntesis (`.html`)

Documento HTML. Las propiedades van en **una sola** etiqueta `<script type="application/yaml">` dentro de
`<head>`, con el mismo YAML que el frontmatter. Los navegadores no muestran ni ejecutan ese bloque.

```html
<!doctype html>
<html lang="es">
<head>
  <meta charset="utf-8">
  <title>Termodinámica</title>
  <script type="application/yaml">
type: sintesis
title: Termodinámica
tags: [fisica]
  </script>
</head>
<body>…</body>
</html>
```

- Un valor que contenga `</` se escribe entre comillas dobles con la barra escapada: `"<\/script>"`.
- Si hay más de una etiqueta de este tipo, solo cuenta la primera. Orbital avisa.
- El JavaScript de la nota y su aislamiento se definen en la especificación de las notas HTML.

### 4.1. Estilos de Orbital (`<style data-orbital>`)

Para que el HTML se mantenga limpio y legible tanto en la vista Código como en editores externos o
navegadores, Orbital nunca inserta atributos `style="..."` en línea al aplicar formatos desde la vista
Visual (FR-014b). En su lugar, utiliza clases semánticas con prefijo `o-` cuyas reglas se agrupan en una
única etiqueta `<style data-orbital>` dentro del `<head>`:

```html
<head>
  ...
  <style data-orbital>
.o-color-rosa { color: #ff8cc6; }
.o-columnas-2 { column-count: 2; column-gap: 2em; }
  </style>
</head>
```

- **Clases estándar reconocidas**:
  - **Color de texto**: `o-color-rosa`, `o-color-rojo`, `o-color-naranja`, `o-color-amarillo`, `o-color-verde`,
    `o-color-azul`, `o-color-violeta`, `o-color-gris`.
  - **Color de fondo**: `o-fondo-rosa`, `o-fondo-rojo`, `o-fondo-naranja`, `o-fondo-amarillo`, `o-fondo-verde`,
    `o-fondo-azul`, `o-fondo-violeta`, `o-fondo-gris`.
  - **Alineación**: `o-alinear-izquierda`, `o-alinear-centro`, `o-alinear-derecha`, `o-alinear-justificado`.
  - **Columnas**: `o-columnas-2`, `o-columnas-3`.
  - **Estilos de bloque**: `o-estilo-destacado` (borde rosa y realce), `o-estilo-nota` (bloque azul con fondo suave),
    `o-estilo-advertencia` (bloque amarillo con fondo suave).
- **Regla de edición**: Al guardar desde la vista Visual, el bloque `<style data-orbital>` es la **única**
  parte de `<head>` que Orbital puede agregar o actualizar (solo incluyendo las reglas de las clases
  efectivamente utilizadas en el documento). Todo el resto de `<head>` (metadatos, estilos propios de la
  nota, scripts) y el bloque de propiedades se conservan byte a byte (FR-015).


## 5. Propiedades

- Obligatoria en `wiki/`: `type` (no bloqueante; Orbital avisa si falta).
- Reconocidas: las de OKF v0.2 (`type`, `title`, `description`, `resource`, `tags`, `status`, `sources`,
  `generated`, `verified`, `stale_after`) y las de Orbital (`aliases`, `archived`, `created`, `updated`).
- Valores de `type` usados por Orbital: `atomo`, `sintesis`, `plantilla`, `diario`. Se aceptan otros
  (tipos libres, como `cuaderno`): no son un error.
- `tipo` es alias pleno de `type`: una nota con `tipo: cuaderno` tiene tipo `cuaderno` y no recibe el
  aviso de que falta `type`. Orbital no le agrega `type` y, si cambia el tipo (por ejemplo desde Bases),
  escribe en `tipo`. Si una nota tiene las dos claves, **gana `type`** y `tipo` se conserva sin usarse.
  Las notas nuevas se crean con `type`.
- `id` (040/041): identificador de la nota, `<PREFIJO>-NNN` (`AT-001`). El prefijo sale del tipo: tabla
  fija (`atomo` → `AT`, `apunte` → `AP`, `concepto` → `CO`, `cuaderno` → `CU`…) o, para un tipo libre,
  su primera letra y la siguiente que no forme un prefijo de la tabla (`cosa` → `CS`, `cuento` → `CE`).
  El prefijo depende solo del nombre del tipo; dos tipos libres pueden compartirlo. El número es el
  primero libre en **toda la bóveda**: cuenta los `id` de todas las notas y los ids de los delimitadores
  `<!-- block … ID -->`, `<!-- summary ID -->`, `<!-- sources ID -->` y `<!-- connections ID -->`.
  Orbital solo pone un `id` cuando se le pide (Estructurar en 3 partes, una parte opcional, Ramas
  libres). Si dos notas tienen el mismo id, avisa en la nota (sin bloquear ni reescribir nada).
- Las fechas con hora llevan desplazamiento explícito (OKF §5).
- Las claves desconocidas se conservan. Editar una propiedad no cambia el resto del bloque ni del archivo.

## 6. Enlaces

- Wikilinks: `[[nombre]]`, `[[nombre|alias]]`, `[[nombre#encabezado]]`, `[[nombre#^bloque]]`. Se resuelven por nombre de archivo
  dentro de `wiki/`. Si `nombre` coincide con un `.md` y un `.html`, el enlace es ambiguo; `[[nombre.html]]`
  lo resuelve.
- Markdown estándar: `[texto](/ruta.md)` relativo a la raíz de `wiki/` (forma recomendada por OKF §6.1) o
  `[texto](./ruta.md)` relativo a la nota.
- Un enlace a una nota inexistente es válido: representa una nota por escribir.
- Transclusiones: un párrafo que es solo `![[nombre]]` o `![[nombre#encabezado]]` muestra el contenido de esa
  nota (o de esa sección: el encabezado y lo que sigue hasta otro del mismo nivel o mayor) en la vista
  formateada. Se resuelven como los wikilinks. Se anidan hasta 3 niveles; una referencia circular se muestra
  como aviso. En las notas `.html` el archivo guarda solo el texto `![[…]]`: la tarjeta la dibuja Orbital y
  nunca se escribe en el archivo.
- Identificadores de bloque (026): al guardar una nota de `wiki/`, Orbital le pone un id a cada bloque que
  no lo tiene (párrafos, cada ítem de lista, citas, callouts, tablas, bloques de código y `$$`). El bloque
  donde está el cursor lo recibe al guardar con el cursor en otro bloque o al dejar la nota. Abrir y dejar
  una nota sin cambios no la modifica; `raw/` nunca se toca. En Código y en el archivo se ven; en Lectura,
  la búsqueda, exportar y OnStage, no.

  ~~~markdown
  La entropía mide el desorden de un sistema. ^k3x9a1

  - Primer principio ^p2m8q4
    - Un subítem ^r5t1w9

  > Una cita de varias líneas
  > que termina acá. ^c8h2n6

  | a | b |
  |---|---|
  | 1 | 2 |
  ^t9y3u1

  ```rust
  fn main() {}
  ```
  ^q1w2e3
  ~~~

  - El id es `^` + letras, dígitos o guiones (Orbital genera 6 caracteres `a-z0-9`), separado por un
    espacio al final del bloque; en tablas, código y `$$`, solo en la línea siguiente. Un id escrito a
    mano (o por Obsidian) se respeta; si se repite en la nota, Orbital reemplaza los siguientes.
  - Los encabezados se enlazan por su texto (`[[nota#Encabezado]]`) y no llevan id; el frontmatter
    tampoco.
  - En las notas `.html` el id es el atributo `id` de `p`, `li`, `blockquote`, `table`, `pre` y `figure`
    (`<p id="k3x9a1">…</p>`), también desde la vista Visual.
  - `[[nota#^k3x9a1]]` (o `[texto](/ruta/nota.md#^k3x9a1)`) lleva al bloque y lo resalta; `[[#^k3x9a1]]`
    es un bloque de la misma nota. `![[nota#^k3x9a1]]`, solo en su párrafo, incrusta ese bloque (un ítem,
    con sus subítems). Al escribir `[[nota#^` se sugieren los bloques de esa nota con su texto.
- Al renombrar o mover una nota, Orbital ofrece actualizar los enlaces que **resuelven** a ella (no los que
  solo comparten el nombre). De cada enlace cambia solo la ruta, en el mismo estilo (con o sin carpeta o
  extensión, `/…` o relativa); alias, anclas y el resto del archivo quedan iguales.

## 7. `wiki/index.md` y `wiki/log.md`

Formato de OKF v0.2 §8 y §9.

```markdown
# atomo

* [Entropía](/entropia.md) - Medida del desorden de un sistema.

# sintesis

* [Termodinámica](/termodinamica.html) - Panorama de la termodinámica clásica.
```

```markdown
# Registro de la wiki

## 2026-11-09
* **Creación**: [Entropía](/entropia.md)
* **Renombre**: [Calor](/calor.md) (antes `temperatura.md`)
```

- Orbital agrega, cambia y quita solo las líneas `* [Título](ruta) - descripción` que apuntan a notas, y las
  entradas de `log.md` del día en curso. El resto del texto no lo toca.
- Orbital no crea estos archivos salvo con **Abrir como bóveda de Orbital**.

## 8. Plantillas y diario

- Plantilla: nota con `type: plantilla`. Variables: `{{fecha}}`, `{{hora}}`, `{{titulo}}`, `{{carpeta}}`.
- Nota diaria: `wiki/<AAAA-MM-DD>.md` con `type: diario` (el nombre se configura).

## 9. Archivos auxiliares

- `.base` y `.canvas`: texto legible en YAML o JSON (JSON también es YAML válido) con `format_version`
  entero. Si falta o el archivo tiene errores, Orbital lo muestra y no lo sobrescribe sin confirmación. Su
  contenido se especifica en sus propias funciones.
- `.orbital/config.yaml`: `format_version`, `link_style` (`wikilink` | `markdown`), `journal_name`, `actor`.
- `.orbital/busquedas.yaml` (004): búsquedas guardadas, una lista de mapas `name`/`query`. Escritura
  atómica con respaldo; si el YAML tiene errores, Orbital avisa y no lo sobrescribe.
- `.orbital/enlaces.yaml` (009): archivos de `raw/` asociados a una URL, una lista de mapas
  `ruta`/`url`. La ruta es relativa a la raíz de la bóveda y la URL es `http` o `https`. Al
  renombrar o mover el archivo, la entrada se actualiza.

  ```yaml
  - ruta: raw/callen-termodinamica.pdf
    url: https://ejemplo.com/termodinamica
  ```

- `.orbital/historial/<ruta>/` (010): versiones automáticas del contenido **previo** a cada guardado,
  un archivo de texto por versión con nombre `AAAA-MM-DD_HH-MM-SS.txt` (milisegundos y sufijo
  incremental si hace falta). Máximo 20 versiones por nota: se descartan las más viejas. Un guardado
  que no cambia el contenido no crea versión duplicada. Las versiones nunca son notas: no entran en
  la búsqueda, el grafo ni las listas.

- `.orbital/listas.yaml` (027): Favoritos y Guardados de la bóveda. Viaja con la carpeta:
  sincronizarla o copiarla los lleva.

  ```yaml
  format_version: 1
  favoritos:
    - wiki/entropia.md
    - raw/callen-termodinamica.pdf
  colecciones:
    - nombre: Para leer
      notas:
        - wiki/segundo-principio.html
        - wiki/entropia.md
    - nombre: "Tesis: capítulo 2"
      notas: []
  ```

  - `format_version`: entero, hoy `1`. Si falta se asume `1`; uno mayor que el que Orbital entiende
    se trata como archivo dañado (no se pisa).
  - `favoritos`: lista de rutas, en el orden en que se muestran.
  - `colecciones`: lista de mapas con `nombre` (texto no vacío, único sin distinguir mayúsculas) y
    `notas` (lista de rutas en orden; puede estar vacía). Las colecciones no se anidan.
  - Las claves que faltan valen como listas vacías. Un archivo vacío o ausente = sin favoritos ni
    colecciones.
  - Una ruta es relativa a la raíz de la bóveda, con `/`, sin componentes ocultos ni `..`. Puede ser
    una nota de `wiki/` (`.md` o `.html`) o cualquier archivo que Orbital abre, también de `raw/`.
  - Una ruta repetida dentro de la misma lista se toma una sola vez (la primera). Una ruta que no
    existe se conserva y Orbital la muestra como faltante, con la opción de quitarla.

  Cómo lo trata Orbital:

  - Lo crea (y crea `.orbital/`) recién al marcar el primer favorito o crear la primera colección.
  - Escritura atómica. Antes de escribir encima de un contenido que no escribió esta misma sesión de
    Orbital (el que había al abrir la bóveda, uno cambiado por fuera o uno dañado), lo copia a
    `.orbital/respaldo/<fecha>/.orbital/listas.<hora>.yaml`. Los cambios siguientes de la misma
    sesión (reordenar, marcar) no dejan más copias.
  - Si el archivo tiene errores (YAML inválido, claves desconocidas, rutas inválidas, nombres de
    colección vacíos o repetidos), Orbital avisa una vez, muestra las listas vacías y no lo
    sobrescribe hasta que la usuaria haga un cambio; ese cambio respalda primero el archivo dañado.
  - Si el archivo cambia afuera mientras Orbital está abierto, lo vuelve a leer. Si cambió entre la
    lectura y una escritura, lo vuelve a leer y repite el cambio sobre lo nuevo.
  - Renombrar o mover un archivo o una carpeta desde Orbital actualiza las rutas; borrar desde
    Orbital quita las entradas (y «Deshacer» las repone). Lo que se borra o mueve por fuera queda
    como faltante.
  - Ninguna nota ni archivo de `raw/` se modifica.

- `.orbital/plegado.yaml` (028): qué partes de cada nota están plegadas (outliner plegable). Viaja
  con la carpeta: sincronizarla o copiarla lo lleva. Las notas nunca cambian por plegar.

  ```yaml
  format_version: 1
  notas:
    - ruta: wiki/entropia.md
      plegados:
        - encabezado: Historia
        - encabezado: Ejemplos
          aparicion: 2
        - bloque: p2m8q4
        - item: Comprar pan
    - ruta: "wiki/proyectos/tesis: capítulo 2.md"
      plegados:
        - encabezado: Bibliografía
  ```

  - `format_version`: entero, hoy `1`. Si falta se asume `1`; uno mayor que el que Orbital entiende
    se trata como archivo dañado y no se pisa.
  - `notas`: lista de mapas con `ruta` y `plegados`, ordenada por ruta. Solo aparecen las notas con
    algo plegado.
  - `ruta`: relativa a la raíz de la bóveda, con `/`, sin componentes ocultos ni `..`; una nota
    `.md`. Una ruta repetida daña el archivo.
  - `plegados`: lista (no vacía) de pliegues, en el orden en que aparecen en la nota. Cada pliegue
    es un mapa con **una** de estas claves:
    - `bloque: <id>`: el ítem de lista cuya primera línea termina en `^<id>` (el identificador de
      bloque de la 026). Es la forma preferida: sobrevive a cambios en el texto del ítem.
    - `item: <texto>`: un ítem de lista sin identificador, por el texto de su primera línea (sin el
      marcador `-`/`*`/`+`/`1.`, sin la casilla `[ ]` y recortado).
    - `encabezado: <texto>`: una sección, por el texto de su encabezado (sin los `#`, recortado).
    - `aparicion: <n>` (opcional, con `item` o `encabezado`; entero ≥ 1, por defecto 1): cuál de los
      elementos con ese mismo texto, contando desde el principio de la nota.
  - Un pliegue que ya no corresponde a nada de la nota no pliega nada; Orbital lo quita la próxima
    vez que escribe el estado de esa nota. Nunca pliega otro elemento en su lugar.

  Cómo lo trata Orbital:

  - Lo lee en segundo plano al abrir la primera nota `.md` de la bóveda (no en el arranque).
  - Lo crea (y crea `.orbital/`) recién al plegar algo por primera vez.
  - Escritura atómica. Antes de escribir encima de un contenido que no escribió esta misma sesión de
    Orbital (el que había al abrir la bóveda, uno cambiado por fuera o uno dañado), lo copia a
    `.orbital/respaldo/<fecha>/.orbital/plegado.<hora>.yaml`. Plegar y desplegar muchas veces en la
    misma sesión no deja más copias.
  - Al escribir, quita las notas que ya no existen en la bóveda.
  - Si el archivo tiene errores (YAML inválido, claves desconocidas, una ruta inválida o repetida,
    un pliegue sin clave o con dos, `aparicion` no entero o menor que 1), Orbital avisa una vez,
    abre las notas desplegadas y no lo sobrescribe hasta que la usuaria pliegue o despliegue algo;
    ese cambio respalda primero el archivo dañado.
  - Si cambió en disco entre la lectura y una escritura (otra computadora, un editor), lo vuelve a
    leer y repite el cambio sobre lo nuevo. No hay vigilante: un cambio de afuera se ve la próxima
    vez que se abre la bóveda.
  - Renombrar o mover una nota o una carpeta desde Orbital actualiza las rutas; borrar desde Orbital
    quita las entradas (y «Deshacer» las repone). Lo que se borra o mueve por fuera se descarta al
    escribir.
  - El enfoque (ver solo un elemento) no se guarda.

## 10. Borrar y papelera

Borrar nunca es definitivo: el archivo va a la papelera del sistema y, si ese disco no tiene (tmpfs, FAT),
a `.orbital/papelera/<fecha>-<hora>/<ruta>`. En los dos casos se puede deshacer, y la línea de `index.md` se
repone tal como estaba.

## 11. Versionado

Este contrato tiene `format_version: 1`. Un cambio incompatible sube la versión y trae una guía de migración.

## 12. Contenido generado (005)

Texto del archivo que la vista expande al leer: el disco siempre tiene el fuente, y la generación es
solo de vista (no entra en la búsqueda ni en el grafo). En Lectura y Visual se muestra el contenido
generado; en Código queda el texto plano.

### Marcadores `[%clave]`

- Sintaxis compatible con Obsidian. La clave usa el mismo criterio de los campos inline (sin
  espacios, `:`, comillas ni delimitadores) y el cierre `]` va en la misma línea.
- No cuentan los que están dentro de código (cercas, en línea, bloques), enlaces, imágenes,
  frontmatter o escapados con `\`.
- Resolución, en orden: campo inline `clave:: valor` de la **misma sección** (si la clave se repite
  en la sección, gana el primero) → propiedad del frontmatter YAML → rótulo `«faltante»`.
- Cada encabezado ATX (`#` a `######`) abre una sección hasta el siguiente; el texto previo al
  primer encabezado es una sección propia.
- En notas `.html`, el inline del texto visible gana sobre el YAML del `<script
  type="application/yaml">`, y el bloque de props no genera marcadores.

### Codeblocks generados

- ` ```indice ` con `tag: #etiqueta` (comillas no necesarias aunque la etiqueta tenga `#`): lista las
  notas con esa etiqueta y sus hijas, anidada por jerarquía y ordenada alfabéticamente. **Solo
  vista**: nunca escribe enlaces en el disco y el grafo no la ve. Tag sin notas: aviso «sin notas
  con #tag». Las filas son clicables: una nota se abre; una etiqueta abre la búsqueda `tag:`.
- ` ```toc ` (también ` ```table-of-contents ` o ` ```tabla-de-contenidos `): tabla de contenidos
  de la nota, anidada por nivel; cada entrada lleva al encabezado y se actualiza sola al editar.
  El cuerpo son líneas `clave: valor` (no YAML estricto): un `# comentario` al final de la línea se
  ignora, y una opción vacía toma su valor por defecto. Opciones:

  | Opción | Por defecto | Qué hace |
  |---|---|---|
  | `title` | vacío | Texto (Markdown en línea) antes de la tabla; acá `#` es parte del texto |
  | `style` | `nestedList` | `nestedList` (`lista`): viñetas anidadas; `nestedOrderedList` (`lista-ordenada`): numerada; `inlineFirstLevel` (`linea-primer-nivel`): solo el primer nivel, en una línea separada por ` \| ` |
  | `minLevel` | `0` | Nivel desde el que se incluye; `0`: el menor nivel presente en la nota |
  | `maxLevel` | `0` | Nivel hasta el que se incluye; `0`: sin límite |
  | `include` | — | Expresión `/patrón/banderas` (`i`, `m`, `s`): solo los encabezados que coinciden. Gana sobre `exclude` |
  | `exclude` | — | Expresión `/patrón/banderas`: sin los encabezados que coinciden |
  | `includeLinks` | `true` | Entradas clicables; `false`: texto plano |
  | `hideWhenEmpty` | `false` | Sin encabezados: no se muestra nada (si no, un aviso) |

  Un encabezado que `include` o `exclude` deja afuera se lleva a sus subencabezados; los
  encabezados vacíos no entran. Un valor inválido (por ejemplo `style: arbol`) muestra el error en
  lugar de la tabla. Con la bandera `i`, un patrón escrito en minúsculas
  encuentra también las mayúsculas acentuadas (`/módulo/i` encuentra «MÓDULO»); uno con mayúsculas
  acentuadas solo las iguala en ASCII.
- ` ```cardlink ` (013): tarjeta de una URL. El cuerpo es YAML con `url` más los metadatos
  (`title`, `description`, `host`, `favicon`, `image`); Lectura lo muestra como tarjeta clicable y
  el archivo conserva el fuente. Compatible con el formato del plugin de Obsidian.

  ````markdown
  ```cardlink
  url: https://ejemplo.com/articulo
  title: El artículo
  description: Resumen del artículo.
  host: ejemplo.com
  ```
  ````

- Un codeblock `indice` sin `tag` o con YAML inválido se muestra como codeblock común.
- El contenido de transclusiones incrustadas (`![[…]]`) no se genera: se ve el codeblock fuente.

## 13. Etiquetas, campos y búsqueda (004)

### Etiquetas

- Una etiqueta es `#` seguido de letras, números, `_`, `-` o `/`, con al menos un carácter que no sea
  número (`#2026` no es etiqueta; `#v2` sí). No cuentan las que están dentro de código ni de enlaces.
- También son etiquetas los valores de la propiedad `tags` (sin `#`).
- Son jerárquicas con `/`: `#fisica/termo` es hija de `#fisica`. Buscar o elegir una etiqueta incluye a
  sus hijas.
- Renombrar una etiqueta cambia el texto y la propiedad `tags` de cada nota afectada (también las hijas),
  con GuardadoSeguro y «Deshacer». No toca etiquetas parecidas (`#fisicas` al renombrar `#fisica`); si
  el nombre nuevo ya existe, las dos se fusionan.

### Campos en línea `clave:: valor`

- Un campo es `clave:: valor` al comienzo de una línea o de un ítem de lista, fuera de código.
- Cuenta como propiedad de la nota para la búsqueda (`[clave]`, `[clave:valor]`) y queda disponible para
  las bases. Si la misma clave está en el bloque de propiedades, manda el bloque.
- Orbital no reescribe los campos: siguen siendo texto normal de la nota.

### Sintaxis de búsqueda

La búsqueda ignora mayúsculas y tildes, y **no** interpreta expresiones regulares.

| Escritura | Significado |
|-----------|-------------|
| `energia fisica` | Todas las palabras, en cualquier orden. |
| `"frase exacta"` | La frase tal cual. |
| `-termino` | Excluye las notas que lo tienen. |
| `a OR b` | Una u otra. |
| `tag:fisica` | Notas con la etiqueta o sus hijas. |
| `path:wiki/fisica` / `file:entropia` | Por carpeta o por nombre de archivo. |
| `line:(a b)` / `section:(a b)` | Todos los términos en la misma línea o en la misma sección. |
| `[clave]` / `[clave:valor]` | Por propiedades del bloque o campos `clave:: valor`. |

Una consulta mal escrita muestra un aviso con la columna del problema y no borra lo escrito. Los
resultados de `raw/` llevan la marca «fuente» y se pueden ocultar. Las búsquedas guardadas viven en
`.orbital/busquedas.yaml` (sección 9), nunca en las notas.

## Bases sobre notas (029)

Una base es un archivo `.base` de definición de vistas sobre notas Markdown/HTML. Los valores
siguen en las notas; el archivo de base guarda filtros, columnas, tipos esperados y orden.
El contrato completo está en [`specs/029-bases/contracts/formato-base.md`](../specs/029-bases/contracts/formato-base.md).

```yaml
format_version: 1
view: table
columns:
  - property: title
    label: Título
    type: text
  - property: precio
    label: Precio
    type: number
filters:
  - folder: wiki/libros
  - property: estado
    value: pendiente
sort:
  property: precio
  descending: false
```

`view` admite `table` y `cards`. Los filtros se combinan con «y»; una propiedad sin `value`
filtra por presencia. `folder` incluye subcarpetas, `tag` compara una etiqueta y `property`
compara un valor tipado. Las propiedades del bloque prevalecen sobre los campos inline.

Los tipos de columna son `auto`, `text`, `number`, `bool`, `date`, `datetime`, `link` y `list`.
Declarar un tipo no convierte datos existentes. `file.path` y `file.name` son campos derivados
de solo lectura. Ausente y nulo son estados distintos. Las filas no se guardan en la base.

Editar una celda actualiza la propiedad en la nota original con comprobación de hash y
GuardadoSeguro. Una definición inválida, una versión futura o campos desconocidos impiden
reescribir la base en silencio. `raw/` sigue siendo de solo lectura.

`![[x.base]]` incrusta el resultado sin copiarlo al archivo de la nota. El renombre de propiedades
previsualiza también las referencias en bases; ante valores diferentes de origen y destino,
la nota queda intacta y se informa el conflicto. Los cambios aplicados se pueden deshacer.
