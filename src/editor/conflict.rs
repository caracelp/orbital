//! Diálogo de conflicto y comparación lado a lado (US5, FR-019 a FR-021).
//!
//! Cuando la nota abierta cambia en el disco mientras el usuario tiene cambios sin guardar,
//! Orbital pausa el autoguardado y ofrece «Conservar la mía», «Usar la del disco» o «Comparar».
//! La comparación muestra dos `sourceview5::View` de solo lectura lado a lado con las líneas distintas
//! coloreadas y con `+`/`−` al margen (FR-021).

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use orbital_vault::Format;
use orbital_vault::diff::{DiffKind, diff_lines};

use crate::editor::code;

/// Opción elegida ante un conflicto de edición.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictChoice {
    KeepMine,
    UseDisk,
}

/// Opción elegida cuando la nota abierta fue borrada en el disco.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeletedChoice {
    SaveAgain,
    DiscardToBackup,
}

/// Muestra el diálogo cuando la nota abierta cambió en el disco con cambios locales pendientes.
pub fn show_conflict_dialog(
    parent: &impl IsA<gtk::Widget>,
    name: &str,
    mine_text: String,
    disk_text: String,
    format: Format,
    on_choice: impl Fn(ConflictChoice) + 'static,
) {
    let heading = "Cambio externo en la nota";
    let body = format!(
        "La nota «{name}» cambió en el disco mientras tenías cambios sin guardar. ¿Qué versión querés conservar?"
    );
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(&body)
        .default_response("keep_mine")
        .close_response("cancel")
        .build();

    dialog.add_response("keep_mine", "Conservar la mía");
    dialog.set_response_appearance("keep_mine", adw::ResponseAppearance::Suggested);
    dialog.add_response("use_disk", "Usar la del disco");
    dialog.add_response("compare", "Comparar");
    dialog.add_response("cancel", "Cancelar");

    let on_choice = Rc::new(on_choice);
    let parent_widget = parent.as_ref().clone();
    let name_str = name.to_string();

    dialog.choose(
        Some(parent),
        None::<&gtk::gio::Cancellable>,
        move |response| match response.as_str() {
            "keep_mine" => on_choice(ConflictChoice::KeepMine),
            "use_disk" => on_choice(ConflictChoice::UseDisk),
            "compare" => {
                let choice = on_choice.clone();
                show_compare_window(
                    &parent_widget,
                    &name_str,
                    &mine_text,
                    &disk_text,
                    format,
                    move |c| {
                        choice(c);
                    },
                );
            }
            _ => {}
        },
    );
}

/// Muestra el diálogo cuando la nota abierta fue borrada en el disco mientras se editaba.
pub fn show_deleted_dialog(
    parent: &impl IsA<gtk::Widget>,
    name: &str,
    on_choice: impl Fn(DeletedChoice) + 'static,
) {
    let heading = "La nota se borró en el disco";
    let body = format!(
        "La nota «{name}» se borró desde otro programa mientras tenías cambios sin guardar. ¿Querés guardarla de nuevo o descartar los cambios a un respaldo?"
    );
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(&body)
        .default_response("save_again")
        .close_response("cancel")
        .build();

    dialog.add_response("save_again", "Guardarla de nuevo");
    dialog.set_response_appearance("save_again", adw::ResponseAppearance::Suggested);
    dialog.add_response("discard", "Descartar a respaldo");
    dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
    dialog.add_response("cancel", "Cancelar");

    dialog.choose(
        Some(parent),
        None::<&gtk::gio::Cancellable>,
        move |response| match response.as_str() {
            "save_again" => on_choice(DeletedChoice::SaveAgain),
            "discard" => on_choice(DeletedChoice::DiscardToBackup),
            _ => {}
        },
    );
}

