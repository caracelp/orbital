//! Prueba de caos (SC-004): dos equipos y la «web de Drive» cambian, renombran y borran los
//! mismos archivos al azar, con cortes de red, errores del servidor y subidas cortadas a mitad.
//! Desde la 061b, además, los dos equipos cambian la ubicación de archivos y carpetas («solo en
//! Drive» / «en ambos») y los predeterminados para lo nuevo, editan las copias de lo «solo en
//! Drive», y «Liberar espacio» se corta a mitad (como un corte de luz, con reinicio).
//! Al final se verifica que **ninguna versión se perdió** y que los dos equipos (lo que ven: la
//! bóveda más lo «solo en Drive») y Drive quedaron iguales. `ORBITAL_CAOS_OPS=N` y `ORBITAL_CAOS_SEMILLA=S` cambian el largo y la semilla.

mod comun;

use std::collections::BTreeSet;

use comun::{Azar, Equipo};

use std::cell::RefCell;

thread_local! {
    static TRAZA: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn traza(t: String) {
    if std::env::var_os("ORBITAL_CAOS_VIVO").is_some() {
        static INICIO: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        let ms = INICIO
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_millis();
        eprintln!("{ms:>7} {t}");
    }
    TRAZA.with(|x| x.borrow_mut().push(t));
}

/// Etiqueta de una versión (la «firma única»).
fn etiqueta(v: &[u8]) -> String {
    let t = String::from_utf8_lossy(v);
    t.lines()
        .find_map(|l| {
            l.strip_prefix("firma única ")
                .or_else(|| l.strip_prefix("BIN "))
        })
        .unwrap_or("?")
        .to_owned()
}

fn foto(e: &Equipo) -> String {
    e.arbol()
        .iter()
        .filter(|(r, _)| !r.starts_with(".orbital"))
        .map(|(r, v)| format!("{r}={}", etiqueta(v)))
        .collect::<Vec<_>>()
        .join(" ")
}
use orbital_drive::falso::{Falla, ServidorFalso};
use orbital_drive::motor::Destino;

const NOMBRES: &[&str] = &[
    "wiki/a.md",
    "wiki/b.md",
    "wiki/c.html",
    "wiki/sub/d.md",
    "wiki/sub/e.md",
    "wiki/sub/f.bin",
    "raw/g.bin",
    "raw/h.md",
    "wiki/otra/i.md",
    "wiki/otra/j.md",
];

fn texto(n: u64, quien: &str, r: &str) -> Vec<u8> {
    if r.ends_with(".bin") {
        let mut v = format!("BIN {quien} {n}\n").into_bytes();
        v.extend((0..(n % 5000) as u32).map(|i| (i % 251) as u8));
        v
    } else {
        // Varios renglones: a veces se tocan renglones distintos (fusión), a veces el mismo.
        let mut s = String::from("# Nota\n\n");
        for k in 0..6 {
            if (n + k).is_multiple_of(3) {
                s.push_str(&format!("renglón {k} de {quien} v{n}\n"));
            } else {
                s.push_str(&format!("renglón {k}\n"));
            }
        }
        s.push_str(&format!("\nfirma única {quien} {n}\n"));
        s.into_bytes()
    }
}

/// Agrega a `debe` todo lo que hay ahora en el equipo (lo que existía en un ciclo exitoso), las
/// copias de lo «solo en Drive» incluidas (pueden tener cambios sin subir).
fn anotar(debe: &mut BTreeSet<Vec<u8>>, e: &Equipo) {
    debe.extend(e.arbol().into_values());
    debe.extend(e.copias());
}

/// Cambia la ubicación de un archivo o de una carpeta, o los predeterminados (061b).
fn actuar_ubicacion(e: &mut Equipo, az: &mut Azar, quien: &str) {
    use orbital_drive::reglas::Ubicacion;
    let carpetas = ["wiki/", "wiki/sub/", "raw/", "wiki/otra/"];
    let u = [Ubicacion::SoloDrive, Ubicacion::Ambos][az.hasta(2)];
    match az.hasta(6) {
        0..=2 => {
            let r = NOMBRES[az.hasta(NOMBRES.len())];
            let u = if az.hasta(4) == 0 { None } else { Some(u) };
            e.s().ubicar(r, u);
            traza(format!("{quien}: ubicación {r} = {u:?}"));
        }
        3 | 4 => {
            let c = carpetas[az.hasta(carpetas.len())];
            let u = if az.hasta(4) == 0 { None } else { Some(u) };
            e.s().ubicar(c, u);
            traza(format!("{quien}: ubicación {c} = {u:?}"));
        }
        _ => {
            let l = [Ubicacion::Ambos, Ubicacion::SoloDrive][az.hasta(2)];
            e.s().predeterminados(l, u);
            traza(format!("{quien}: predeterminados local={l:?} remoto={u:?}"));
        }
    }
}

/// Edita la copia de la caché de algo «solo en Drive» (como el editor de Orbital).
fn editar_copia(e: &mut Equipo, az: &mut Azar, n: &mut u64, quien: &str) {
    let marcas = e.marcadores();
    if marcas.is_empty() {
        return;
    }
    let r = marcas[az.hasta(marcas.len())].clone();
    match e.s().traer(&r) {
        Ok(copia) => {
            *n += 1;
            std::thread::sleep(std::time::Duration::from_millis(2));
            std::fs::write(&copia, texto(*n, quien, &r)).unwrap();
            e.s().marcar([r.clone()]);
            traza(format!("{quien}: edita la copia de {r} = {quien} {n}"));
        }
        Err(m) => traza(format!("{quien}: no trajo {r} ({m})")),
    }
}

/// Una bajada de lo «solo en Drive» que la usuaria cancela a mitad (a veces con la red cortada o
/// un tramo cortado antes): no queda nada a medias de ese archivo y el motor sigue sano.
fn bajada_cancelada(e: &mut Equipo, az: &mut Azar, pedido: &mut u64, quien: &str) {
    let marcas = e.marcadores();
    if marcas.is_empty() {
        return;
    }
    let r = marcas[az.hasta(marcas.len())].clone();
    let id = e.s().estado.registros[&r].id.clone();
    *pedido += 1;
    let en = az.hasta(3000) as u64;
    e.s().cancelador.cancelar_en(*pedido, en);
    let res = e.s().traer_pedido(&r, *pedido);
    traza(format!(
        "{quien}: bajada de {r} cancelada en {en} B -> {res:?}"
    ));
    if res
        .as_deref()
        .is_err_and(|m| m == orbital_drive::motor::CANCELADA)
    {
        let quedan: Vec<String> = e
            .parciales()
            .into_iter()
            .filter(|p| p.starts_with(&format!("{id}-")))
            .collect();
        assert!(quedan.is_empty(), "bajada cancelada a medias: {quedan:?}");
    }
}

/// Renombra, mueve o manda a la papelera algo «solo en Drive» como desde el explorador: sin
/// conexión (o si se corta en el intento) queda en cola. A veces la web toca el mismo archivo
/// mientras tanto (lo edita, lo renombra, lo borra o le quita el nombre).
fn cambio_en_cola(
    srv: &ServidorFalso,
    raiz: &str,
    e: &mut Equipo,
    az: &mut Azar,
    n: &mut u64,
    quien: &str,
) {
    let marcas = e.marcadores();
    if marcas.is_empty() {
        return;
    }
    let r = marcas[az.hasta(marcas.len())].clone();
    let id = e.s().estado.registros[&r].id.clone();
    let en_linea = !srv.datos().caido && az.hasta(3) == 0;
    if az.hasta(4) == 0 {
        let res = e.s().pedir_borrar(&r, en_linea);
        traza(format!(
            "{quien}: papelera {r} (en línea {en_linea}) -> {res:?}"
        ));
    } else {
        let destino = NOMBRES[az.hasta(NOMBRES.len())];
        if destino == r || e.root.join(destino).exists() {
            return;
        }
        let res = e.s().pedir_mover(&r, destino, en_linea);
        traza(format!(
            "{quien}: mueve {r} -> {destino} (en línea {en_linea}) -> {res:?}"
        ));
    }
    if az.hasta(2) == 0 {
        return;
    }
    // La web, mientras tanto, sobre el mismo archivo.
    let mut d = srv.datos();
    if !d.archivos.get(&id).is_some_and(|a| !a.papelera) {
        return;
    }
    match az.hasta(4) {
        0 | 1 => {
            *n += 1;
            let c = texto(*n, "web", &r);
            d.editar(&id, &c, "Web");
            traza(format!("web: edita (cruzado) {r} = web {n}"));
        }
        2 => {
            d.a_papelera(&id);
            traza(format!("web: papelera (cruzado) {r}"));
        }
        _ => {
            // Otro archivo toma el nombre que se pidió (o el mismo se renombra en su carpeta).
            let destino = NOMBRES[az.hasta(NOMBRES.len())];
            if r.rsplit_once('/').map(|x| x.0) == destino.rsplit_once('/').map(|x| x.0)
                && d.buscar(destino, raiz).is_none()
            {
                d.renombrar(&id, destino.rsplit_once('/').unwrap().1);
                traza(format!("web: renombra (cruzado) {r} -> {destino}"));
            }
        }
    }
}

fn actuar_local(e: &mut Equipo, az: &mut Azar, n: &mut u64, quien: &str) {
    let r = NOMBRES[az.hasta(NOMBRES.len())];
    // Nunca se edita un archivo en conflicto sin resolverlo antes (la versión es de la usuaria).
    if e.s().estado.conflictos.contains_key(r) {
        let mia = az.hasta(2) == 0;
        let res = e.s().resolver(r, mia);
        traza(format!("{quien}: resolver {r} mia={mia} -> {res:?}"));
        return;
    }
    let existe = e.root.join(r).is_file();
    match az.hasta(10) {
        0..=5 => {
            *n += 1;
            e.escribir(r, &texto(*n, quien, r));
            traza(format!("{quien}: escribe {r} = {quien} {n}"));
        }
        6 | 7 if existe => {
            let destino = NOMBRES[az.hasta(NOMBRES.len())];
            if destino != r
                && !e.root.join(destino).exists()
                && !e.s().estado.conflictos.contains_key(destino)
            {
                std::fs::create_dir_all(e.root.join(destino).parent().unwrap()).unwrap();
                std::fs::rename(e.root.join(r), e.root.join(destino)).unwrap();
                traza(format!("{quien}: renombra {r} -> {destino}"));
            }
        }
        8 if existe => {
            std::fs::remove_file(e.root.join(r)).unwrap();
            traza(format!("{quien}: borra {r}"));
        }
        _ => {}
    }
}

fn actuar_web(srv: &ServidorFalso, raiz: &str, az: &mut Azar, n: &mut u64) {
    let r = NOMBRES[az.hasta(NOMBRES.len())];
    let mut d = srv.datos();
    let id = d.buscar(r, raiz);
    match (az.hasta(8), id) {
        (0..=3, Some(id)) => {
            *n += 1;
            d.editar(&id, &texto(*n, "web", r), "Web");
            traza(format!("web: edita {r} = web {n}"));
        }
        (4, Some(id)) => {
            let destino = NOMBRES[az.hasta(NOMBRES.len())];
            let (carpeta_r, carpeta_d) = (
                r.rsplit_once('/').map(|x| x.0),
                destino.rsplit_once('/').map(|x| x.0),
            );
            if carpeta_r == carpeta_d && d.buscar(destino, raiz).is_none() {
                d.renombrar(&id, destino.rsplit_once('/').unwrap().1);
                traza(format!("web: renombra {r} -> {destino}"));
            }
        }
        (5, Some(id)) => {
            d.a_papelera(&id);
            traza(format!("web: papelera {r}"));
        }
        (6 | 7, None) => {
            let carpeta = r.rsplit_once('/').unwrap().0;
            if let Some(p) = d.buscar(carpeta, raiz) {
                *n += 1;
                let c = texto(*n, "web", r);
                d.crear(&p, r.rsplit_once('/').unwrap().1, &c, "Web");
                traza(format!("web: crea {r} = web {n}"));
            }
        }
        _ => {}
    }
}

/// El mismo archivo renombrado a la vez en A, en B y en la web, con nombres distintos.
fn renombre_simultaneo(
    srv: &ServidorFalso,
    raiz: &str,
    a: &mut Equipo,
    b: &mut Equipo,
    az: &mut Azar,
) {
    let r = NOMBRES[az.hasta(NOMBRES.len())];
    let libres: Vec<&str> = NOMBRES
        .iter()
        .copied()
        .filter(|n| {
            *n != r
                && !a.root.join(n).exists()
                && !b.root.join(n).exists()
                && n.rsplit_once('/').map(|x| x.0) == r.rsplit_once('/').map(|x| x.0)
        })
        .collect();
    if libres.is_empty() {
        return;
    }
    for (e, q) in [(&mut *a, "a"), (&mut *b, "b")] {
        let destino = libres[az.hasta(libres.len())];
        if e.root.join(r).is_file()
            && !e.s().estado.conflictos.contains_key(r)
            && !e.s().estado.conflictos.contains_key(destino)
        {
            std::fs::rename(e.root.join(r), e.root.join(destino)).unwrap();
            traza(format!("{q}: renombra (a la vez) {r} -> {destino}"));
        }
    }
    let destino = libres[az.hasta(libres.len())];
    let mut d = srv.datos();
    if let Some(id) = d.buscar(r, raiz)
        && d.buscar(destino, raiz).is_none()
    {
        d.renombrar(&id, destino.rsplit_once('/').unwrap().1);
        traza(format!("web: renombra (a la vez) {r} -> {destino}"));
    }
}

#[test]
fn caos_nunca_pierde_nada() {
    let ops: usize = std::env::var("ORBITAL_CAOS_OPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(400);
    let semilla: u64 = std::env::var("ORBITAL_CAOS_SEMILLA")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0x0610_cafe);
    let srv = ServidorFalso::iniciar();
    comun::SUBIDA_CHICA.store(true, std::sync::atomic::Ordering::Relaxed);
    srv.datos().granulo = 1024;
    let mut az = Azar(semilla);
    let mut n = 0u64;
    let mut a = Equipo::new("caos-a", &srv);
    for r in NOMBRES.iter().take(5) {
        n += 1;
        a.escribir(r, &texto(n, "a", r));
    }
    a.conectar(&srv, Destino::Nueva("Caos".into()));
    a.sincronizar();
    let raiz = a.s().estado.raiz.clone();
    let mut b = Equipo::new("caos-b", &srv);
    b.conectar(&srv, Destino::Existente(raiz.clone()));
    b.sincronizar();
    let mut debe: BTreeSet<Vec<u8>> = BTreeSet::new();
    anotar(&mut debe, &a);
    anotar(&mut debe, &b);
    let (mut exitos, mut fallos, mut cortes) = (0, 0, 0);
    let mut vuelve_la_red = 0usize;
    let mut pedido = 0u64;
    for op in 0..ops {
        if op == vuelve_la_red && srv.datos().caido {
            srv.datos().caido = false;
            traza("red vuelve".into());
        }
        match az.hasta(31) {
            0..=4 => actuar_local(&mut a, &mut az, &mut n, "a"),
            5..=9 => actuar_local(&mut b, &mut az, &mut n, "b"),
            10..=11 => actuar_web(&srv, &raiz, &mut az, &mut n),
            12..=16 => {
                let (e, q) = if az.hasta(2) == 0 {
                    (&mut a, "a")
                } else {
                    (&mut b, "b")
                };
                let r = e.ciclo();
                for ev in e.eventos.lock().unwrap().drain(..) {
                    match ev {
                        orbital_drive::Evento::Aviso(t) => traza(format!("{q}: AVISO {t}")),
                        orbital_drive::Evento::Conflicto(t) => traza(format!("{q}: CONFLICTO {t}")),
                        orbital_drive::Evento::Fusionada(t) => traza(format!("{q}: FUSIONADA {t}")),
                        _ => {}
                    }
                }
                match r {
                    Ok(()) => {
                        exitos += 1;
                        anotar(&mut debe, e);
                        let c = e.conflictos();
                        traza(format!("{q}: ciclo bien; conflictos {c:?}; {}", foto(e)));
                    }
                    Err(er) => {
                        fallos += 1;
                        traza(format!("{q}: ciclo cortado ({er})"));
                    }
                }
            }
            17 => {
                // Un corte de red de unas pocas operaciones.
                srv.datos().caido = true;
                vuelve_la_red = op + 1 + az.hasta(12);
                traza("red caída".into());
            }
            18 if srv.datos().fallas_pendientes() < 2 => {
                cortes += 1;
                srv.datos().fallar(Falla::Error(500, "backendError"));
            }
            19 if srv.datos().fallas_pendientes() < 2 => {
                cortes += 1;
                srv.datos().fallar(Falla::CortarBajada(40));
            }
            20 if srv.datos().fallas_pendientes() < 2 => {
                // Un tramo de subida reanudable cortado a mitad.
                cortes += 1;
                srv.datos().fallar(Falla::CortarSubida(1500));
            }
            21 => renombre_simultaneo(&srv, &raiz, &mut a, &mut b, &mut az),
            22 => actuar_ubicacion(&mut a, &mut az, "a"),
            23 => actuar_ubicacion(&mut b, &mut az, "b"),
            24 => {
                if az.hasta(2) == 0 {
                    editar_copia(&mut a, &mut az, &mut n, "a")
                } else {
                    editar_copia(&mut b, &mut az, &mut n, "b")
                }
            }
            25 => {
                // «Liberar espacio» cortado a mitad (después de guardar el estado, antes de
                // sacar la copia), y Orbital que vuelve a arrancar.
                let (e, q) = if az.hasta(2) == 0 {
                    (&mut a, "a")
                } else {
                    (&mut b, "b")
                };
                e.s().cortar_liberar = true;
                let r = e.ciclo();
                e.s().cortar_liberar = false;
                traza(format!(
                    "{q}: ciclo con corte al liberar -> {r:?}; reinicio"
                ));
                cortes += 1;
                e.arrancar(&srv);
            }
            26 => {
                // Orbital se cierra y vuelve a abrir (el estado sale del disco).
                let (e, q) = if az.hasta(2) == 0 {
                    (&mut a, "a")
                } else {
                    (&mut b, "b")
                };
                e.arrancar(&srv);
                traza(format!("{q}: reinicio"));
            }
            27 | 28 => {
                let (e, q) = if az.hasta(2) == 0 {
                    (&mut a, "a")
                } else {
                    (&mut b, "b")
                };
                cambio_en_cola(&srv, &raiz, e, &mut az, &mut n, q);
            }
            29 | 30 => {
                let (e, q) = if az.hasta(2) == 0 {
                    (&mut a, "a")
                } else {
                    (&mut b, "b")
                };
                // A veces con un tramo cortado antes o sin red.
                match az.hasta(4) {
                    0 if srv.datos().fallas_pendientes() < 2 => {
                        cortes += 1;
                        srv.datos().fallar(Falla::CortarBajada(30));
                    }
                    1 => {
                        srv.datos().caido = true;
                        vuelve_la_red = op + 1 + az.hasta(6);
                        traza("red caída".into());
                    }
                    _ => {}
                }
                bajada_cancelada(e, &mut az, &mut pedido, q);
            }
            _ => {}
        }
    }
    // Vuelve la red: se resuelven los conflictos y se deja que todo se asiente.
    srv.datos().caido = false;
    srv.datos().sin_fallas();
    for vuelta in 0..6 {
        for e in [&mut a, &mut b] {
            let _ = e.ciclo();
            for r in e.conflictos() {
                let _ = e.s().resolver(&r, vuelta % 2 == 0);
            }
            let _ = e.ciclo();
        }
    }
    a.sincronizar();
    b.sincronizar();
    a.sincronizar();

    // 1. Nada se perdió: cada versión que existió en un ciclo exitoso está en algún lado.
    let mut hay: BTreeSet<Vec<u8>> = a.todo().into_iter().collect();
    hay.extend(b.todo());
    hay.extend(a.copias());
    hay.extend(b.copias());
    hay.extend(srv.datos().todas_las_versiones());
    let perdidas: Vec<String> = debe
        .iter()
        .filter(|v| !hay.contains(*v))
        .map(|v| String::from_utf8_lossy(&v[..v.len().min(60)]).into_owned())
        .collect();
    if !perdidas.is_empty() && std::env::var_os("ORBITAL_CAOS_TRAZA").is_some() {
        TRAZA.with(|t| {
            for l in t.borrow().iter() {
                eprintln!("{l}");
            }
        });
        for v in debe.iter().filter(|v| !hay.contains(*v)) {
            eprintln!("PERDIDA: {}", etiqueta(v));
        }
    }
    assert!(perdidas.is_empty(), "versiones perdidas: {perdidas:#?}");

    // 2. Los dos equipos (lo que ven: la bóveda más lo «solo en Drive») y Drive quedaron iguales,
    // y lo «solo en Drive» no está en la bóveda.
    let remoto = srv.datos().arbol(&raiz);
    assert!(a.conflictos().is_empty() && b.conflictos().is_empty());
    let (sin, orbital): (
        std::collections::BTreeMap<_, _>,
        std::collections::BTreeMap<_, _>,
    ) = remoto
        .clone()
        .into_iter()
        .partition(|(r, _)| !r.starts_with(".orbital"));
    for e in [&mut a, &mut b] {
        let v = e.vista(&srv);
        assert_eq!(
            v.keys().collect::<Vec<_>>(),
            sin.keys().collect::<Vec<_>>(),
            "registros: {:?}\nnodos: {:?}",
            e.s()
                .estado
                .registros
                .iter()
                .map(|(r, b)| format!("{r}={}{}", b.id, if b.solo_drive { "·d" } else { "" }))
                .collect::<Vec<_>>(),
            {
                let st = &e.sinc.as_ref().unwrap().estado;
                let mut x: Vec<String> = st
                    .rutas()
                    .into_iter()
                    .map(|(id, r)| format!("{r}={id}:{:?}", st.nodos[&id].md5))
                    .collect();
                x.sort();
                x
            }
        );
        assert_eq!(v, sin, "lo que ve el equipo es igual a Drive");
        let cfg: std::collections::BTreeMap<_, _> = e
            .arbol()
            .into_iter()
            .filter(|(r, _)| r.starts_with(".orbital"))
            .collect();
        assert_eq!(cfg, orbital, "la configuración compartida");
        for m in e.marcadores() {
            assert!(
                !e.root.join(&m).exists(),
                "«{m}» es solo en Drive y está en la bóveda"
            );
        }
    }
    let marcas = (a.marcadores().len(), b.marcadores().len());
    let d = srv.datos();
    eprintln!(
        "caos: {ops} operaciones, {} versiones vigiladas, {exitos} ciclos bien, {fallos} cortados, {cortes} fallas provocadas, {marcas:?} solo en Drive al final (A, B), {} pedidos, {} tramos de subida cortados y {} subidas retomadas",
        debe.len(),
        d.pedidos,
        d.cortadas,
        d.reanudadas
    );
}
