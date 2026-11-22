# Crear el proyecto de Google Cloud para sincronizar la bóveda con Drive

Orbital sincroniza la bóveda con Google Drive usando **un proyecto de Google Cloud tuyo**. Es
gratis, se hace una sola vez (unos 15 minutos) y te da dos datos: un **ID de cliente** y un
**secreto de cliente**. Se los das a Orbital en «Google Drive…» y quedan guardados en el llavero de
GNOME, no en la bóveda ni en ningún archivo de Orbital.

¿Por qué un proyecto propio? Para leer y escribir en todo tu Drive, Google pide una auditoría de
seguridad anual a las apps que se reparten a todo el mundo. Un proyecto para uso personal no la
necesita: solo muestra una vez el aviso «Google no verificó esta app».

Hace falta una cuenta de Google y el navegador. La consola de Google cambia los nombres de vez en
cuando: entre paréntesis va el nombre anterior.

## 1. Crear el proyecto

1. Entrá a <https://console.cloud.google.com/> con tu cuenta de Google. Si es la primera vez,
   aceptá las condiciones del servicio.
2. Arriba, en el selector de proyectos, elegí **Proyecto nuevo**.
3. Nombre del proyecto: `Orbital` (puede ser cualquiera). Organización: **Sin organización**.
4. **Crear**. Esperá a que termine y fijate de que el selector de arriba muestre el proyecto nuevo.

## 2. Habilitar la API de Google Drive

1. Menú ☰ → **APIs y servicios** → **Biblioteca**.
2. Buscá `Google Drive API` y entrá en el resultado de Google.
3. **Habilitar**.

## 3. Configurar la pantalla de consentimiento

1. Menú ☰ → **Google Auth Platform** (antes: APIs y servicios → **Pantalla de consentimiento de
   OAuth**). Si aparece **Comenzar**, tocalo.
2. **Información de la app**: nombre `Orbital (personal)` y tu correo como correo de asistencia.
   **Siguiente**.
3. **Público**: elegí **Externo**. **Siguiente**.
4. **Información de contacto**: tu correo. **Siguiente**.
5. Aceptá la política de datos de usuario de los servicios de las API de Google y tocá **Crear**.
6. En **Público** (antes, en la misma pantalla de consentimiento), en «Estado de publicación»,
   tocá **Publicar app** y confirmá. Tiene que quedar **En producción**.

   **Este paso importa:** si la app queda «En prueba» (*Testing*), Google vence el inicio de
   sesión cada 7 días y Orbital deja de sincronizar hasta que vuelvas a entrar.
7. En **Acceso a los datos** (antes: «Permisos» o *Scopes*), tocá **Agregar o quitar permisos**,
   marcá `https://www.googleapis.com/auth/drive` («Ver, editar, crear y borrar todos tus archivos
   de Google Drive»), **Actualizar** y **Guardar**. Si no aparece en la lista, pegalo en «Agregar
   permisos manualmente». Google lo marca como permiso restringido: es lo esperado.

## 4. Crear el cliente «App de escritorio»

1. En **Google Auth Platform** → **Clientes** (antes: APIs y servicios → **Credenciales** →
   **Crear credenciales** → **ID de cliente de OAuth**), tocá **Crear cliente**.
2. Tipo de aplicación: **App de escritorio**. Nombre: `Orbital`.
3. **Crear**.
4. Se muestran el **ID de cliente** (termina en `.apps.googleusercontent.com`) y el **secreto del
   cliente** (empieza con `GOCSPX-`). **Descargá el JSON** o copiá los dos ahora: Google puede no
   volver a mostrar el secreto. Si lo perdés, en el cliente podés agregar un secreto nuevo.

No compartas el secreto ni lo pongas en la bóveda: con él y tu permiso, cualquiera podría pedir
acceso a tu Drive en nombre de tu proyecto.

## 5. Conectar Orbital

1. En Orbital, abrí la bóveda y elegí **Google Drive…** (en el menú de la bóveda, abajo a la
   izquierda, o en la paleta de comandos).
2. Pegá el **ID del cliente** y el **secreto del cliente**.
3. Tocá **Iniciar sesión con Google**. Se abre el navegador:
   1. Elegí tu cuenta.
   2. Aparece «**Google no verificó esta app**»: tocá **Avanzado** (o «Configuración avanzada»)
      y después **Ir a Orbital (personal) (no seguro)**. Es tu propio proyecto.
   3. Revisá el permiso (ver, editar, crear y borrar tus archivos de Drive) y tocá **Continuar**.
   4. El navegador muestra «Listo: ya podés volver a Orbital».
4. En Orbital, elegí la carpeta: **Crear la carpeta «…» en Mi unidad** (la primera vez) o **Usar
   una carpeta que ya está en Drive** (pegando su enlace, para la segunda computadora), y tocá
   **Conectar**.

Para la **otra computadora**: hacé los pasos 5.1 a 5.4 con el mismo ID y secreto (sirven en todas
tus computadoras), abriendo en Orbital una carpeta vacía y eligiendo «Usar una carpeta que ya está
en Drive» con el enlace de la carpeta de la bóveda.

## Si algo falla

| Lo que pasa | Qué hacer |
|---|---|
| «Error 403: access_denied» al iniciar sesión | La app está «En prueba» y tu cuenta no figura como usuaria de prueba: publicala (paso 3.6) |
| Cada semana hay que volver a iniciar sesión | La app quedó «En prueba»: publicala (paso 3.6) y volvé a iniciar sesión |
| «Google Drive API has not been used in project…» | Falta habilitar la API (paso 2) o elegiste otro proyecto en el selector |
| «redirect_uri_mismatch» | El cliente no es «App de escritorio»: creá otro de ese tipo (paso 4) |
| «invalid_client» | El ID o el secreto están mal copiados (sin espacios al principio ni al final) |

## Quitarle el acceso a Orbital

En <https://myaccount.google.com/permissions> elegí «Orbital (personal)» → **Quitar acceso**.
Orbital lo nota en la próxima sincronización, la pausa y pide volver a iniciar sesión. No se borra
nada, ni en la computadora ni en Drive.
