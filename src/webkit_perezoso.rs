//! WebKit se carga al primer uso, no al arrancar (060).
//!
//! Enlazada como de costumbre, `libwebkitgtk-6.0` (con `libjavascriptcoregtk-6.0` y las
//! bibliotecas que arrastran) se carga y se inicia antes de `main`: ≈ 43 ms de los ≈ 80 que tarda
//! el enlazador dinámico en el Celeron, aunque la mayoría de las sesiones nunca abre una vista
//! web (notas `.html`, el visor PDF, exportar a PDF).
//!
//! Este módulo define, dentro del binario, cada función de WebKit que usa Orbital como un
//! trampolín: `jmp` a una casilla que al principio apunta a un cargador. La primera llamada a
//! cualquiera de ellas guarda los registros de argumentos, abre la biblioteca con `dlopen`, llena
//! todas las casillas con `dlsym` y sigue a la función de verdad; las siguientes saltan directo.
//! Como las funciones quedan definidas acá (ocultas), el enlazador no necesita la biblioteca
//! (`--as-needed`) y no la anota como dependencia.
//!
//! **Si se usa una función de WebKit que no está en la lista**, el enlazador la toma de la
//! biblioteca y WebKit vuelve a cargarse al arrancar (todo funciona, pero se pierde la ganancia):
//! lo detecta la prueba `webkit_no_es_dependencia_del_binario`. Agregar el nombre a
//! [`webkit_perezoso!`] alcanza.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::Once;

const WEBKIT: &CStr = c"libwebkitgtk-6.0.so.4";
const JSC: &CStr = c"libjavascriptcoregtk-6.0.so.1";
const RTLD_NOW: c_int = 2;
const RTLD_GLOBAL: c_int = 0x100;

