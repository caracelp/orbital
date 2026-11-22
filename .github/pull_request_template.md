<!-- Interfaz, documentación y mensajes de commit en español; identificadores del código en inglés.
Guía completa: CONTRIBUTING.md -->

## Qué cambia

<!-- Resumen del cambio y por qué. Si resuelve un issue: «Cierra #123». -->

## Cómo se verificó

<!-- Si toca la interfaz, una prueba GTK en proceso o una verificación por AT-SPI
(tools/verificar-*.sh). Si toca algo medido, la medición antes y después. -->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`
- [ ] `cargo deny check`
- [ ] `tools/pruebas-gtk.sh`
- [ ] Prueba manual o por AT-SPI (si aplica), con una bóveda temporal
- [ ] `docs/formato-boveda.md` actualizado (si cambia lo que se lee o escribe en la bóveda)

## Notas

<!-- Decisiones tomadas, mediciones, lo que quedó pendiente. -->