/// Ventana de comparación lado a lado con líneas distintas coloreadas y `+`/`−` al margen (FR-021).
pub fn show_compare_window(
    parent: &impl IsA<gtk::Widget>,
    name: &str,
    mine_text: &str,
    disk_text: &str,
    format: Format,
    on_choice: impl Fn(ConflictChoice) + 'static,
) {
    let window = adw::Window::builder()
        .title(format!("Comparar versiones: {name}"))
        .modal(true)
        .default_width(900)
        .default_height(600)
        .build();

    if let Some(root) = parent.root().and_downcast::<gtk::Window>() {
        window.set_transient_for(Some(&root));
    }

    let toolbar_view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    let title_widget = adw::WindowTitle::new("Comparar versiones", name);
    header.set_title_widget(Some(&title_widget));

    let on_choice = Rc::new(on_choice);

    let btn_keep = gtk::Button::builder()
        .label("Conservar la mía")
        .css_classes(["suggested-action"])
        .build();
    btn_keep
        .upcast_ref::<gtk::Widget>()
        .update_property(&[gtk::accessible::Property::Label("Conservar la mía")]);

    let btn_disk = gtk::Button::builder().label("Usar la del disco").build();
    btn_disk
        .upcast_ref::<gtk::Widget>()
        .update_property(&[gtk::accessible::Property::Label("Usar la del disco")]);

    let w_close = window.clone();
    let c_keep = on_choice.clone();
    btn_keep.connect_clicked(move |_| {
        w_close.close();
        c_keep(ConflictChoice::KeepMine);
    });

    let w_close2 = window.clone();
    let c_disk = on_choice.clone();
    btn_disk.connect_clicked(move |_| {
        w_close2.close();
        c_disk(ConflictChoice::UseDisk);
    });

    header.pack_end(&btn_disk);
    header.pack_end(&btn_keep);
    toolbar_view.add_top_bar(&header);

    let paned = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .wide_handle(true)
        .position(450)
        .build();

    // Columna izquierda: Mi versión
    let box_left = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let label_left = gtk::Label::builder()
        .label("Mi versión (editor)")
        .css_classes(["heading"])
        .halign(gtk::Align::Center)
        .margin_top(8)
        .margin_bottom(4)
        .build();
    label_left
        .upcast_ref::<gtk::Widget>()
        .update_property(&[gtk::accessible::Property::Label("Mi versión (editor)")]);
    box_left.append(&label_left);

    let (view_left, buffer_left) = code::new_view();
    view_left.set_editable(false);
    view_left
        .upcast_ref::<gtk::Widget>()
        .update_property(&[gtk::accessible::Property::Label("Mi versión")]);
    code::set_language(
        &buffer_left,
        match format {
            Format::Md => "markdown",
            Format::Html => "html",
        },
    );

    let scroller_left = gtk::ScrolledWindow::builder()
        .child(&view_left)
        .vexpand(true)
        .hexpand(true)
        .build();
    box_left.append(&scroller_left);

    // Columna derecha: Versión del disco
    let box_right = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let label_right = gtk::Label::builder()
        .label("Versión del disco")
        .css_classes(["heading"])
        .halign(gtk::Align::Center)
        .margin_top(8)
        .margin_bottom(4)
        .build();
    label_right
        .upcast_ref::<gtk::Widget>()
        .update_property(&[gtk::accessible::Property::Label("Versión del disco")]);
    box_right.append(&label_right);

    let (view_right, buffer_right) = code::new_view();
    view_right.set_editable(false);
    view_right
        .upcast_ref::<gtk::Widget>()
        .update_property(&[gtk::accessible::Property::Label("Versión del disco")]);
    code::set_language(
        &buffer_right,
        match format {
            Format::Md => "markdown",
            Format::Html => "html",
        },
    );

    let scroller_right = gtk::ScrolledWindow::builder()
        .child(&view_right)
        .vexpand(true)
        .hexpand(true)
        .build();
    box_right.append(&scroller_right);

    // Etiquetas de estilo para diferencias (paleta rosa/verde con contraste verificado)
    let tag_removed = gtk::TextTag::builder()
        .name("diff-removed")
        .paragraph_background(crate::style::DIFF_REMOVED_BG.hex())
        .foreground(crate::style::DIFF_REMOVED_FG.hex())
        .build();
    let tag_removed_marker = gtk::TextTag::builder()
        .name("diff-removed-marker")
        .weight(700)
        .foreground(crate::style::DIFF_REMOVED_MARKER.hex())
        .build();
    buffer_left.tag_table().add(&tag_removed);
    buffer_left.tag_table().add(&tag_removed_marker);

    let tag_added = gtk::TextTag::builder()
        .name("diff-added")
        .paragraph_background(crate::style::DIFF_ADDED_BG.hex())
        .foreground(crate::style::DIFF_ADDED_FG.hex())
        .build();
    let tag_added_marker = gtk::TextTag::builder()
        .name("diff-added-marker")
        .weight(700)
        .foreground(crate::style::DIFF_ADDED_MARKER.hex())
        .build();
    buffer_right.tag_table().add(&tag_added);
    buffer_right.tag_table().add(&tag_added_marker);

    let lines = diff_lines(mine_text, disk_text);
    let mut iter_left = buffer_left.start_iter();
    let mut iter_right = buffer_right.start_iter();

    for line in lines {
        match line.kind {
            DiffKind::Same => {
                let text = format!("   {}\n", line.text);
                buffer_left.insert(&mut iter_left, &text);
                buffer_right.insert(&mut iter_right, &text);
            }
            DiffKind::Removed => {
                buffer_left.insert_with_tags_by_name(
                    &mut iter_left,
                    "−  ",
                    &["diff-removed", "diff-removed-marker"],
                );
                let text = format!("{}\n", line.text);
                buffer_left.insert_with_tags_by_name(&mut iter_left, &text, &["diff-removed"]);
                buffer_right.insert(&mut iter_right, "\n");
            }
            DiffKind::Added => {
                buffer_left.insert(&mut iter_left, "\n");
                buffer_right.insert_with_tags_by_name(
                    &mut iter_right,
                    "+  ",
                    &["diff-added", "diff-added-marker"],
                );
                let text = format!("{}\n", line.text);
                buffer_right.insert_with_tags_by_name(&mut iter_right, &text, &["diff-added"]);
            }
        }
    }

    // Desplazamiento vertical sincronizado
    let syncing = Rc::new(Cell::new(false));
    let s1 = syncing.clone();
    let adj_r = scroller_right.vadjustment();
    scroller_left
        .vadjustment()
        .connect_value_changed(move |adj| {
            if s1.get() {
                return;
            }
            s1.set(true);
            adj_r.set_value(adj.value());
            s1.set(false);
        });

    let s2 = syncing.clone();
    let adj_l = scroller_left.vadjustment();
    scroller_right
        .vadjustment()
        .connect_value_changed(move |adj| {
            if s2.get() {
                return;
            }
            s2.set(true);
            adj_l.set_value(adj.value());
            s2.set(false);
        });

    paned.set_start_child(Some(&box_left));
    paned.set_end_child(Some(&box_right));
    toolbar_view.set_content(Some(&paned));
    window.set_content(Some(&toolbar_view));

    window.present();
}

#[cfg(test)]
#[path = "conflict_tests.rs"]
mod conflict_tests;
