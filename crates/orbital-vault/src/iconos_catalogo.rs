//! Catálogo curado de íconos vectoriales de trazo (062) y paleta de colores.
//!
//! Todos los trazos están en una caja de 24×24 (compatibles con `gsk::Path::parse` y SVG `<path>`),
//! sin dependencias externas y 100 % sin conexión.

/// Un ícono vectorial del catálogo embebido.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IconoCatalogo {
    /// Identificador estable (minúsculas, guiones; se usa en `.orbital/iconos.yaml`, `icon:` y `:nombre:`).
    pub nombre: &'static str,
    /// Etiqueta legible en español para el selector y lector de pantalla.
    pub etiqueta: &'static str,
    /// Categoría para agrupar en el selector.
    pub categoria: &'static str,
    /// Palabras clave adicionales para buscar (español e inglés).
    pub claves: &'static str,
    /// Trazo SVG (`d="..."`) sobre un `viewBox="0 0 24 24"`.
    pub trazo: &'static str,
}

/// Un color sugerido en la paleta del selector de íconos (cumple buen contraste sobre base oscura).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorIcono {
    pub nombre: &'static str,
    pub hex: &'static str,
}

/// Paleta rápida de colores para íconos personalizados.
pub const COLORES: &[ColorIcono] = &[
    ColorIcono {
        nombre: "Rosa",
        hex: "#ff8cc6",
    },
    ColorIcono {
        nombre: "Rojo",
        hex: "#f7768e",
    },
    ColorIcono {
        nombre: "Naranja",
        hex: "#ff9e64",
    },
    ColorIcono {
        nombre: "Amarillo",
        hex: "#e0af68",
    },
    ColorIcono {
        nombre: "Verde",
        hex: "#9ece6a",
    },
    ColorIcono {
        nombre: "Turquesa",
        hex: "#73daca",
    },
    ColorIcono {
        nombre: "Celeste",
        hex: "#7dcfff",
    },
    ColorIcono {
        nombre: "Azul",
        hex: "#7aa2f7",
    },
    ColorIcono {
        nombre: "Violeta",
        hex: "#bb9af7",
    },
    ColorIcono {
        nombre: "Gris",
        hex: "#a9b1d6",
    },
];

