//! T003: sesión de edición (data-model de la 002).

mod common;

use std::fs;

use common::{TempDir, copy_tree, ctx, fixture};
use orbital_vault::session::{EditSession, ExternalAction, SaveOutcome, SessionState};
use orbital_vault::write::hash;
use orbital_vault::{RelPath, Vault};

fn rel(s: &str) -> RelPath {
    RelPath::new(s).unwrap()
}

fn setup() -> (TempDir, Vault) {
    let t = TempDir::new("sesion");
    copy_tree(&fixture("completa"), t.path());
    copy_tree(&fixture("notas"), &t.path().join("wiki/notas"));
    let v = orbital_vault::open(t.path()).unwrap();
    (t, v)
}

fn backups(t: &TempDir) -> usize {
    fn count(p: &std::path::Path) -> usize {
        fs::read_dir(p).map_or(0, |d| {
            d.map(|e| {
                let p = e.unwrap().path();
                if p.is_dir() {
                    count(&p)
                } else {
                    usize::from(!p.to_string_lossy().ends_with(".yaml"))
                }
            })
            .sum()
        })
    }
    count(&t.path().join(".orbital/respaldo"))
}

#[test]
fn edit_save_cycle() {
    let (t, v) = setup();
    let p = rel("wiki/fisica/entropia.md");
    let note = v.read_note(&p).unwrap();
    let (mut s, text) = EditSession::open(&note);
    assert_eq!(s.state(), &SessionState::Clean);
    assert_eq!(text, note.source);

    // Guardar sin cambios no escribe.
    assert_eq!(
        s.save(&v, &text, &ctx()).unwrap(),
        SaveOutcome::NothingToSave
    );

    let new = format!("{text}\nUn párrafo nuevo.\n");
    s.edited();
    assert_eq!(s.state(), &SessionState::Dirty);
    assert!(matches!(
        s.save(&v, &new, &ctx()).unwrap(),
        SaveOutcome::Saved { .. }
    ));
    assert_eq!(s.state(), &SessionState::Clean);
    assert_eq!(fs::read_to_string(p.to_path(t.path())).unwrap(), new);
    assert_eq!(backups(&t), 1);
    assert!(s.last_saved().is_some());

    // Una segunda edición parte del hash nuevo (no es un conflicto).
    let newer = format!("{new}Otro.\n");
    s.edited();
    assert!(matches!(
        s.save(&v, &newer, &ctx()).unwrap(),
        SaveOutcome::Saved { .. }
    ));
}

#[test]
fn bom_and_crlf_are_kept() {
    let (t, v) = setup();
    for name in ["bom.md", "crlf.md"] {
        let p = rel(&format!("wiki/notas/{name}"));
        let before = fs::read(p.to_path(t.path())).unwrap();
        let note = v.read_note(&p).unwrap();
        let (mut s, text) = EditSession::open(&note);
        assert!(!text.starts_with('\u{feff}'), "el editor no ve el BOM");
        assert!(!text.contains('\r'), "el editor trabaja con \\n");
        let edited = format!("{text}Más.\n");
        s.edited();
        s.save(&v, &edited, &ctx()).unwrap();
        let after = fs::read(p.to_path(t.path())).unwrap();
        assert_eq!(
            after.starts_with(&[0xef, 0xbb, 0xbf]),
            before.starts_with(&[0xef, 0xbb, 0xbf]),
            "{name}"
        );
        let crlf_before = before.windows(2).any(|w| w == b"\r\n");
        let crlf_after = String::from_utf8_lossy(&after).contains("\r\n");
        assert_eq!(crlf_before, crlf_after, "{name}");
        assert!(
            String::from_utf8_lossy(&after)
                .trim_start_matches('\u{feff}')
                .ends_with(if crlf_before { "Más.\r\n" } else { "Más.\n" })
        );
    }
}

#[test]
fn external_changes() {
    let (t, v) = setup();
    let p = rel("wiki/fisica/entropia.md");
    let note = v.read_note(&p).unwrap();

    // Sin cambios pendientes: se recarga.
    let (mut s, _) = EditSession::open(&note);
    assert_eq!(s.external_change(12345), ExternalAction::Reload);

    // El eco de un guardado propio se ignora.
    let (mut s, text) = EditSession::open(&note);
    let mine = format!("{text}x\n");
    s.edited();
    s.save(&v, &mine, &ctx()).unwrap();
    assert_eq!(
        s.external_change(hash(mine.as_bytes())),
        ExternalAction::Ignore
    );

    // Con cambios pendientes: conflicto, y el guardado automático no escribe.
    s.edited();
    fs::write(p.to_path(t.path()), "de otro programa\n").unwrap();
    let disk = hash(b"de otro programa\n");
    assert_eq!(s.external_change(disk), ExternalAction::AskUser);
    assert_eq!(s.state(), &SessionState::Conflict { disk_hash: disk });
    assert_eq!(s.save(&v, "mía\n", &ctx()).unwrap(), SaveOutcome::Conflict);
    assert_eq!(
        fs::read_to_string(p.to_path(t.path())).unwrap(),
        "de otro programa\n"
    );
}

