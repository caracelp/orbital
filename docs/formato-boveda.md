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
