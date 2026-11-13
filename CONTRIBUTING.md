# Contribuir a Orbital

Gracias por el interés. Antes de mandar cambios, una vuelta por estas reglas: son las que mantiene
el proyecto coherente.

## Principios

Los fundamentos están en [`.specify/memory/constitution.md`](.specify/memory/constitution.md).
Los dos que más se rompen:

1. **Rendimiento primero.** Toda decisión pondera CPU, memoria y tiempo de arranque en la máquina
   de referencia (Intel Celeron N5100, sin AVX, con una bóveda de miles de notas). El hilo principal
   de GTK nunca se bloquea. No se afirma «más rápido» sin medición (benchmarks o `hyperfine`).
2. **El disco siempre tiene la verdad.** Las notas son archivos `.md` y `.html` normales; nada atado
   a un formato propietario ni índices que no se puedan regenerar.

## Entorno

Rust estable y las bibliotecas de desarrollo de GTK4, libadwaita y WebKitGTK 6.0:

- **Arch:** `gtk4`, `libadwaita`, `webkitgtk-6.0`
- **Debian/Ubuntu:** los paquetes `-dev` correspondientes (`libwebkit2gtk-4.1-dev` incluye el de GTK4
  en versiones recientes; revisalo contra `pkg-config --exists webkitgtk-6.0`)

## Compilar y verificar

```sh
cargo run                                   # compilar y ejecutar
cargo fmt                                   # formato
cargo clippy --all-targets -- -D warnings   # sin avisos
cargo test                                  # núcleo, GTK en proceso y más
cargo deny check                            # licencias y vulnerabilidades
```

Los cuatro tienen que estar en verde antes de cerrar cualquier cambio. Si el cambio toca interfaz,
agregá la regresión AT-SPI correspondiente en `tools/verificar-NNN-us*.sh` (cada script levanta una
instancia y una bóveda temporales propias; nunca se toca la bóveda real del usuario ni se envían
teclas sintéticas).

Conocimiento acumulado sobre GTK y AT-SPI (no repetirlo): [`AGENTS.md`](AGENTS.md).

## Flujo de trabajo

- Las funciones no triviales siguen el flujo Spec Kit: `specs/NNN-nombre/` con
  `spec.md → plan.md → research.md → tasks.md`, más `quickstart.md` (resultados medidos) y
  `traspaso.md` (estado y lecciones). La cola de trabajo vigente está en
  [`specs/cola-autonoma.md`](specs/cola-autonoma.md).
- Cada especificación tiene su rama (`NNN-nombre`); al terminarla se fusiona en `main`
  (`git merge --no-ff`) y se borra, local y en GitHub.
- Interfaz, mensajes de commit y documentos en **español**; identificadores del código en inglés.
- Los mensajes de commit terminan con la línea `Co-Authored-By` del agente que los hizo.

## Reportar

Errores y pedidos de función van por [issues](https://github.com/caracelp/orbital/issues) con las
plantillas; preguntas y propuestas abiertas, en
[discussions](https://github.com/caracelp/orbital/discussions).

## Licencia

GPL-3.0-or-later. Toda contribución se publica bajo esa licencia.
