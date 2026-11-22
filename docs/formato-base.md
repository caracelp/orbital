# Formato de las bases (`.base`)

Complemento de [formato-boveda.md](formato-boveda.md#bases-sobre-notas).

El archivo `.base` es YAML UTF-8 versionado, sin datos duplicados de las notas.
No se promete compatibilidad con otros formatos llamados `.base`.

```yaml
format_version: 1
view: table
columns: [{property: title, label: Título, type: auto}, {property: precio, label: Precio, type: number}]
filters: [{folder: wiki/proyectos}, {tag: activo}, {property: estado, value: abierto}]
sort: {property: precio, descending: false}
```

Las claves admitidas son exactamente `format_version`, `view`, `columns`, `filters`, `sort`.
Una clave desconocida, versión distinta de 1 o YAML inválido bloquea la edición del archivo,
con explicación; nunca se reescribe automáticamente. `format_version` es obligatorio; los demás campos
pueden omitirse: tabla, columna título, sin filtros y orden por ruta. La emisión es canónica,
con el emisor YAML existente; al consultar no se escribe.

`view`: `table` o `cards`. Cada columna tiene `property`, `label` y `type`;
tipos: `auto`, `text`, `number`, `bool`, `date`, `datetime`, `link`, `list`.
Los mapas existentes se muestran en automático. Declarar un tipo no convierte datos.
Propiedad ausente se representa con `None`, nulo con `Some(PropValue::Null)`.
Las propiedades especiales de consulta `file.path` y `file.name` son de solo lectura.

Los filtros forman una conjunción. `folder` incluye descendientes con límite de componente
("wiki/a" no incluye "wiki/abc"); `tag` exige etiqueta exacta; `property` sin `value`
exige presencia y con `value` exige igualdad de valores tipados. Las propiedades del bloque
prevalecen sobre los campos inline. Las rutas son relativas a la bóveda, sin escapes.
`sort` admite propiedad y booleano `descending`; números comparan por valor, ausentes y nulos
se agrupan separadamente; todos los empates se resuelven por ruta ascendente.

## API Rust de `orbital_vault::bases`

```rust
BaseDefinition { version: u32, view: BaseView, columns: Vec<BaseColumn>,
                 filters: Vec<BaseFilter>, sort: Option<BaseSort> }
BaseColumn { property: String, label: String, kind: PropertyType }
BaseSort { property: String, descending: bool }
BaseView::{Table, Cards}
PropertyType::{Auto, Text, Number, Bool, Date, DateTime, Link, List}
BaseFilter::{Folder(String), Tag(String), Property { key: String, value: Option<PropValue> }}
BaseDocument { definition: BaseDefinition, hash: u64 }
BaseRow { path: RelPath, title: String, hash: u64, read_only: bool,
          properties: Vec<(String, PropValue)>, error: Option<String> }
BaseSnapshot { rows: Vec<BaseRow>, errors: Vec<(RelPath, String)> }
```

`BaseDefinition::default()` da una tabla de títulos sin filtros.
`BaseRow::value(&str) -> Option<&PropValue>` consulta valores efectivos.
`BaseIndex::build(&Vault) -> BaseIndex` lee notas Markdown y HTML una vez.
`BaseIndex::update(&mut self, &Vault, &RelPath)` relee o retira una sola ruta.
`BaseIndex::query(&self, &BaseDefinition) -> BaseSnapshot` filtra y ordena en memoria.
El índice es perezoso: la ventana lo comparte por bóveda y lo construye fuera del hilo GTK
al primer uso, incluidas incrustaciones. El cambio de bóveda invalida cualquier resultado pendiente.

Funciones públicas:

```rust
parse(&str) -> Result<BaseDefinition, String>
emit(&BaseDefinition) -> String
read(&Vault, &RelPath) -> Result<BaseDocument, VaultError>
save(&Vault, &RelPath, &BaseDefinition, Option<u64>, &WriteCtx)
    -> Result<WriteReceipt, VaultError>
validate_value(PropertyType, &PropValue) -> bool
parse_value(PropertyType, &str) -> Result<PropValue, String>
```

`save` usa GuardadoSeguro, hash esperado y contención canónica. Crear no sobrescribe;
guardar configuración existente exige el hash leído. Las rutas de `raw/` no se escriben.
La UI valida cada celda y edita sobre el buffer si la nota está abierta, conservando deshacer
y autoguardado; fuera del editor usa `Vault::set_prop` con la versión leída.

## Renombrar propiedades

La planificación visita notas y definiciones de base y devuelve un `EditPlan` más conflictos.
Si origen y destino tienen valores diferentes, conserva la nota completa e informa el conflicto.
Si son iguales, quita únicamente origen; si destino falta, renombra origen. Claves reservadas
OKF quedan excluidas. Actualiza columnas, filtros y orden de bases válidas; informa bases inválidas.
La aplicación reutiliza `apply_edits`, y deshacer usa únicamente `plan.applied(receipt)` para
no revertir archivos que fallaron. Todo guardado verifica hash y contención y muestra errores parciales.
Los cambios de claves que afectan el índice OKF requieren su sincronización explícita.

## Incrustaciones

`![[x.base]]` resuelve nombres de base sin mezclar archivos con el índice de nombres de notas.
La consulta produce el mismo snapshot que la vista completa. La representación Markdown usa
una tabla `RenderDoc` y HTML genera markup escapado efímero, excluido del guardado de la nota.
Además de cambiar la definición, cualquier cambio en notas puede alterar filtros: invalida
incrustaciones por revisión del índice, incluso cuando una fila entra o sale del resultado.

## Límites explícitos de esta implementación

Las incrustaciones muestran hasta 100 notas en la vista elegida (tabla o tarjetas). Si hay más,
lo indican y el título abre la base completa, cuya interfaz usa listas virtualizadas.
Los enlaces generados usan rutas de bóveda codificadas (`orbital-path:`), independientes de la
carpeta de la nota anfitriona; no son enlaces a archivos fuera de la bóveda.

El renombre masivo opera sobre propiedades del bloque YAML. Si encuentra una propiedad de origen
inline, informa la omisión y conserva la nota. Un destino inline con valor diferente también es un
conflicto. YAML de flujo o claves complejas solo se modifican si la postcondición demuestra que se
conservaron todas las demás propiedades y el cuerpo; en caso contrario se informa y no se escribe.
Editar un campo inline desde una celda crea una propiedad en el bloque (override explícito),
sin reescribir el cuerpo. Quitar ese override puede volver a mostrar el valor inline original.
