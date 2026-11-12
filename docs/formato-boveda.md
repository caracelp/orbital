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
- `.orbital/`: `config.yaml`, `respaldo/` y `papelera/` (solo se usa si el disco no tiene papelera del
  sistema, por ejemplo tmpfs o FAT). Ninguna otra herramienta debería escribir aquí. Borrarla no
  pierde contenido.

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
- Valores de `type` usados por Orbital: `atomo`, `sintesis`, `plantilla`, `diario`. Se aceptan otros.
- Las fechas con hora llevan desplazamiento explícito (OKF §5).
- Las claves desconocidas se conservan. Editar una propiedad no cambia el resto del bloque ni del archivo.

## 6. Enlaces

- Wikilinks: `[[nombre]]`, `[[nombre|alias]]`, `[[nombre#encabezado]]`. Se resuelven por nombre de archivo
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
- ` ```toc ` con `minLevel`/`maxLevel` opcionales (números, 0 = sin límite): índice de encabezados
  de la nota, anidado por nivel; cada entrada lleva al encabezado. Sin encabezados: aviso discreto.
- Un codeblock `indice` sin `tag` o con YAML inválido se muestra como codeblock común.
- El contenido de transclusiones incrustadas (`![[…]]`) no se genera: se ve el codeblock fuente.
