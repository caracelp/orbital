//! Analizador de consultas de búsqueda (R6, spec 004).

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Atom {
    Word(String),
    Phrase(String),
    Tag(String),
    Path(String),
    File(String),
    Line(Box<Query>),
    Section(Box<Query>),
    Prop { key: String, value: Option<String> },
    Group(Box<Query>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clause {
    pub negated: bool,
    pub atom: Atom,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    /// Grupos AND separados por OR (disyunción de conjunciones).
    pub any_of: Vec<Vec<Clause>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryError {
    /// Posición en bytes donde ocurrió el error.
    pub at: usize,
    /// Mensaje descriptivo en español.
    pub message: String,
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Error en la columna {}: {}", self.at + 1, self.message)
    }
}

impl std::error::Error for QueryError {}

/// Parsea una consulta de búsqueda en texto plano según la gramática de R6.
pub fn parse_query(text: &str) -> Result<Query, QueryError> {
    let mut parser = Parser::new(text);
    let query = parser.parse_query(0)?;
    parser.skip_whitespace();
    if !parser.is_eof() {
        return Err(QueryError {
            at: parser.pos,
            message: "Paréntesis de cierre inesperado".to_string(),
        });
    }
    Ok(query)
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn remaining(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn skip_whitespace(&mut self) {
        while !self.is_eof() {
            let ch = self.remaining().chars().next().unwrap();
            if ch.is_whitespace() {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn starts_with(&self, prefix: &str) -> bool {
        self.remaining().starts_with(prefix)
    }

    fn parse_query(&mut self, depth: usize) -> Result<Query, QueryError> {
        let mut any_of = Vec::new();
        let mut current_and = Vec::new();

        self.skip_whitespace();
        if self.is_eof() {
            return Err(QueryError {
                at: 0,
                message: "Consulta vacía".to_string(),
            });
        }

        while !self.is_eof() {
            self.skip_whitespace();
            if self.is_eof() {
                break;
            }

            // Paréntesis de cierre
            if self.starts_with(")") {
                if depth == 0 {
                    return Err(QueryError {
                        at: self.pos,
                        message: "Paréntesis de cierre inesperado".to_string(),
                    });
                }
                break;
            }

            // Operador OR
            if self.starts_with("OR") {
                let after = self.pos + 2;
                let is_boundary = after >= self.input.len()
                    || self.input[after..].starts_with(char::is_whitespace)
                    || self.input[after..].starts_with(')');
                if is_boundary {
                    let or_pos = self.pos;
                    self.pos += 2;
                    if current_and.is_empty() {
                        return Err(QueryError {
                            at: or_pos,
                            message: "Falta término antes de OR".to_string(),
                        });
                    }
                    any_of.push(std::mem::take(&mut current_and));
                    continue;
                }
            }

            let clause = self.parse_clause(depth)?;
            current_and.push(clause);
        }

        if !current_and.is_empty() {
            any_of.push(current_and);
        }

        if any_of.is_empty() || any_of.iter().all(|g| g.is_empty()) {
            return Err(QueryError {
                at: self.pos,
                message: "Consulta vacía".to_string(),
            });
        }

        Ok(Query { any_of })
    }

    fn parse_clause(&mut self, depth: usize) -> Result<Clause, QueryError> {
        self.skip_whitespace();

        let negated = if self.starts_with("-")
            && self.remaining().len() > 1
            && !self.remaining()[1..].starts_with(char::is_whitespace)
        {
            self.pos += 1;
            true
        } else {
            false
        };

        let atom = self.parse_atom(depth)?;
        Ok(Clause { negated, atom })
    }

    fn parse_atom(&mut self, depth: usize) -> Result<Atom, QueryError> {
        self.skip_whitespace();
        let start = self.pos;

        // 1. Grupo entre paréntesis: ( ... )
        if self.starts_with("(") {
            self.pos += 1;
            let inner = self.parse_query(depth + 1)?;
            self.skip_whitespace();
            if !self.starts_with(")") {
                return Err(QueryError {
                    at: start,
                    message: "Paréntesis sin cerrar".to_string(),
                });
            }
            self.pos += 1;
            return Ok(Atom::Group(Box::new(inner)));
        }

        // 2. Frase entre comillas: " ... "
        if self.starts_with("\"") {
            self.pos += 1;
            if let Some(rel_end) = self.remaining().find('"') {
                let phrase = &self.remaining()[..rel_end];
                self.pos += rel_end + 1;
                return Ok(Atom::Phrase(phrase.to_string()));
            } else {
                return Err(QueryError {
                    at: start,
                    message: "Comillas sin cerrar".to_string(),
                });
            }
        }

        // 3. Propiedad entre corchetes: [clave] o [clave:valor]
        if self.starts_with("[") {
            self.pos += 1;
            if let Some(rel_end) = self.remaining().find(']') {
                let inside = &self.remaining()[..rel_end];
                self.pos += rel_end + 1;
                if let Some((k, v)) = inside.split_once(':') {
                    return Ok(Atom::Prop {
                        key: k.trim().to_string(),
                        value: Some(v.trim().to_string()),
                    });
                } else {
                    return Ok(Atom::Prop {
                        key: inside.trim().to_string(),
                        value: None,
                    });
                }
            } else {
                return Err(QueryError {
                    at: start,
                    message: "Corchete sin cerrar".to_string(),
                });
            }
        }

        // 4. line:( ... )
        if self.starts_with("line:(") {
            let open_paren = self.pos + 5;
            self.pos += 6; // past "line:("
            let inner = self.parse_query(depth + 1)?;
            self.skip_whitespace();
            if !self.starts_with(")") {
                return Err(QueryError {
                    at: open_paren,
                    message: "Paréntesis sin cerrar en line:".to_string(),
                });
            }
            self.pos += 1;
            return Ok(Atom::Line(Box::new(inner)));
        }

        // 5. section:( ... )
        if self.starts_with("section:(") {
            let open_paren = self.pos + 8;
            self.pos += 9; // past "section:("
            let inner = self.parse_query(depth + 1)?;
            self.skip_whitespace();
            if !self.starts_with(")") {
                return Err(QueryError {
                    at: open_paren,
                    message: "Paréntesis sin cerrar en section:".to_string(),
                });
            }
            self.pos += 1;
            return Ok(Atom::Section(Box::new(inner)));
        }

        // 6. tag:
        if self.starts_with("tag:") {
            self.pos += 4;
            let token = self.read_token();
            let tag = token.trim_start_matches('#');
            return Ok(Atom::Tag(tag.to_string()));
        }

        // 7. path:
        if self.starts_with("path:") {
            self.pos += 5;
            let token = self.read_token();
            return Ok(Atom::Path(token.to_string()));
        }

        // 8. file:
        if self.starts_with("file:") {
            self.pos += 5;
            let token = self.read_token();
            return Ok(Atom::File(token.to_string()));
        }

        // 9. Comprobar si parece un operador desconocido con ':'
        let token = self.read_token();
        if let Some((op, _)) = token.split_once(':') {
            // Si es URL (http:// o https:// o contiene ://), no es operador
            if !token.contains("://")
                && op
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            {
                return Err(QueryError {
                    at: start,
                    message: format!("Operador desconocido «{op}:»"),
                });
            }
        }

        Ok(Atom::Word(token.to_string()))
    }

    fn read_token(&mut self) -> &'a str {
        let start = self.pos;
        while !self.is_eof() {
            let ch = self.remaining().chars().next().unwrap();
            if ch.is_whitespace() || ch == ')' || ch == ']' {
                break;
            }
            self.pos += ch.len_utf8();
        }
        &self.input[start..self.pos]
    }
}
