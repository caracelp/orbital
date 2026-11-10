# Orbital

Aplicación de escritorio para Linux de toma de notas en Markdown y HTML, con herramientas de productividad.

Hecha en Rust con GTK4 y libadwaita.

## Formato de la bóveda

Las notas son archivos `.md` y `.html` normales, en una estructura `raw/` + `wiki/` compatible con
[Open Knowledge Format v0.2](https://github.com/GoogleCloudPlatform/open-knowledge-format). El formato
completo está en [docs/formato-boveda.md](docs/formato-boveda.md).

## Compilar y ejecutar

```sh
cargo run
```

## Licencia

GPL-3.0-or-later. Ver [LICENSE](LICENSE).
