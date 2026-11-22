use super::*;

impl OnStage {
    /// Muestra la diapositiva virtual o real que toca según el índice actual.
    pub(super) fn render_actual(&self) {
        let portada_v = self.portada_visible();
        let toc_v = self.toc_visible();
        let total =
            usize::from(portada_v) + usize::from(toc_v) + self.0.diapositivas.borrow().len();
        if total == 0 {
            // Nota sin contenido: una diapositiva vacía (nunca falla).
            self.actualizar_estado();
            self.0.stack.set_visible_child_name("diapositiva");
            self.volcar(Prepared::new("", &[]));
            return;
        }
        let indice = self.0.indice.get().min(total - 1);
        self.0.indice.set(indice);
        self.actualizar_estado();
        if portada_v && indice == 0 {
            self.0.stack.set_visible_child_name("portada");
            return;
        }
        if toc_v && indice == usize::from(portada_v) {
            self.0.stack.set_visible_child_name("indice");
            return;
        }
        let prefijo = usize::from(portada_v) + usize::from(toc_v);
        let real = indice - prefijo;
        self.cargar_diapositiva(real);
    }

    /// Vuelve a armar la lista del índice con las diapositivas actuales.
    pub(super) fn rearmar_toc(&self) {
        let i = &self.0;
        while let Some(hijo) = i.toc_lista.first_child() {
            i.toc_lista.remove(&hijo);
        }
        let diapositivas = i.diapositivas.borrow();
        for (n, d) in diapositivas.iter().enumerate() {
            let rotulo = if d.titulo.is_empty() {
                format!("{}. (sin título)", n + 1)
            } else {
                format!("{}. {}", n + 1, d.titulo)
            };
            let texto = gtk::Label::builder()
                .label(&rotulo)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .halign(gtk::Align::Start)
                .build();
            let boton = gtk::Button::builder()
                .css_classes(["flat"])
                .child(&texto)
                .build();
            if d.nivel > 1 {
                texto.set_margin_start(INDENT_NIVEL2);
            }
            boton.update_property(&[gtk::accessible::Property::Description(
                format!("Ir a la diapositiva {}: {}", n + 1, d.titulo).as_str(),
            )]);
            let this = self.clone();
            let destino = n;
            boton.connect_clicked(move |_| {
                this.ir_a(this.prefijo() + destino);
            });
            i.toc_lista.append(&boton);
        }
        if diapositivas.is_empty() {
            let vacio = gtk::Label::builder()
                .label("La nota no tiene encabezados de nivel 1 ni 2")
                .css_classes(["dim-label"])
                .halign(gtk::Align::Start)
                .build();
            i.toc_lista.append(&vacio);
        }
    }