unsafe extern "C" {
    fn dlopen(nombre: *const c_char, modo: c_int) -> *mut c_void;
    fn dlsym(biblioteca: *mut c_void, simbolo: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
}

/// Arma los trampolines, sus casillas y la tabla de nombres (en el mismo orden).
macro_rules! webkit_perezoso {
    ($($f:ident),* $(,)?) => {
        const NOMBRES: &[&CStr] = &[$(
            // SAFETY: el nombre termina en el NUL que se agrega acá y no tiene otros.
            unsafe { CStr::from_bytes_with_nul_unchecked(concat!(stringify!($f), "\0").as_bytes()) }
        ),*];

        // Cada función: `f` salta a su casilla; `f.cargar` (adonde apunta la casilla al principio)
        // carga WebKit guardando los registros y vuelve a saltar a la casilla, ya llena.
        std::arch::global_asm!(
            $(
                concat!(".globl ", stringify!($f)),
                concat!(".hidden ", stringify!($f)),
                concat!(".type ", stringify!($f), ",@function"),
                concat!(stringify!($f), ":"),
                concat!("  jmp *orbital_webkit_casilla_", stringify!($f), "(%rip)"),
                concat!(stringify!($f), ".cargar:"),
                "  call orbital_webkit_cargar_guardando",
                concat!("  jmp *orbital_webkit_casilla_", stringify!($f), "(%rip)"),
                concat!(".size ", stringify!($f), ", .-", stringify!($f)),
            )*
            ".pushsection .data",
            ".balign 8",
            ".globl orbital_webkit_casillas",
            ".hidden orbital_webkit_casillas",
            "orbital_webkit_casillas:",
            $(
                concat!("orbital_webkit_casilla_", stringify!($f), ":"),
                concat!("  .quad ", stringify!($f), ".cargar"),
            )*
            ".popsection",
            options(att_syntax)
        );
    };
}

// Funciones de WebKit y JavaScriptCore que usa Orbital (directa o indirectamente, por `webkit6`;
// algunas solo las piden las compilaciones sin optimizar o las pruebas).
webkit_perezoso!(
    webkit_find_controller_count_matches,
    webkit_find_controller_get_type,
    webkit_policy_decision_get_type,
    webkit_print_operation_get_type,
    webkit_uri_request_get_type,
    webkit_uri_scheme_request_get_type,
    webkit_find_controller_search,
    webkit_find_controller_search_finish,
    webkit_find_controller_search_next,
    webkit_find_controller_search_previous,
    webkit_navigation_action_copy,
    webkit_navigation_action_free,
    webkit_navigation_action_get_navigation_type,
    webkit_navigation_action_get_request,
    webkit_navigation_policy_decision_get_navigation_action,
    webkit_navigation_policy_decision_get_type,
    webkit_network_session_get_type,
    webkit_network_session_is_ephemeral,
    webkit_network_session_new_ephemeral,
    webkit_policy_decision_ignore,
    webkit_print_operation_new,
    webkit_print_operation_print,
    webkit_print_operation_set_print_settings,
    webkit_settings_get_type,
    webkit_settings_new,
    webkit_settings_set_allow_file_access_from_file_urls,
    webkit_settings_set_enable_html5_database,
    webkit_settings_set_enable_javascript,
    webkit_settings_set_enable_javascript_markup,
    webkit_settings_set_enable_page_cache,
    webkit_settings_set_enable_webaudio,
    webkit_settings_set_enable_webgl,
    webkit_uri_request_get_uri,
    webkit_uri_scheme_request_finish,
    webkit_uri_scheme_request_get_uri,
    webkit_user_content_manager_add_script,
    webkit_user_content_manager_get_type,
    webkit_user_content_manager_new,
    webkit_user_content_manager_register_script_message_handler,
    webkit_user_script_new_for_world,
    webkit_user_script_unref,
    webkit_web_context_add_path_to_sandbox,
    webkit_web_context_get_default,
    webkit_web_context_get_type,
    webkit_web_context_new,
    webkit_web_context_register_uri_scheme,
    webkit_web_view_evaluate_javascript,
    webkit_web_view_get_network_session,
    webkit_web_view_get_snapshot,
    webkit_web_view_get_snapshot_finish,
    webkit_web_view_is_loading,
    webkit_web_view_evaluate_javascript_finish,
    webkit_web_view_get_find_controller,
    webkit_web_view_get_type,
    webkit_web_view_load_html,
    webkit_web_view_load_uri,
    webkit_web_view_new,
    webkit_web_view_set_background_color,
    webkit_web_view_set_editable,
    jsc_value_get_type,
    jsc_value_object_get_property_at_index,
    jsc_value_to_int32,
    jsc_value_to_string,
);

// Guarda los registros de argumentos (enteros, `rax` de las funciones variádicas y `xmm0`–`xmm7`),
// llama al cargador y los repone. Alineación: al entrar, `rsp` ≡ 0 (mód. 16); 7 `push` y 136
// bytes la dejan otra vez en 0 para el `call`.
std::arch::global_asm!(
    ".globl orbital_webkit_cargar_guardando",
    ".hidden orbital_webkit_cargar_guardando",
    ".type orbital_webkit_cargar_guardando,@function",
    "orbital_webkit_cargar_guardando:",
    "  push %rdi",
    "  push %rsi",
    "  push %rdx",
    "  push %rcx",
    "  push %r8",
    "  push %r9",
    "  push %rax",
    "  sub $136, %rsp",
    "  movdqu %xmm0, 0(%rsp)",
    "  movdqu %xmm1, 16(%rsp)",
    "  movdqu %xmm2, 32(%rsp)",
    "  movdqu %xmm3, 48(%rsp)",
    "  movdqu %xmm4, 64(%rsp)",
    "  movdqu %xmm5, 80(%rsp)",
    "  movdqu %xmm6, 96(%rsp)",
    "  movdqu %xmm7, 112(%rsp)",
    "  call {cargar}",
    "  movdqu 0(%rsp), %xmm0",
    "  movdqu 16(%rsp), %xmm1",
    "  movdqu 32(%rsp), %xmm2",
    "  movdqu 48(%rsp), %xmm3",
    "  movdqu 64(%rsp), %xmm4",
    "  movdqu 80(%rsp), %xmm5",
    "  movdqu 96(%rsp), %xmm6",
    "  movdqu 112(%rsp), %xmm7",
    "  add $136, %rsp",
    "  pop %rax",
    "  pop %r9",
    "  pop %r8",
    "  pop %rcx",
    "  pop %rdx",
    "  pop %rsi",
    "  pop %rdi",
    "  ret",
    ".size orbital_webkit_cargar_guardando, .-orbital_webkit_cargar_guardando",
    cargar = sym cargar,
    options(att_syntax)
);

unsafe extern "C" {
    /// Las casillas de los trampolines, en el orden de `NOMBRES`.
    static mut orbital_webkit_casillas: [usize; 0];
}

static CARGA: Once = Once::new();

/// Si WebKit ya se cargó (para la medición del arranque y las pruebas).
pub fn cargado() -> bool {
    CARGA.is_completed()
}

fn error_de_dl(que: &str) -> ! {
    // SAFETY: `dlerror` devuelve NULL o una cadena terminada en NUL.
    let e = unsafe { dlerror() };
    let detalle = if e.is_null() {
        String::new()
    } else {
        // SAFETY: no es NULL y termina en NUL.
        unsafe { CStr::from_ptr(e) }.to_string_lossy().into_owned()
    };
    eprintln!("Orbital: no se pudo cargar WebKit ({que}): {detalle}");
    std::process::abort()
}

extern "C" fn cargar() {
    CARGA.call_once(|| {
        crate::medir_arranque::marca("WebKit: carga pedida");
        // SAFETY: nombres válidos terminados en NUL; los punteros se comprueban.
        unsafe {
            let webkit = dlopen(WEBKIT.as_ptr(), RTLD_NOW | RTLD_GLOBAL);
            if webkit.is_null() {
                error_de_dl("libwebkitgtk-6.0");
            }
            if dlopen(JSC.as_ptr(), RTLD_NOW | RTLD_GLOBAL).is_null() {
                error_de_dl("libjavascriptcoregtk-6.0");
            }
            let casillas = (&raw mut orbital_webkit_casillas).cast::<usize>();
            for (i, nombre) in NOMBRES.iter().enumerate() {
                // `dlsym` con la biblioteca busca también en sus dependencias (JavaScriptCore).
                let f = dlsym(webkit, nombre.as_ptr());
                if f.is_null() {
                    error_de_dl(&nombre.to_string_lossy());
                }
                casillas.add(i).write_volatile(f as usize);
            }
        }
        crate::medir_arranque::marca("WebKit: cargado");
    });
}

#[cfg(test)]
mod tests {
    /// El binario no depende de WebKit al arrancar: ninguna función de WebKit quedó afuera de
    /// la lista (si quedara, el enlazador anotaría la biblioteca como dependencia).
    #[test]
    fn webkit_no_es_dependencia_del_binario() {
        let exe = std::env::current_exe().unwrap();
        let datos = std::fs::read(&exe).unwrap();
        let necesarias = necesarias(&datos);
        assert!(
            !necesarias.is_empty(),
            "no se leyeron las dependencias de {}",
            exe.display()
        );
        for n in &necesarias {
            assert!(
                !n.starts_with("libwebkitgtk") && !n.starts_with("libjavascriptcoregtk"),
                "{} depende de {n}: falta una función en `webkit_perezoso!` (ver \
                 `nm -D --undefined-only` del binario)",
                exe.display()
            );
        }
    }

    /// Las funciones de la lista existen en la biblioteca instalada y se cargan bien.
    #[test]
    fn webkit_se_carga_al_primer_uso() {
        super::cargar();
        assert!(super::cargado());
    }

    /// Las bibliotecas `DT_NEEDED` de un ELF de 64 bits (pruebas).
    fn necesarias(elf: &[u8]) -> Vec<String> {
        let u16_ = |o: usize| u16::from_le_bytes([elf[o], elf[o + 1]]) as usize;
        let u64_ = |o: usize| u64::from_le_bytes(elf[o..o + 8].try_into().unwrap()) as usize;
        if elf.get(..4) != Some(b"\x7fELF") || elf[4] != 2 {
            return Vec::new();
        }
        let (shoff, shentsize, shnum) = (u64_(0x28), u16_(0x3a), u16_(0x3c));
        let seccion = |i: usize| shoff + i * shentsize;
        let mut salida = Vec::new();
        for i in 0..shnum {
            let s = seccion(i);
            // SHT_DYNAMIC = 6; su `sh_link` es la tabla de cadenas.
            if u32::from_le_bytes(elf[s + 4..s + 8].try_into().unwrap()) != 6 {
                continue;
            }
            let (off, tam) = (u64_(s + 0x18), u64_(s + 0x20));
            let enlace = u32::from_le_bytes(elf[s + 0x28..s + 0x2c].try_into().unwrap()) as usize;
            let cadenas = u64_(seccion(enlace) + 0x18);
            for e in (off..off + tam).step_by(16) {
                let (etiqueta, valor) = (u64_(e), u64_(e + 8));
                if etiqueta == 1 {
                    let ini = cadenas + valor;
                    let fin = ini + elf[ini..].iter().position(|&b| b == 0).unwrap_or(0);
                    salida.push(String::from_utf8_lossy(&elf[ini..fin]).into_owned());
                }
            }
        }
        salida
    }
}
