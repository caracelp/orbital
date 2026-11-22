# Contribuir a Orbital

Gracias por el interés. Orbital es un proyecto chico, mantenido por una sola persona: estas pautas
existen para que cada cambio sea fácil de revisar y mantenga el proyecto coherente.

## Antes de empezar

- **Errores**: abrí un [issue](https://github.com/caracelp/orbital/issues/new/choose) con la
  plantilla de error (pasos, lo que esperabas y tu entorno).
- **Funciones nuevas o cambios grandes**: proponelos primero en un issue o en
  [discussions](https://github.com/caracelp/orbital/discussions). Así se acuerda el alcance antes
  de escribir código.
- **Seguridad**: no abras un issue público; seguí [SECURITY.md](SECURITY.md).

Al participar aceptás el [código de conducta](CODE_OF_CONDUCT.md).

## Principios

1. **Rendimiento primero.** Cada decisión pondera CPU, memoria y tiempo de arranque en la máquina
   de referencia (Intel Celeron N5100, sin AVX, con bóvedas de miles de notas):
   - El hilo principal de GTK nunca se bloquea: la E/S y el cálculo pesado van a otro hilo o a
     una tarea asíncrona.
   - Nada de `clone()` ni asignaciones innecesarias; carga perezosa cuando se puede.
   - Cada dependencia nueva se justifica por su costo de compilación y de tamaño del binario.
   - «Más rápido» se afirma con una medición (`cargo bench`, `hyperfine` o las mediciones
     integradas), no con una suposición.
   - Las compilaciones que se distribuyen no usan `target-cpu=native` ni `x86-64-v3`.
2. **El disco siempre tiene la verdad.** Las notas son archivos `.md` y `.html` comunes; nada
   atado a un formato propietario ni índices que no se puedan regenerar. Cualquier cambio en lo
   que se lee o escribe en la bóveda actualiza [docs/formato-boveda.md](docs/formato-boveda.md).
3. **Sin conexión.** Ninguna función usa la red sin que la persona la pida.
4. **Accesible.** Todo control tiene nombre accesible y se puede usar con el teclado.
5. **Licencias compatibles con GPL-3.0.** `cargo deny check` lo verifica. No se copia código sin
   licencia.

## Entorno

Rust estable y las bibliotecas de desarrollo de GTK 4, libadwaita 1.8 o más nueva,
GtkSourceView 5, WebKitGTK 6.0 y GStreamer. Los paquetes de cada distribución están en el
[README](README.md#desde-el-código).

```sh
cargo run                                  # compilar y abrir la última bóveda
cargo run -- <carpeta>                     # abrir una carpeta
cargo run -p orbital-vault --example generar-boveda -- /tmp/boveda 500   # bóveda de prueba
```

**Nunca** pruebes contra tu bóveda real: usá carpetas temporales. El generador rechaza
`~/Documentos` y las carpetas de `ORBITAL_BOVEDAS_PROTEGIDAS` (separadas por `:`).

Para no tocar tu configuración ni la instancia de Orbital que tengas abierta (la app es de
instancia única), corré las pruebas manuales con una configuración temporal:

```sh
XDG_CONFIG_HOME=$(mktemp -d) ORBITAL_INSTANCIA_PROPIA=1 cargo run -- /tmp/boveda
```

## Estructura

| Carpeta | Qué tiene |
|---|---|
| `src/` | La app GTK: ventana (`src/window/`, repartida por tema), editor (`src/editor/`), vistas y diálogos |
| `crates/orbital-vault/` | Núcleo sin GTK: formato de la bóveda, propiedades, enlaces, búsqueda, bases. Con pruebas y benchmarks |
| `crates/orbital-formulas/` | Fórmulas LaTeX, química y unidades |
| `crates/orbital-graficos/` | Gráficos 2D y 3D |
| `crates/orbital-diagramas/` | Diagramas Mermaid |
| `crates/orbital-drive/` | Sincronización con Google Drive, con un servidor falso para las pruebas |
| `tools/` | Pruebas GTK en proceso y scripts de verificación por AT-SPI |
| `build-aux/flatpak/` | Manifiesto y script del Flatpak |
| `docs/` | Documentación para usuarios |

## Verificar

Los cinco tienen que pasar antes de mandar un cambio (el CI corre los mismos):

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo deny check
tools/pruebas-gtk.sh            # pruebas GTK en proceso, una por proceso
```

- El núcleo se prueba solo con `cargo test -p orbital-vault`; sus benchmarks, con
  `cargo bench -p orbital-vault`.
- Si el cambio toca la interfaz, sumá una prueba GTK en proceso o una verificación por AT-SPI
  (`tools/verificar-*.sh`; cada una levanta su propia instancia y bóveda temporales). Las
  verificaciones por AT-SPI no envían teclas sintéticas.
- Si el cambio toca algo medido, adjuntá la medición antes y después (mediana de varias
  corridas, intercaladas).

## Estilo

- Interfaz, documentación y mensajes de commit en **español**; identificadores del código en
  inglés. Seguí la [GNOME HIG](https://developer.gnome.org/hig/) y preferí los widgets de
  libadwaita.
- Un `GtkTextView` va directo dentro de su `GtkScrolledWindow` (envuelto en otro widget, GTK
  recalcula el documento entero en cada tecla).
- Mensajes de commit: una primera línea corta que diga qué cambia; el porqué, en el cuerpo.
- Un pull request por tema, con la plantilla completa.

## Licencia

Orbital se publica bajo [GPL-3.0-or-later](LICENSE). Al contribuir aceptás que tu aporte se
publique bajo esa licencia.