    /// Material nuevo del editor: divide, resuelve el frontmatter y prepara la diapositiva
    /// vigente, todo fuera del hilo principal.
    pub(super) fn refrescar(&self) {
        let i = &self.0;
        let Some(fuente) = i.proveedor.borrow().as_ref().and_then(|f| f()) else {
            return;
        };
        let generation = i.generation.get() + 1;
        i.generation.set(generation);
        let indice_viejo = i.indice.get();
        let portada_on = i.toggle_portada.is_active();
        let toc_on = i.toggle_toc.is_active();
        let nombre = fuente
            .ruta
            .as_str()
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_owned();
        let job = {
            let texto = fuente.texto.clone();
            let ruta = fuente.ruta.clone();
            let vault = fuente.vault.clone();
            let busqueda = fuente.indice.clone();
            async move {
                gio::spawn_blocking(move || {
                    let yaml = orbital_vault::props::markdown::locate(&texto)
                        .map(|r| orbital_vault::props::yaml::parse_block(&texto[r]))
                        .map(|parsed| parsed.unwrap_or_default())
                        .unwrap_or_default();
                    let diapositivas = dividir_diapositivas(&texto);
                    let portada = portada_de(&yaml);
                    let prefijo = usize::from(portada_on && portada.is_some())
                        + usize::from(toc_on && !diapositivas.is_empty());
                    let total = prefijo + diapositivas.len();
                    let indice = if total == 0 {
                        0
                    } else {
                        indice_viejo.min(total - 1)
                    };
                    let real = indice
                        .checked_sub(prefijo)
                        .filter(|r| *r < diapositivas.len());
                    let prepared = real.map(|r| {
                        let slice = diapositivas[r].texto.as_str();
                        super::preparar(slice, vault.as_ref(), &ruta, &yaml, busqueda.as_ref())
                    });
                    (diapositivas, yaml, portada, indice, prepared)
                })
                .await
            }
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let Ok((diapositivas, yaml, portada, indice, prepared)) = job.await else {
                return;
            };
            if this.0.generation.get() != generation {
                return;
            }
            let i = &this.0;
            *i.fuente.borrow_mut() = Some(fuente);
            *i.yaml.borrow_mut() = yaml;
            *i.diapositivas.borrow_mut() = diapositivas;
            *i.portada_datos.borrow_mut() = portada.clone();
            i.titulo_barra.set_label(&nombre);
            if let Some((titulo, descripcion)) = &portada {
                i.portada_titulo.set_label(titulo);
                i.portada_desc
                    .set_label(descripcion.as_deref().unwrap_or(""));
            }
            i.indice.set(indice);
            this.rearmar_toc();
            // is_missing con la nota vigente: los enlaces a notas ausentes quedan atenuados.
            if let Some(f) = i.fuente.borrow().as_ref()
                && let Some(v) = f.vault.clone()
            {
                let ruta = f.ruta.clone();
                let vault = Arc::downgrade(&v);
                i.reader.set_is_missing(move |url| {
                    if url.starts_with("orbital-tag:")
                        || url.starts_with(crate::editor::reader::NOTA_NUEVA)
                        || url.starts_with("http://")
                        || url.starts_with("https://")
                        || url.starts_with("mailto:")
                    {
                        return false;
                    }
                    let Some(v) = vault.upgrade() else {
                        return false;
                    };
                    v.resolve_link(&ruta, url) == orbital_vault::Resolution::Missing
                });
            }
            match prepared {
                Some(prepared) => {
                    this.actualizar_estado();
                    this.volcar(prepared);
                }
                None => this.render_actual(),
            }
        });
    }

    /// Vuelca una diapositiva preparada en el Reader y muestra la vista.
    pub(super) fn volcar(&self, prepared: Prepared) {
        let i = &self.0;
        let base_dir = i.fuente.borrow().as_ref().and_then(|f| {
            f.vault
                .as_ref()
                .and_then(|v| f.ruta.to_path(v.root()).parent().map(PathBuf::from))
        });
        i.stack.set_visible_child_name("diapositiva");
        i.reader.render(prepared, || None, base_dir);
    }

    /// Prepara y vuelca la diapositiva real que toca (navegación y toggles).
    pub(super) fn cargar_diapositiva(&self, real: usize) {
        let i = &self.0;
        let prestamo = i.fuente.borrow();
        let Some(fuente) = prestamo.as_ref() else {
            return;
        };
        let Some(slide) = i.diapositivas.borrow().get(real).cloned() else {
            return;
        };
        let vault = fuente.vault.clone();
        let indice = fuente.indice.clone();
        let ruta = fuente.ruta.clone();
        let yaml = i.yaml.borrow().clone();
        let generation = i.generation.get() + 1;
        i.generation.set(generation);
        let texto = slide.texto;
        let estilo = i.reader.estilo_formulas();
        let estilo_diagramas = i.reader.estilo_diagramas();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let Ok(prepared) = gio::spawn_blocking(move || {
                let prepared =
                    super::preparar(&texto, vault.as_ref(), &ruta, &yaml, indice.as_ref());
                // Las fórmulas, fuera del hilo de GTK (050).
                crate::formulas::precalentar(Reader::claves_formulas(&prepared.doc, &estilo));
                // Los diagramas (058).
                crate::diagramas::precalentar(Reader::claves_diagramas(
                    &prepared.doc,
                    estilo_diagramas,
                ));
                prepared
            })
            .await
            else {
                return;
            };
            if this.0.generation.get() != generation {
                return;
            }
            this.actualizar_estado();
            this.volcar(prepared);
        });
    }
}
