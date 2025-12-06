
| Name | Description |
| ---------- | ----------- |
| $eq | Matches values that are equal to a specified value. |
| $gt | Matches values that are greater than a specified value. |
| $gte | Matches values that are greater than or equal to a specified value. |
| $in | Matches any of the values specified in an array. |
| $lt | Matches values that are less than a specified value. |
| $lte | Matches values that are less than or equal to a specified value. |
| $ne | Matches all values that are not equal to a specified value. |
| $nin | Matches none of the values specified in an array. |

## 1. Query basics

PoloDB usa documentos de filtro compatibles con BSON, similares a MongoDB.

- Un filtro es siempre un `Document` (mapa clave → valor).
- Las claves pueden ser nombres simples (`"name"`) o paths con puntos (`"info.color"`, `"items.price"`).
- Los paths con puntos soportan:
  - Navegación en subdocumentos.
  - Proyección implícita sobre arrays de documentos (por ejemplo, `"items.price"`).
  - Acceso por índice en arrays vía segmentos numéricos (`"tags.0"`, `"data.values.1"`).

Ejemplos básicos:

```rust
// Igualdad simple sobre un campo escalar
doc! { "name": "Alice" }

// Igualdad sobre campo anidado
doc! { "info.color": "yellow" }

// Igualdad sobre elemento de array por índice
doc! { "tags.0": "red" }
```

Cuando el valor almacenado es un array y el filtro usa un valor escalar (`{"tags": "red"}`), PoloDB
aplica semántica de *contains*: la condición es verdadera si **algún elemento del array** es igual
al valor del filtro.

---

## 2. Comparison & equality operators

Estos operadores se aplican normalmente como subdocumentos sobre un campo:

```rust
doc! { "age": { "$gt": 18 } }
```

Operadores soportados (además de igualdad simple `field: value`):

| Operator | Shape                        | Semantics |
|----------|-----------------------------|-----------|
| `$eq`    | `{ field: { "$eq": v } }`  | Igual que `field: v`, pero permite composición con otros operadores. |
| `$ne`    | `{ field: { "$ne": v } }`  | Valores distintos de `v`. |
| `$gt`    | `{ field: { "$gt": v } }`  | Mayor que `v`. |
| `$gte`   | `{ field: { "$gte": v } }` | Mayor o igual que `v`. |
| `$lt`    | `{ field: { "$lt": v } }`  | Menor que `v`. |
| `$lte`   | `{ field: { "$lte": v } }` | Menor o igual que `v`. |

### Arrays y comparación

Para campos que son arrays, estos operadores comparan **cada elemento** (implícitamente):

- `{"scores": { "$gt": 10 }}` coincide si **algún elemento** del array `scores` es `> 10`.
- El mismo comportamiento aplica a `$gte`, `$lt`, `$lte`, `$eq`, `$ne`.

---

## 3. Array semantics & operators

### 3.1 Igualdad vs contains

- `{"tags": "red"}`: *contains* → coincide si algún elemento de `tags` es exactamente `"red"`.
- `{"tags": ["red", "big"]}`: *exact array match* → el array debe ser exactamente igual (mismos
  elementos en el mismo orden).

Ejemplos (basado en tests):

```rust
// contains sobre array de strings
doc! { "tags": "rojo" }

// contains sobre array de números
doc! { "scores": 10 }

// comparación exacta de array
doc! { "tags": ["rojo", "grande", "metal"] }
```

### 3.2 Operadores `$in` y `$nin`

Formas soportadas:

- Escalar: `{"color": { "$in": ["red", "blue"] }}`.
- Array: `{"tags": { "$in": ["rojo", "azul"] }}` → coincide si el array contiene **alguno** de
  los valores de la lista.

`$nin` es la negación de `$in`: coincide cuando **ningún** elemento coincide.

### 3.3 Operador `$all`

`$all` se aplica sobre campos array y exige que el array contenga **todos** los valores indicados.

```rust
doc! { "tags": { "$all": ["rojo", "grande"] } }
```

Comportamiento (tests):

- Devuelve solo documentos cuyo array contiene todos los elementos pedidos.
- Funciona tanto con arrays de strings como numéricos.

### 3.4 Operador `$size`

`$size` compara la longitud de un array. El valor debe ser un entero (`Int64` en BSON):

```rust
doc! { "tags": { "$size": 3_i64 } }
```

Internamente PoloDB aplica `ArraySize == expected_size`.

### 3.5 Acceso por índice: `field.N`

Los segmentos numéricos en el path se interpretan como índices de array:

- `"tags.0"` → primer elemento del array `tags`.
- `"data.values.1"` → segundo elemento del array `values` dentro del subdocumento `data`.

Ejemplos:

```rust
// Primer elemento del array tags
doc! { "tags.0": "rojo" }

// Segundo elemento de un array anidado
doc! { "data.values.1": 200 }
```

Si el índice está fuera de rango (`tags.10` en un array de longitud 2), la query no devuelve
resultados.

### 3.6 Arrays de documentos y paths con puntos

Cuando un path alcanza un array de documentos, PoloDB proyecta implícitamente el resto del path
sobre cada elemento del array:

- `"items.price"` sobre `{ items: [ { price: 10 }, { price: 20 } ] }` consulta los campos `price` de
  cada elemento.
- Casos más profundos como `"metadata.attributes.key"` también están soportados.

Esto permite escribir filtros como:

```rust
doc! { "channel_ids.channel_id": "ios" }
doc! { "metadata.attributes.key": "color" }
```

---

## 4. Logical operators

### 4.1 `$and`

Forma:

```rust
doc! {
    "$and": [
        { "age": { "$gt": 18 } },
        { "active": true },
    ]
}
```