#[test]
fn save_detects_conflict_by_itself() {
    let (t, v) = setup();
    let p = rel("wiki/fisica/entropia.md");
    let (mut s, text) = EditSession::open(&v.read_note(&p).unwrap());
    fs::write(p.to_path(t.path()), "cambiado antes del guardado\n").unwrap();
    s.edited();
    assert_eq!(
        s.save(&v, &format!("{text}y\n"), &ctx()).unwrap(),
        SaveOutcome::Conflict
    );
    assert!(matches!(s.state(), SessionState::Conflict { .. }));
    assert_eq!(
        fs::read_to_string(p.to_path(t.path())).unwrap(),
        "cambiado antes del guardado\n"
    );
}

#[test]
fn keep_mine_and_use_disk_back_up_the_loser() {
    let (t, v) = setup();
    let p = rel("wiki/fisica/entropia.md");

    // Conservar la mía: la del disco queda como respaldo.
    let (mut s, text) = EditSession::open(&v.read_note(&p).unwrap());
    s.edited();
    fs::write(p.to_path(t.path()), "versión del disco 1\n").unwrap();
    s.external_change(hash("versión del disco 1\n".as_bytes()));
    let mine = format!("{text}mía\n");
    let before = backups(&t);
    s.keep_mine(&v, &mine, &ctx()).unwrap();
    assert_eq!(fs::read_to_string(p.to_path(t.path())).unwrap(), mine);
    assert_eq!(s.state(), &SessionState::Clean);
    assert_eq!(backups(&t), before + 1);

    // Usar la del disco: la mía queda como respaldo.
    s.edited();
    fs::write(p.to_path(t.path()), "versión del disco 2\n").unwrap();
    s.external_change(hash("versión del disco 2\n".as_bytes()));
    let before = backups(&t);
    let loaded = s.use_disk(&v, "mía sin guardar\n", &ctx()).unwrap();
    assert_eq!(loaded, "versión del disco 2\n");
    assert_eq!(
        fs::read_to_string(p.to_path(t.path())).unwrap(),
        "versión del disco 2\n"
    );
    assert_eq!(s.state(), &SessionState::Clean);
    assert_eq!(backups(&t), before + 1);
}

#[test]
fn failure_then_retry() {
    let (t, v) = setup();
    let p = rel("wiki/fisica/entropia.md");
    let (mut s, text) = EditSession::open(&v.read_note(&p).unwrap());
    s.edited();
    let dir = t.path().join("wiki/fisica");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    let r = s.save(&v, &format!("{text}z\n"), &ctx());
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(r.is_err());
    assert!(matches!(s.state(), SessionState::Failed { .. }));
    assert_eq!(s.failures(), 1);
    // Reintento con permisos restaurados.
    assert!(matches!(
        s.save(&v, &format!("{text}z\n"), &ctx()).unwrap(),
        SaveOutcome::Saved { .. }
    ));
    assert_eq!(s.failures(), 0);
}

/// T018: las propiedades se editan sobre el texto del editor (no sobre el disco) y solo cambian sus
/// líneas; quitar una propiedad también.
#[test]
fn props_on_editor_text() {
    let (_t, v) = setup();
    let p = rel("wiki/fisica/entropia.md");
    let note = v.read_note(&p).unwrap();
    let (_s, text) = EditSession::open(&note);
    // Algo escrito y sin guardar: no se pierde al editar una propiedad.
    let typed = format!("{text}\nSin guardar.\n");
    let editing = note.reparse(typed.clone()).unwrap();

    let new = editing
        .with_prop(
            "title",
            &orbital_vault::props::PropValue::Text("Entropía térmica".into()),
        )
        .unwrap();
    let changed: Vec<_> = typed
        .lines()
        .zip(new.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(
        changed,
        vec![("title: Entropía", "title: Entropía térmica")]
    );
    assert_eq!(typed.lines().count(), new.lines().count());
    assert!(new.ends_with("\nSin guardar.\n"));

    let without = note.reparse(new).unwrap().without_prop("tags").unwrap();
    assert!(!without.contains("tags:"));
    assert_eq!(without.lines().count(), typed.lines().count() - 1);
    // El disco no se tocó.
    assert_eq!(v.read_note(&p).unwrap().source, note.source);
}

/// Si el guardado automático cambia el título, `index.md` se pone al día (como `set_prop`).
#[test]
fn autosave_updates_index_when_title_changes() {
    let (t, v) = setup();
    let p = rel("wiki/fisica/entropia.md");
    let note = v.read_note(&p).unwrap();
    let (mut s, text) = EditSession::open(&note);
    let index = || fs::read_to_string(t.path().join("wiki/index.md")).unwrap();

    // Un cambio en el cuerpo no toca el índice.
    let before = index();
    s.edited();
    let body = format!("{text}\nOtro párrafo.\n");
    s.save(&v, &body, &ctx()).unwrap();
    assert_eq!(index(), before);

    let renamed = body.replace("title: Entropía", "title: Entropía térmica");
    s.edited();
    let job = s.prepare_save(&renamed).unwrap();
    let result = job.run(&v, &ctx());
    let orbital_vault::session::SaveResult::Written { also, .. } = &result else {
        panic!("{result:?}")
    };
    assert_eq!(also.len(), 1);
    assert_eq!(also[0].0, rel("wiki/index.md"));
    assert!(index().contains("[Entropía térmica](/fisica/entropia.md)"));
    s.apply(result, ctx().time, false).unwrap();

    // Guardar otra vez sin cambiar el título no vuelve a escribir el índice.
    s.edited();
    let job = s.prepare_save(&format!("{renamed}más\n")).unwrap();
    let orbital_vault::session::SaveResult::Written { also, .. } = job.run(&v, &ctx()) else {
        panic!()
    };
    assert!(also.is_empty());
}