/// Catálogo curado de íconos vectoriales de trazo 24×24.
pub const CATALOGO: &[IconoCatalogo] = &[
    // --- General y favoritos ---
    IconoCatalogo {
        nombre: "estrella",
        etiqueta: "Estrella",
        categoria: "General",
        claves: "star favorito destacado",
        trazo: "M12 2.5l2.7 5.5 6.1.9-4.4 4.3 1 6.1-5.4-2.9-5.4 2.9 1-6.1-4.4-4.3 6.1-.9z",
    },
    IconoCatalogo {
        nombre: "corazon",
        etiqueta: "Corazón",
        categoria: "General",
        claves: "heart amor favorito vida",
        trazo: "M12 20.5S4 15.2 4 9.5A4.5 4.5 0 0 1 12 6.7 4.5 4.5 0 0 1 20 9.5c0 5.7-8 11-8 11z",
    },
    IconoCatalogo {
        nombre: "casa",
        etiqueta: "Casa",
        categoria: "General",
        claves: "home inicio portada",
        trazo: "M3 10.5L12 3l9 7.5M5 9.5V20h14V9.5M9.5 20v-6h5v6",
    },
    IconoCatalogo {
        nombre: "foco",
        etiqueta: "Idea",
        categoria: "General",
        claves: "bulb idea luz pensamiento",
        trazo: "M9 18h6M10 21h4M12 3a6 6 0 0 0-3.5 10.9c.7.5 1 1.3 1 2.1h5c0-.8.3-1.6 1-2.1A6 6 0 0 0 12 3z",
    },
    IconoCatalogo {
        nombre: "fuego",
        etiqueta: "Fuego",
        categoria: "General",
        claves: "fire flame llama urgente",
        trazo: "M12 3c1 3 5 5 5 10a5 5 0 0 1-10 0c0-2 1-3.5 2-4.5c0 2 1 3 2 3c0-3-1-5 1-8.5z",
    },
    IconoCatalogo {
        nombre: "rayo",
        etiqueta: "Rayo",
        categoria: "General",
        claves: "bolt zap energia rapido",
        trazo: "M13 2L4 14h7l-1 8 9-12h-7z",
    },
    IconoCatalogo {
        nombre: "chispa",
        etiqueta: "Chispa",
        categoria: "General",
        claves: "sparkle magia brillo",
        trazo: "M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9zM18.5 15.5l.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8z",
    },
    IconoCatalogo {
        nombre: "diamante",
        etiqueta: "Diamante",
        categoria: "General",
        claves: "gem diamond joya valor",
        trazo: "M6 4h12l4 5-10 12L2 9zM2 9h20M12 21L8 9l4-5 4 5z",
    },
    IconoCatalogo {
        nombre: "corona",
        etiqueta: "Corona",
        categoria: "General",
        claves: "crown rey reina principal",
        trazo: "M3 18h18l-1.5-10-4.5 4-3-6-3 6-4.5-4zM4 21h16",
    },
    IconoCatalogo {
        nombre: "trofeo",
        etiqueta: "Trofeo",
        categoria: "General",
        claves: "trophy premio logro",
        trazo: "M8 4h8v6a4 4 0 0 1-8 0zM8 6H4v2a3 3 0 0 0 3 3h1M16 6h4v2a3 3 0 0 1-3 3h-1M12 14v5M8 19h8",
    },
    IconoCatalogo {
        nombre: "diana",
        etiqueta: "Objetivo",
        categoria: "General",
        claves: "target meta objetivo",
        trazo: "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18zM12 7a5 5 0 1 0 0 10a5 5 0 1 0 0-10zM12 11a1 1 0 1 0 0 2a1 1 0 1 0 0-2z",
    },
    IconoCatalogo {
        nombre: "bandera",
        etiqueta: "Bandera",
        categoria: "General",
        claves: "flag hito marca",
        trazo: "M5 3v18M5 4h12l-2.5 4.5L17 13H5",
    },
    IconoCatalogo {
        nombre: "chinche",
        etiqueta: "Fijado",
        categoria: "General",
        claves: "pin fijar anclar",
        trazo: "M9 4h6l-1 5 3 3v2H7v-2l3-3zM12 14v7",
    },
    IconoCatalogo {
        nombre: "campana",
        etiqueta: "Campana",
        categoria: "General",
        claves: "bell aviso alerta recordatorio",
        trazo: "M6 16h12l-1.5-2.5V10a4.5 4.5 0 0 0-9 0v3.5zM10 19a2 2 0 0 0 4 0",
    },
    // --- Notas, carpetas y archivos ---
    IconoCatalogo {
        nombre: "carpeta",
        etiqueta: "Carpeta",
        categoria: "Archivos",
        claves: "folder directorio",
        trazo: "M3 6h7l2 2h9v11H3z",
    },
    IconoCatalogo {
        nombre: "carpeta-abierta",
        etiqueta: "Carpeta abierta",
        categoria: "Archivos",
        claves: "folder-open",
        trazo: "M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v1H6l-3 9zM6 10h15l-2 9H3z",
    },
    IconoCatalogo {
        nombre: "nota",
        etiqueta: "Nota",
        categoria: "Archivos",
        claves: "file note documento",
        trazo: "M6 3h8l5 5v13H6zM14 3v5h5M9 13h7M9 17h7",
    },
    IconoCatalogo {
        nombre: "cuaderno",
        etiqueta: "Cuaderno",
        categoria: "Archivos",
        claves: "notebook diario libreta",
        trazo: "M6 3h12a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1zM9 3v18M3 8h2M3 12h2M3 16h2",
    },
    IconoCatalogo {
        nombre: "libro",
        etiqueta: "Libro",
        categoria: "Archivos",
        claves: "book lectura biblioteca",
        trazo: "M5 4.5A1.5 1.5 0 0 1 6.5 3H19v15H6.5A1.5 1.5 0 0 0 5 19.5zM5 19.5A1.5 1.5 0 0 0 6.5 21H19M9 7h6",
    },
    IconoCatalogo {
        nombre: "libro-abierto",
        etiqueta: "Libro abierto",
        categoria: "Archivos",
        claves: "book-open estudio",
        trazo: "M2 5h6a4 4 0 0 1 4 4v11a3 3 0 0 0-3-3H2zM22 5h-6a4 4 0 0 0-4 4v11a3 3 0 0 1 3-3h7z",
    },
    IconoCatalogo {
        nombre: "archivo",
        etiqueta: "Archivador",
        categoria: "Archivos",
        claves: "archive caja guardar",
        trazo: "M3 4h18v4H3zM5 8v12h14V8M10 12h4",
    },
    IconoCatalogo {
        nombre: "marcador",
        etiqueta: "Marcador",
        categoria: "Archivos",
        claves: "bookmark guardado",
        trazo: "M6 3h12v18l-6-4-6 4z",
    },
    IconoCatalogo {
        nombre: "etiqueta",
        etiqueta: "Etiqueta",
        categoria: "Archivos",
        claves: "tag label",
        trazo: "M20.59 13.41l-7.17 7.17a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82zM7 7h.01",
    },
    IconoCatalogo {
        nombre: "lapiz",
        etiqueta: "Lápiz",
        categoria: "Archivos",
        claves: "pencil edit escribir",
        trazo: "M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4",
    },
    IconoCatalogo {
        nombre: "pluma",
        etiqueta: "Pluma",
        categoria: "Archivos",
        claves: "pen escritura autor",
        trazo: "M20 4c-6 0-11 4-13 10l-3 6 6-3c6-2 10-7 10-13zM14 10l-7 7",
    },
    IconoCatalogo {
        nombre: "clip",
        etiqueta: "Adjunto",
        categoria: "Archivos",
        claves: "paperclip attach",
        trazo: "M21 11.5l-9.2 9.2a5 5 0 0 1-7.1-7.1l9.2-9.2a3.3 3.3 0 0 1 4.7 4.7l-9.2 9.2a1.7 1.7 0 0 1-2.4-2.4l8.5-8.5",
    },
    IconoCatalogo {
        nombre: "enlace",
        etiqueta: "Enlace",
        categoria: "Archivos",
        claves: "link url conexion",
        trazo: "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1",
    },
    IconoCatalogo {
        nombre: "lista",
        etiqueta: "Lista",
        categoria: "Archivos",
        claves: "list checklist indice",
        trazo: "M9 6h11M9 12h11M9 18h11M4.5 6h.5M4.5 12h.5M4.5 18h.5",
    },
    IconoCatalogo {
        nombre: "calendario",
        etiqueta: "Calendario",
        categoria: "Archivos",
        claves: "calendar fecha diario agenda",
        trazo: "M5 5h14a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1zM4 10h16M8 3v4M16 3v4",
    },
    IconoCatalogo {
        nombre: "reloj",
        etiqueta: "Reloj",
        categoria: "Archivos",
        claves: "clock time tiempo",
        trazo: "M12 4a8 8 0 1 0 8 8 8 8 0 0 0-8-8zm0 4v4l2.5 2.5",
    },
    // --- Proyectos, código y tecnología ---
    IconoCatalogo {
        nombre: "cohete",
        etiqueta: "Cohete",
        categoria: "Proyectos",
        claves: "rocket proyecto lanzamiento",
        trazo: "M12 2c4 2 6 6 6 11l-4 2-2 4-2-4-4-2c0-5 2-9 6-11zM12 8a1.5 1.5 0 1 0 0 3 1.5 1.5 0 1 0 0-3zM6 17l-2 3 3-1M18 17l2 3-3-1",
    },
    IconoCatalogo {
        nombre: "codigo",
        etiqueta: "Código",
        categoria: "Proyectos",
        claves: "code dev programacion",
        trazo: "M8 8l-4 4 4 4m8-8 4 4-4 4M14 4l-4 16",
    },
    IconoCatalogo {
        nombre: "terminal",
        etiqueta: "Terminal",
        categoria: "Proyectos",
        claves: "terminal consola cli shell",
        trazo: "M4 5h16v14H4zM8 10l3 2.5L8 15M13 15h4",
    },
    IconoCatalogo {
        nombre: "base-datos",
        etiqueta: "Base de datos",
        categoria: "Proyectos",
        claves: "database db datos",
        trazo: "M4 5a8 3 0 1 0 16 0a8 3 0 1 0 -16 0zM4 5v14c0 3 16 3 16 0V5M4 12c0 3 16 3 16 0",
    },
    IconoCatalogo {
        nombre: "engranaje",
        etiqueta: "Engranaje",
        categoria: "Proyectos",
        claves: "gear settings config sistema",
        trazo: "M9 12a3 3 0 1 0 6 0a3 3 0 1 0 -6 0zM12 2v3m0 14v3M2 12h3m14 0h3M4.9 4.9 7 7m10 10 2.1 2.1m0-14.2L17 7M7 17l-2.1 2.1",
    },
    IconoCatalogo {
        nombre: "herramienta",
        etiqueta: "Herramienta",
        categoria: "Proyectos",
        claves: "wrench tool taller",
        trazo: "M14.7 6.3a4 4 0 0 0-5.4 5.4L4 17l3 3 5.3-5.3a4 4 0 0 0 5.4-5.4l-2.3 2.3-3-3z",
    },
    IconoCatalogo {
        nombre: "bicho",
        etiqueta: "Error",
        categoria: "Proyectos",
        claves: "bug error depurar",
        trazo: "M9 9h6v7a3 3 0 0 1-6 0zM10 9V7a2 2 0 0 1 4 0v2M5 10h4M15 10h4M4 14h5M15 14h5M6 18l3-1.5M18 18l-3-1.5",
    },
    IconoCatalogo {
        nombre: "red",
        etiqueta: "Red",
        categoria: "Proyectos",
        claves: "graph network grafo nodos",
        trazo: "M10 4a2 2 0 1 0 4 0a2 2 0 1 0 -4 0zM3 19a2 2 0 1 0 4 0a2 2 0 1 0 -4 0zM17 19a2 2 0 1 0 4 0a2 2 0 1 0 -4 0zM11 6l-5 11m7-11 5 11M7 19h10",
    },
    IconoCatalogo {
        nombre: "caja",
        etiqueta: "Paquete",
        categoria: "Proyectos",
        claves: "box package modulo",
        trazo: "M12 3l8 4.5v9L12 21l-8-4.5v-9zM4 7.5L12 12l8-4.5M12 12v9",
    },
    IconoCatalogo {
        nombre: "grafico",
        etiqueta: "Gráfico",
        categoria: "Proyectos",
        claves: "chart stats metricas finanzas",
        trazo: "M4 20h16M7 20v-7M12 20V9M17 20v-4",
    },
    IconoCatalogo {
        nombre: "candado",
        etiqueta: "Candado",
        categoria: "Proyectos",
        claves: "lock privado seguridad",
        trazo: "M6 11h12v9H6zM8 11V8a4 4 0 0 1 8 0v3M12 15v2",
    },
    IconoCatalogo {
        nombre: "llave",
        etiqueta: "Llave",
        categoria: "Proyectos",
        claves: "key clave acceso",
        trazo: "M7.5 14a4.5 4.5 0 1 0 0-9a4.5 4.5 0 1 0 0 9zM11.5 11.5L21 21M17 17l2-2M15 15l2-2",
    },
    IconoCatalogo {
        nombre: "escudo",
        etiqueta: "Escudo",
        categoria: "Proyectos",
        claves: "shield defensa seguridad",
        trazo: "M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z",
    },
    // --- Ciencia y estudio ---
    IconoCatalogo {
        nombre: "atomo",
        etiqueta: "Átomo",
        categoria: "Ciencia",
        claves: "atom fisica ciencia orbital",
        trazo: "M12 12m-1.5 0a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0 -3 0M12 4c5 0 8 3.6 8 8s-3 8-8 8-8-3.6-8-8 3-8 8-8zM5 8c3-2 11 4 14 8M19 8c-3-2-11 4-14 8",
    },
    IconoCatalogo {
        nombre: "matraz",
        etiqueta: "Química",
        categoria: "Ciencia",
        claves: "flask quimica laboratorio",
        trazo: "M9 3h6M10 3v6l-5 9a2 2 0 0 0 1.7 3h10.6a2 2 0 0 0 1.7-3l-5-9V3M7 16h10",
    },
    IconoCatalogo {
        nombre: "graduacion",
        etiqueta: "Estudio",
        categoria: "Ciencia",
        claves: "graduation universidad clase curso",
        trazo: "M2 9l10-5 10 5-10 5zM6 11v5c0 2 3 3.5 6 3.5s6-1.5 6-3.5v-5M22 9v6",
    },
    IconoCatalogo {
        nombre: "cerebro",
        etiqueta: "Mente",
        categoria: "Ciencia",
        claves: "brain psicologia memoria pensamiento",
        trazo: "M12 5a3 3 0 0 0-5.5 1.5A3 3 0 0 0 5 12a3 3 0 0 0 2 5.2A3 3 0 0 0 12 18V5zM12 5a3 3 0 0 1 5.5 1.5A3 3 0 0 1 19 12a3 3 0 0 1-2 5.2A3 3 0 0 1 12 18",
    },
    IconoCatalogo {
        nombre: "planeta",
        etiqueta: "Planeta",
        categoria: "Ciencia",
        claves: "planet astronomia espacio mundo",
        trazo: "M12 6a6 6 0 1 0 0 12a6 6 0 1 0 0-12zM3 15c3 1 15-3 18-6",
    },
    IconoCatalogo {
        nombre: "globo",
        etiqueta: "Mundo",
        categoria: "Ciencia",
        claves: "globe web idioma geografia",
        trazo: "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18zM3 12h18M12 3c2.5 2.5 4 5.5 4 9s-1.5 6.5-4 9c-2.5-2.5-4-5.5-4-9s1.5-6.5 4-9z",
    },
    IconoCatalogo {
        nombre: "brujula",
        etiqueta: "Brújula",
        categoria: "Ciencia",
        claves: "compass explorar navegacion",
        trazo: "M3 12a9 9 0 1 0 18 0a9 9 0 1 0 -18 0zM16.2 7.8l-2.6 5.8-5.8 2.6 2.6-5.8z",
    },
    IconoCatalogo {
        nombre: "mapa",
        etiqueta: "Mapa",
        categoria: "Ciencia",
        claves: "map ubicacion viaje",
        trazo: "M3 6l6-2 6 2 6-2v14l-6 2-6-2-6 2zM9 4v14M15 6v14",
    },
    IconoCatalogo {
        nombre: "balanza",
        etiqueta: "Balanza",
        categoria: "Ciencia",
        claves: "scale derecho filosofia justicia",
        trazo: "M12 4v16M7 20h10M4 7h16M6 7l-2 6h4zM18 7l-2 6h4z",
    },
    IconoCatalogo {
        nombre: "raiz",
        etiqueta: "Matemática",
        categoria: "Ciencia",
        claves: "math formula calculo",
        trazo: "M3 13h3l3 6 4-14h8",
    },
    // --- Arte, medios y creatividad ---
    IconoCatalogo {
        nombre: "musica",
        etiqueta: "Música",
        categoria: "Arte",
        claves: "music audio cancion",
        trazo: "M9 18a2.5 2.5 0 1 1-5 0 2.5 2.5 0 1 1 5 0zM20 16a2.5 2.5 0 1 1-5 0 2.5 2.5 0 1 1 5 0zM9 18V6l11-2v12",
    },
    IconoCatalogo {
        nombre: "paleta",
        etiqueta: "Paleta",
        categoria: "Arte",
        claves: "palette pintura diseno color",
        trazo: "M12 3a9 9 0 1 0 0 18h1.5a2 2 0 0 0 1.4-3.4A1.8 1.8 0 0 1 16 14.5h2a3 3 0 0 0 3-3C21 6.8 17 3 12 3zM7.5 10.5h.01M10.5 7.5h.01M14.5 7.5h.01M16.5 10.5h.01",
    },
    IconoCatalogo {
        nombre: "imagen",
        etiqueta: "Imagen",
        categoria: "Arte",
        claves: "image foto galeria",
        trazo: "M5 4h14a2 2 0 0 1 2 2v12a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-12a2 2 0 0 1 2 -2zM8 9a1 1 0 1 0 2 0a1 1 0 1 0 -2 0zM4 18l5-5 4 4 3-3 4 4",
    },
    IconoCatalogo {
        nombre: "camara",
        etiqueta: "Cámara",
        categoria: "Arte",
        claves: "camera fotografia",
        trazo: "M4 8h3l2-3h6l2 3h3v11H4zM12 10.5a3 3 0 1 0 0 6a3 3 0 1 0 0-6z",
    },
    IconoCatalogo {
        nombre: "video",
        etiqueta: "Video",
        categoria: "Arte",
        claves: "video cine pelicula",
        trazo: "M3 7h12v10H3zM15 10l6-3v10l-6-3",
    },
    IconoCatalogo {
        nombre: "microfono",
        etiqueta: "Micrófono",
        categoria: "Arte",
        claves: "mic voz podcast grabacion",
        trazo: "M9 6a3 3 0 0 1 6 0v5a3 3 0 0 1-6 0zM6 11a6 6 0 0 0 12 0M12 17v4M9 21h6",
    },
    IconoCatalogo {
        nombre: "dado",
        etiqueta: "Juego",
        categoria: "Arte",
        claves: "dice rol juego ocio",
        trazo: "M5 4h14a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1zM8.5 8.5h.01M15.5 8.5h.01M12 12h.01M8.5 15.5h.01M15.5 15.5h.01",
    },
    // --- Naturaleza y cotidiano ---
    IconoCatalogo {
        nombre: "sol",
        etiqueta: "Sol",
        categoria: "Naturaleza",
        claves: "sun dia clima verano",
        trazo: "M12 8a4 4 0 1 0 0 8a4 4 0 1 0 0-8zM12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M19.1 4.9L17 7M7 17l-2.1 2.1",
    },
    IconoCatalogo {
        nombre: "luna",
        etiqueta: "Luna",
        categoria: "Naturaleza",
        claves: "moon noche sueno",
        trazo: "M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5z",
    },
    IconoCatalogo {
        nombre: "nube",
        etiqueta: "Nube",
        categoria: "Naturaleza",
        claves: "cloud drive sincronizacion",
        trazo: "M7 18h10a4 4 0 0 0 .8-7.9A5.5 5.5 0 0 0 7 8.5 4 4 0 0 0 7 18z",
    },
    IconoCatalogo {
        nombre: "hoja",
        etiqueta: "Hoja",
        categoria: "Naturaleza",
        claves: "leaf planta jardin naturaleza",
        trazo: "M5 20C5 10 11 4 20 4c0 9-6 15-15 16zM5 20l9-9",
    },
    IconoCatalogo {
        nombre: "flor",
        etiqueta: "Flor",
        categoria: "Naturaleza",
        claves: "flower botanica jardin",
        trazo: "M12 10a2 2 0 1 0 0 4a2 2 0 1 0 0-4zM12 4a2.5 2.5 0 0 1 2.5 2.5c0 1.4-2.5 3.5-2.5 3.5S9.5 7.9 9.5 6.5A2.5 2.5 0 0 1 12 4zM12 20a2.5 2.5 0 0 1-2.5-2.5c0-1.4 2.5-3.5 2.5-3.5s2.5 2.1 2.5 3.5A2.5 2.5 0 0 1 12 20zM4 12a2.5 2.5 0 0 1 2.5-2.5c1.4 0 3.5 2.5 3.5 2.5s-2.1 2.5-3.5 2.5A2.5 2.5 0 0 1 4 12zM20 12a2.5 2.5 0 0 1-2.5 2.5c-1.4 0-3.5-2.5-3.5-2.5s2.1-2.5 3.5-2.5A2.5 2.5 0 0 1 20 12z",
    },
    IconoCatalogo {
        nombre: "montana",
        etiqueta: "Montaña",
        categoria: "Naturaleza",
        claves: "mountain cumbre aventura",
        trazo: "M3 19l6-11 4 7 3-4 5 8z",
    },
    IconoCatalogo {
        nombre: "cafe",
        etiqueta: "Café",
        categoria: "Naturaleza",
        claves: "coffee pausa cocina",
        trazo: "M4 9h12v6a4 4 0 0 1-4 4H8a4 4 0 0 1-4-4zM16 10h2.5a2 2 0 0 1 0 4H16M7 5v2M11 5v2",
    },
    IconoCatalogo {
        nombre: "avion",
        etiqueta: "Viaje",
        categoria: "Naturaleza",
        claves: "plane travel viaje",
        trazo: "M3 12l18-7-4 15-5-5-4 3 1-5z",
    },
    // --- Personas y estados ---
    IconoCatalogo {
        nombre: "usuario",
        etiqueta: "Persona",
        categoria: "Personas",
        claves: "user person contacto perfil",
        trazo: "M12 6a3.5 3.5 0 1 0 0 7a3.5 3.5 0 1 0 0-7zM5 20a7 7 0 0 1 14 0",
    },
    IconoCatalogo {
        nombre: "usuarios",
        etiqueta: "Equipo",
        categoria: "Personas",
        claves: "users team grupo comunidad",
        trazo: "M9 7a3 3 0 1 0 0 6a3 3 0 1 0 0-6zM3 19a6 6 0 0 1 12 0M16 8a2.5 2.5 0 1 1 0 5M17 19a5 5 0 0 0-2-4",
    },
    IconoCatalogo {
        nombre: "mensaje",
        etiqueta: "Mensaje",
        categoria: "Personas",
        claves: "chat comment comentario reunion",
        trazo: "M4 5h16v11H8l-4 4z",
    },
    IconoCatalogo {
        nombre: "correo",
        etiqueta: "Correo",
        categoria: "Personas",
        claves: "mail email carta",
        trazo: "M3 6h18v12H3zM3 7l9 6 9-6",
    },
    IconoCatalogo {
        nombre: "ojo",
        etiqueta: "Vista",
        categoria: "Personas",
        claves: "eye revisar observar",
        trazo: "M2 12s4-6 10-6 10 6 10 6-4 6-10 6S2 12 2 12zM12 9.5a2.5 2.5 0 1 0 0 5a2.5 2.5 0 1 0 0-5z",
    },
    IconoCatalogo {
        nombre: "check",
        etiqueta: "Listo",
        categoria: "Estados",
        claves: "check ok hecho completo",
        trazo: "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18zM8 12l3 3 5-5",
    },
    IconoCatalogo {
        nombre: "alerta",
        etiqueta: "Alerta",
        categoria: "Estados",
        claves: "warning atencion cuidado",
        trazo: "M12 4L2.5 20h19zM12 10v4.5M12 17.5v.5",
    },
    IconoCatalogo {
        nombre: "info",
        etiqueta: "Información",
        categoria: "Estados",
        claves: "info detalle nota",
        trazo: "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18zM12 11v6M12 7.5v.5",
    },
    IconoCatalogo {
        nombre: "pregunta",
        etiqueta: "Pregunta",
        categoria: "Estados",
        claves: "question duda ayuda",
        trazo: "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18zM9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.7.3-1 .9-1 1.7M12 16.5v.5",
    },
    IconoCatalogo {
        nombre: "cruz",
        etiqueta: "Cerrado",
        categoria: "Estados",
        claves: "close cancel error",
        trazo: "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18zM9 9l6 6M15 9l-6 6",
    },
];