Todos los documentos del array deben coincidir (conjunción).

### 4.2 `$or`

Forma:

```rust
doc! {
    "$or": [
        { "color": "red" },
        { "color": "blue" },
    ]
}
```

Devuelve documentos que cumplan **al menos uno** de los filtros.

### 4.3 `$not`

`$not` se aplica **a nivel de campo** con otro operador dentro:

```rust
doc! {
    "age": {
        "$not": { "$eq": 18 }
    }
}
```

En los tests, se usa para negar condiciones como `{$eq: 18}`.

---

## 5. Regex queries (`$regex`)

El operador `$regex` permite filtrar usando expresiones regulares BSON (`bson::Regex`):

```rust
use bson::Regex;

doc! {
    "value": {
        "$regex": Regex {
            pattern: "c[0-9]+".into(),
            options: "i".into(),
        }
    }
}
```

Notas:

- Las opciones deben ser válidas; opciones inválidas (como `"pml"`) producen error en tiempo de
  iteración (primer `next()` del cursor falla).
- `$regex` se usa junto con igualdad sobre el campo, no mezclado con otros operadores.

---

## 6. Special handling of `_id`

El campo `_id` se trata como primary key:

- Consultas `{ "_id": value }` usan un path de acceso optimizado en el VM.
- Para agregación, `$match` con `_id` se trata igual que cualquier otro campo
  (ver `SubProgram::compile_aggregate_with_match`).

En updates, PoloDB **no permite** modificar `_id` (los operadores de update validan y fallan si
incluyes `_id`).

---

## 7. Aggregation pipeline (query avanzada)

PoloDB soporta una API de agregación basada en pipeline:

```rust
collection
    .aggregate(vec![
        doc! { "$match": { "color": "yellow" } },
        doc! { "$count": "count" },
    ])
    .run()?;
```

### 7.1 Stages soportadas

Las siguientes stages están implementadas (ver `vm::codegen::emit_aggregation_stage`):

| Stage       | Shape                         | Semantics básica |
|-------------|------------------------------|------------------|
| `$match`    | `{ "$match": { ... } }`     | Filtro igual que en `find`. |
| `$count`    | `{ "$count": "name" }`    | Cuenta documentos y devuelve un único documento `{ name: N }`. |
| `$skip`     | `{ "$skip": N }`           | Omite los primeros `N` documentos. |
| `$limit`    | `{ "$limit": N }`          | Limita el número de resultados a `N`. |
| `$sort`     | `{ "$sort": { f: 1/-1 } }` | Orden por campos (1 asc, -1 desc). |
| `$group`    | `{ "$group": { ... } }`    | Agrupación por `_id` con acumuladores. |
| `$addFields`| `{ "$addFields": { ... } }`| Añade/reescribe campos calculados. |
| `$unset`    | `{ "$unset": field(s) }`   | Elimina uno o varios campos de cada documento. |

### 7.2 `$group` y acumuladores

La stage `$group` requiere siempre un campo `_id` en su documento:

```rust
doc! {
    "$group": {
        "_id": "$color",
        "count": { "$sum": 1 },
    }
}
```

Acumuladores soportados actualmente (ver `vm::operators::OpRegistry`):

- `$sum` – contador simple por grupo (equivalente a `count` por grupo).
- `$abs` – usado principalmente vía `$addFields` para obtener el valor absoluto de un campo
  (por ejemplo, `{"$abs": "$weight"}`).

El soporte de acumuladores es intencionalmente minimalista y se centra en los casos usados por los
tests actuales.

### 7.3 `$addFields`

Permite añadir campos derivados:

```rust
doc! {
    "$addFields": {
        "abs_weight": { "$abs": "$weight" },
    }
}
```

Formas soportadas para cada valor en `$addFields`:

- Documento con un único operador de agregación (`{"$abs": "$field"}`).
- String que empieza por `$` → alias a otro campo (`"$weight"`).
- Cualquier otro valor → constante.

### 7.4 `$unset`

Puede recibir una string o un array de strings:

```rust
doc! { "$unset": "color" }
doc! { "$unset": ["color", "shape"] }
```

---

## 8. Update operators (visión rápida)

Aunque las updates se documentan en `Update.md`, para tener una visión completa de operadores
relacionados con queries, aquí se listan los soportados (ver `vm::update_operators`):

| Operator | Description |
|----------|-------------|
| `$inc`   | Incrementa un campo numérico. |
| `$min`   | Asigna el mínimo entre el valor actual y el nuevo. |
| `$max`   | Asigna el máximo entre el valor actual y el nuevo. |
| `$mul`   | Multiplica el campo por un factor. |
| `$rename`| Renombra un campo. |
| `$set`   | Asigna un nuevo valor al campo. |
| `$unset` | Elimina el campo. |
| `$push`  | Añade un elemento al final de un array (o crea uno nuevo). |
| `$pop`   | Elimina el primer (`-1`) o último (`1`) elemento de un array. |

Todas estas operaciones **prohíben modificar** `_id`.

---

## 9. Performance tips for efficient queries

- Crea índices en los campos más consultados, incluyendo arrays (soportan multikey index).
- Usa paths sin índices numéricos (`"items.price"` en lugar de `"items.0.price"`) cuando no
  necesites una posición exacta: PoloDB puede resolverlos con una sola instrucción `GetField`.
- Para arrays de objetos, prefiere paths como `"items.price"` o `"metadata.attributes.key"`, que
  aprovechan la proyección implícita sobre arrays.
- Evita regex con opciones inválidas: fallarán tarde (en el cursor) y son más costosas que
  comparaciones exactas o por rango.

