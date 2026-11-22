# Seguridad

## Versiones con soporte

Orbital está en desarrollo activo y no tiene ramas de mantenimiento: los arreglos de seguridad van
a `main` y a la próxima versión.

## Cómo reportar una vulnerabilidad

**No abras un issue público.** Usá el reporte privado de GitHub:
[Reportar una vulnerabilidad](https://github.com/caracelp/orbital/security/advisories/new)
(pestaña *Security* del repositorio → *Report a vulnerability*).

Incluí, si podés:

- qué parte de Orbital está afectada (por ejemplo, la vista de notas HTML, la importación de
  documentos o la sincronización con Google Drive) y la versión o el commit;
- los pasos para reproducirlo, con una bóveda o un archivo de ejemplo mínimo;
- el impacto que ves (lectura o escritura de archivos fuera de la bóveda, ejecución de código,
  filtración de credenciales…).

Vas a recibir una respuesta dentro de los 7 días. Cuando el arreglo esté publicado, el aviso se
hace público con el crédito para quien lo reportó, salvo que prefieras lo contrario.

## Alcance

Orbital trabaja con archivos que pueden venir de cualquier lado, así que estos puntos importan en
especial:

- Las notas HTML se muestran con WebKitGTK en una sesión efímera, sin JavaScript de la nota y con
  una política de contenido que bloquea toda carga externa.
- `raw/` es de solo lectura para Orbital. Fuera de la bóveda abierta, la app escribe solo en su
  configuración (`$XDG_CONFIG_HOME/orbital`), su caché (`$XDG_CACHE_HOME/orbital`), el estado de
  la sincronización (`$XDG_STATE_HOME/orbital`), la papelera del sistema y donde la persona elija
  exportar.
- Las credenciales de Google Drive (id y secreto del cliente, token de renovación) se guardan en el
  llavero del sistema (Secret Service), nunca en archivos de Orbital ni en la bóveda.

Los problemas de las bibliotecas del sistema (GTK, WebKitGTK, GStreamer) se reportan a sus
proyectos; si afectan a Orbital de una forma particular, contanos igual.
