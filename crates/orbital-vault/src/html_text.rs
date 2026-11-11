//! Extracción de texto de notas .html conservando saltos de línea (R5, spec 004).
//!
//! Extrae solo el contenido visible del `<body>`, descartando etiquetas, comentarios, `<script>`
//! y `<style>`. Decodifica las entidades HTML comunes y preserva cada salto de línea del archivo
//! original para que las líneas de las coincidencias correspondan byte a byte con el fuente.

use crate::html_edit::{body_range, tags};

/// Decodifica entidades HTML comunes (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&nbsp;`, numéricas).
pub fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            let mut entity = String::new();
            let mut found_semi = false;
            while let Some(&next) = chars.peek() {
                if next == ';' {
                    chars.next();
                    found_semi = true;
                    break;
                }
                if next.is_alphanumeric() || next == '#' {
                    entity.push(chars.next().unwrap());
                    if entity.len() > 10 {
                        break;
                    }
                } else {
                    break;
                }
            }
            if found_semi {
                match entity.as_str() {
                    "amp" => out.push('&'),
                    "lt" => out.push('<'),
                    "gt" => out.push('>'),
                    "quot" => out.push('"'),
                    "apos" => out.push('\''),
                    "nbsp" => out.push(' '),
                    s if s.starts_with("#x") || s.starts_with("#X") => {
                        if let Ok(cp) = u32::from_str_radix(&s[2..], 16)
                            && let Some(ch) = char::from_u32(cp)
                        {
                            out.push(ch);
                        } else {
                            out.push('&');
                            out.push_str(&entity);
                            out.push(';');
                        }
                    }
                    s if s.starts_with('#') => {
                        if let Ok(cp) = s[1..].parse::<u32>()
                            && let Some(ch) = char::from_u32(cp)
                        {
                            out.push(ch);
                        } else {
                            out.push('&');
                            out.push_str(&entity);
                            out.push(';');
                        }
                    }
                    _ => {
                        out.push('&');
                        out.push_str(&entity);
                        out.push(';');
                    }
                }
            } else {
                out.push('&');
                out.push_str(&entity);
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn push_newlines_only(s: &str, out: &mut String) {
    for c in s.chars() {
        if c == '\n' {
            out.push('\n');
        }
    }
}

fn push_text_strip_comments(s: &str, out: &mut String) {
    let mut i = 0;
    while i < s.len() {
        if s[i..].starts_with("<!--") {
            let end = s[i + 4..].find("-->").map_or(s.len(), |e| i + 4 + e + 3);
            push_newlines_only(&s[i..end], out);
            i = end;
        } else {
            let next_comment = s[i..].find("<!--").map_or(s.len(), |idx| i + idx);
            out.push_str(&decode_entities(&s[i..next_comment]));
            i = next_comment;
        }
    }
}

/// Extrae el texto visible del `<body>` de una nota HTML.
///
/// Conserva todos los saltos de línea del archivo fuente (incluyendo los del `<head>`,
/// etiquetas y scripts) para que el número de cada línea coincida exactamente con el original.
pub fn html_text(source: &str) -> String {
    let Some(body) = body_range(source) else {
        let mut out = String::new();
        push_newlines_only(source, &mut out);
        return out;
    };

    let mut out = String::with_capacity(source.len());

    // 1. Antes del <body>: solo conservar saltos de línea
    push_newlines_only(&source[..body.start], &mut out);

    // 2. Dentro del <body>: texto visible, decodificando entidades y salteando etiquetas/scripts
    let mut cursor = body.start;
    let mut in_raw_block = false;

    for tag in tags(source, body.start) {
        if tag.range.start >= body.end {
            break;
        }

        if tag.range.start > cursor {
            let text_slice = &source[cursor..tag.range.start];
            if in_raw_block {
                push_newlines_only(text_slice, &mut out);
            } else {
                push_text_strip_comments(text_slice, &mut out);
            }
        }

        push_newlines_only(&source[tag.range.clone()], &mut out);

        match tag.name.as_str() {
            "script" | "style" => in_raw_block = true,
            "/script" | "/style" => in_raw_block = false,
            _ => {}
        }

        cursor = tag.range.end.min(body.end);
    }

    if cursor < body.end {
        let text_slice = &source[cursor..body.end];
        if in_raw_block {
            push_newlines_only(text_slice, &mut out);
        } else {
            push_text_strip_comments(text_slice, &mut out);
        }
    }

    // 3. Después del <body>: solo conservar saltos de línea
    push_newlines_only(&source[body.end..], &mut out);

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_text_extracts_only_body_and_preserves_lines() {
        let html = "\
<!doctype html>
<html lang=\"es\">
<head>
  <meta charset=\"utf-8\">
  <title>Título en Head</title>
  <style>
    body { color: red; }
  </style>
</head>
<body>
  <h1>Encabezado Principal</h1>
  <!-- Comentario aquí -->
  <p>Párrafo con <b>negrita</b> &amp; <i>cursiva</i>.</p>
  <script>
    console.log(\"no indexar esto\");
  </script>
  <p>Última línea visible</p>
</body>
</html>";

        let text = html_text(html);

        // No debe contener contenido de head, style, script ni comentarios
        assert!(!text.contains("Título en Head"));
        assert!(!text.contains("color: red"));
        assert!(!text.contains("Comentario aquí"));
        assert!(!text.contains("console.log"));
        assert!(!text.contains("no indexar esto"));
        assert!(!text.contains("<h1>"));
        assert!(!text.contains("<b>"));

        // Debe contener el texto del body con entidades decodificadas
        assert!(text.contains("Encabezado Principal"));
        assert!(text.contains("Párrafo con negrita & cursiva."));
        assert!(text.contains("Última línea visible"));

        // Cantidad de saltos de línea debe ser idéntica
        let html_newlines = html.chars().filter(|&c| c == '\n').count();
        let text_newlines = text.chars().filter(|&c| c == '\n').count();
        assert_eq!(
            text_newlines, html_newlines,
            "La cantidad de saltos de línea debe ser exacta"
        );

        // Verificar coincidencia de líneas exactas (con split('\n'))
        let html_lines: Vec<&str> = html.split('\n').collect();
        let text_lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(html_lines.len(), text_lines.len());

        // Línea 11 (0-indexed: 10) es <h1>Encabezado Principal</h1>
        assert_eq!(text_lines[10].trim(), "Encabezado Principal");
        // Línea 13 (0-indexed: 12) es párrafo con &amp;
        assert_eq!(text_lines[12].trim(), "Párrafo con negrita & cursiva.");
        // Línea 17 (0-indexed: 16) es última línea visible
        assert_eq!(text_lines[16].trim(), "Última línea visible");
    }

    #[test]
    fn html_text_entity_decoding() {
        assert_eq!(decode_entities("Tom &amp; Jerry"), "Tom & Jerry");
        assert_eq!(decode_entities("&lt;div&gt;"), "<div>");
        assert_eq!(decode_entities("&quot;hola&quot;"), "\"hola\"");
        assert_eq!(decode_entities("don&#39;t"), "don't");
        assert_eq!(decode_entities("espacio&nbsp;duro"), "espacio duro");
        assert_eq!(decode_entities("letra &#65;"), "letra A");
        assert_eq!(decode_entities("hex &#x42;"), "hex B");
    }

    #[test]
    fn html_text_without_body() {
        let html = "<html><head><title>Sin body</title></head></html>";
        let text = html_text(html);
        assert!(text.trim().is_empty());
    }
}