/// Busca un ícono del catálogo embebido por su nombre (sin distinguir mayúsculas).
pub fn buscar(nombre: &str) -> Option<&'static IconoCatalogo> {
    let n = nombre.trim().to_ascii_lowercase();
    CATALOGO.iter().find(|i| i.nombre == n)
}

/// Genera un documento SVG limpio de 24×24 para un trazo del catálogo (para exportar HTML o `GIcon`).
pub fn svg_de_trazo(trazo: &str, color: &str, etiqueta: Option<&str>) -> String {
    let aria = match etiqueta {
        Some(e) => format!(" role=\"img\" aria-label=\"{}\"", escapar_attr(e)),
        None => " aria-hidden=\"true\"".to_owned(),
    };
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" width=\"16\" height=\"16\" fill=\"none\" stroke=\"{color}\" stroke-width=\"1.8\" stroke-linecap=\"round\" stroke-linejoin=\"round\"{aria}><path d=\"{trazo}\"/></svg>"
    )
}

fn escapar_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Valida si `hex` es un color `#rgb` o `#rrggbb` válido y devuelve la forma normalizada `#rrggbb`.
pub fn normalizar_color(raw: &str) -> Option<String> {
    let s = raw.trim();
    let hex = s.strip_prefix('#')?;
    if hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some(format!("#{}", hex.to_ascii_lowercase()));
    }
    if hex.len() == 3 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        let b = hex.as_bytes();
        let out = format!(
            "#{}{}{}{}{}{}",
            (b[0] as char).to_ascii_lowercase(),
            (b[0] as char).to_ascii_lowercase(),
            (b[1] as char).to_ascii_lowercase(),
            (b[1] as char).to_ascii_lowercase(),
            (b[2] as char).to_ascii_lowercase(),
            (b[2] as char).to_ascii_lowercase(),
        );
        return Some(out);
    }
    None
}
