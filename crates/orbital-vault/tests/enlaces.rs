//! 009: asociación de archivos con URL en .orbital/enlaces.yaml.

mod common;

use common::{TempDir, ctx};
use orbital_vault::RelPath;

#[test]
fn asociar_persistir_quitar_y_migrar() {
    let tmp = TempDir::new("enlaces");
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let ruta = RelPath::new("raw/libro.pdf").unwrap();
    vault
        .asociar_url(&ruta, "https://ejemplo.com/x", &ctx())
        .unwrap();
    assert_eq!(
        vault.url_de(&ruta).as_deref(),
        Some("https://ejemplo.com/x")
    );
    // Reemplazo (US1.3): la segunda asociación pisa la primera.
    vault
        .asociar_url(&ruta, "https://nuevo.org/", &ctx())
        .unwrap();
    assert_eq!(vault.url_de(&ruta).as_deref(), Some("https://nuevo.org/"));
    // FR-2: solo http/https.
    assert!(vault.asociar_url(&ruta, "ftp://nada", &ctx()).is_err());
    // FR-3: migración al renombrar.
    let nueva = RelPath::new("raw/libro-2026.pdf").unwrap();
    assert!(vault.migrar_enlace(&ruta, &nueva, &ctx()).unwrap());
    assert!(vault.url_de(&ruta).is_none());
    assert_eq!(vault.url_de(&nueva).as_deref(), Some("https://nuevo.org/"));
    // Quitar; quitar de nuevo es no-op.
    vault.quitar_url(&nueva, &ctx()).unwrap();
    assert!(vault.url_de(&nueva).is_none());
    vault.quitar_url(&nueva, &ctx()).unwrap();
}

#[test]
fn yaml_corrupto_se_preserva_con_aviso() {
    let tmp = TempDir::new("enlaces");
    let vault = orbital_vault::open(tmp.path()).unwrap();
    std::fs::create_dir_all(tmp.path().join(".orbital")).unwrap();
    std::fs::write(tmp.path().join(".orbital/enlaces.yaml"), "- ruta: [").unwrap();
    let snap = vault.enlaces_snapshot();
    assert!(snap.warning.is_some(), "debe avisar el YAML corrupto");
    // FR-4: no se sobrescribe hasta que cambie.
    let ruta = RelPath::new("raw/x.pdf").unwrap();
    assert!(vault.asociar_url(&ruta, "https://a.b/", &ctx()).is_err());
    assert_eq!(
        std::fs::read_to_string(tmp.path().join(".orbital/enlaces.yaml")).unwrap(),
        "- ruta: ["
    );
}

#[test]
fn cambio_externo_se_ve_en_la_lectura_siguiente() {
    // `asociar_url` relee el YAML en cada llamada: los cambios externos entran como base nueva
    // (última escritura gana, con escritura atómica). La edición a mano del YAML es de primera.
    let tmp = TempDir::new("enlaces");
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let ruta = RelPath::new("raw/x.pdf").unwrap();
    vault.asociar_url(&ruta, "https://a.b/", &ctx()).unwrap();
    std::fs::write(
        tmp.path().join(".orbital/enlaces.yaml"),
        "- ruta: raw/x.pdf\n  url: https://externo/\n",
    )
    .unwrap();
    assert_eq!(vault.url_de(&ruta).as_deref(), Some("https://externo/"));
    // Y el editor de la app escribe encima sin perder las demás claves.
    vault.asociar_url(&ruta, "https://otro/", &ctx()).unwrap();
    assert_eq!(vault.url_de(&ruta).as_deref(), Some("https://otro/"));
}
