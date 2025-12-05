# Propuesta: Soporte de Queries sobre Campos Array

## Resumen

Implementar soporte completo para queries sobre campos de tipo array en documentos, replicando el comportamiento de MongoDB.

## Estado Actual

### Operadores Existentes
- `$eq`, `$gt`, `$gte`, `$lt`, `$lte`, `$ne` — comparaciones básicas
- `$in`, `$nin` — verifican si un valor escalar está en un array de búsqueda
- `$size` — verifica el tamaño del array (ya funcional)
- `$regex`, `$not`, `$and`, `$or` — operadores lógicos

### Limitaciones Actuales
1. **El valor de query no puede ser un array** — El código en `codegen.rs:568-573` devuelve error si el valor es `Bson::Array`
2. **`$in` es unidireccional** — Solo verifica si `valor_documento ∈ array_query`, no al revés
3. **No existe `$all`**
4. **No existe `$elemMatch`**
5. **Acceso por índice (`tags.0`)** — No verificado, puede funcionar parcialmente

## Funcionalidades a Implementar

| Query | Significado | Prioridad |
|-------|-------------|-----------|
| `{ tags: "rojo" }` | Array contiene "rojo" | Alta |
| `{ tags: { $in: [...] } }` | Array contiene cualquiera de esos valores | Alta |
| `{ tags: { $all: [...] } }` | Array contiene todos esos valores | Alta |
| `{ tags: ["a","b"] }` | Array es exactamente `["a","b"]` | Media |
| `{ tags: { $size: 3 } }` | Array de longitud 3 | ✅ Ya implementado |
| `{ tags.0: "rojo" }` | Primer elemento = "rojo" | Media |
| `{ array: { $elemMatch: {...} } }` | Un mismo elemento cumple varias condiciones | Baja |

---

## Plan de Implementación

### Fase 1: Query Simple sobre Array (`{ tags: "valor" }`)

**Archivos a modificar:**
- `src/polodb_core/vm/codegen.rs`
- `src/polodb_core/vm/vm.rs`
- `src/polodb_core/vm/op.rs`

**Cambios propuestos:**

#### 1.1 Modificar `emit_query_tuple` en `codegen.rs`

Actualmente (línea 575-588), cuando el valor de query es un escalar, se emite:
```rust
self.emit(DbOp::Equal);
```

**Propuesta:** Crear un nuevo operador `DbOp::EqualOrContains` que:
- Si el campo del documento es un array y el valor de query es escalar → verificar si el array contiene el valor
- Si ambos son del mismo tipo → comparación estándar de igualdad

```rust
// codegen.rs - modificar caso escalar
_ => {
    let key_static_id = self.push_static(key.into());
    self.emit_goto2(DbOp::GetField, key_static_id, not_found_label);

    let value_static_id = self.push_static(value.clone());
    self.emit_push_value(value_static_id);

    self.emit(DbOp::EqualOrContains);  // NUEVO: antes era DbOp::Equal
    self.emit_goto(DbOp::IfFalse, not_found_label);

    self.emit(DbOp::Pop);
    self.emit(DbOp::Pop);
}
```

#### 1.2 Implementar `DbOp::EqualOrContains` en `vm.rs`

```rust
DbOp::EqualOrContains => {
    let query_value = &self.stack[self.stack.len() - 1];  // valor de búsqueda
    let doc_value = &self.stack[self.stack.len() - 2];    // valor del documento

    self.r0 = match doc_value {
        Bson::Array(arr) => {
            // Si el campo es array, buscar si contiene el valor
            let found = arr.iter().any(|item| {
                matches!(
                    crate::utils::bson::value_cmp(query_value, item),
                    Ok(Ordering::Equal)
                )
            });
            if found { 1 } else { 0 }
        }
        _ => {
            // Comparación estándar de igualdad
            let cmp = crate::utils::bson::value_cmp(doc_value, query_value);
            if matches!(cmp, Ok(Ordering::Equal)) { 1 } else { 0 }
        }
    };

    self.pc = self.pc.add(1);
}
```

---

### Fase 2: Operador `$in` Bidireccional

**Comportamiento actual de `$in` (línea 812-827 en vm.rs):**
```rust
// Verifica: valor_documento ∈ array_query
for item in top1.as_array().unwrap().iter() {
    if value_cmp(top2, item) == Ok(Ordering::Equal) {
        self.r0 = 1;
        break;
    }
}
```

**Propuesta:** Modificar para manejar arrays en el documento:

```rust
DbOp::In => {
    let query_array = &self.stack[self.stack.len() - 1];  // array de búsqueda
    let doc_value = &self.stack[self.stack.len() - 2];    // valor del documento

    self.r0 = 0;
    let query_arr = query_array.as_array().unwrap();

    match doc_value {
        Bson::Array(doc_arr) => {
            // Array en documento: verificar si hay intersección
            // { tags: { $in: ["rojo", "azul"] } } donde tags = ["rojo", "grande"]
            'outer: for query_item in query_arr.iter() {
                for doc_item in doc_arr.iter() {
                    if matches!(value_cmp(query_item, doc_item), Ok(Ordering::Equal)) {
                        self.r0 = 1;
                        break 'outer;
                    }
                }
            }
        }
        _ => {
            // Valor escalar: comportamiento actual
            for item in query_arr.iter() {
                if matches!(value_cmp(doc_value, item), Ok(Ordering::Equal)) {
                    self.r0 = 1;
                    break;
                }
            }
        }
    };

    self.pc = self.pc.add(1);
}
```

---

### Fase 3: Nuevo Operador `$all`

**Archivos:**
- `src/polodb_core/vm/op.rs` — agregar `DbOp::All`
- `src/polodb_core/vm/codegen.rs` — manejar `"$all"` en `emit_query_tuple_document_kv`
- `src/polodb_core/vm/vm.rs` — implementar lógica
- `src/polodb_core/vm/subprogram.rs` — agregar caso de debug

#### 3.1 Agregar operador en `op.rs`

```rust
// Después de DbOp::In
All,  // check if array contains ALL values
```

#### 3.2 Manejar en codegen `emit_query_tuple_document_kv`

```rust
"$all" => {
    match sub_value {
        Bson::Array(_) => (),
        _ => {
            return Err(Error::InvalidField(mk_invalid_query_field(
                self.last_key().into(),
                self.gen_path(),
            )))
        }
    }

    let field_size = self.recursively_get_field(key, not_found_label);

    let stat_val_id = self.push_static(sub_value.clone());
    self.emit_push_value(stat_val_id);
    self.emit_logical(DbOp::All, is_in_not);

    self.emit_goto(DbOp::IfFalse, not_found_label);

    self.emit(DbOp::Pop2);
    self.emit_u32((field_size + 1) as u32);
}
```

#### 3.3 Implementar en vm.rs

```rust
DbOp::All => {
    let query_array = &self.stack[self.stack.len() - 1];
    let doc_value = &self.stack[self.stack.len() - 2];

    self.r0 = 0;

    if let (Bson::Array(doc_arr), Bson::Array(query_arr)) = (doc_value, query_array) {
        let all_found = query_arr.iter().all(|query_item| {
            doc_arr.iter().any(|doc_item| {
                matches!(value_cmp(query_item, doc_item), Ok(Ordering::Equal))
            })
        });
        self.r0 = if all_found { 1 } else { 0 };
    }

    self.pc = self.pc.add(1);
}
```

---

### Fase 4: Comparación Exacta de Arrays (`{ tags: ["a","b"] }`)

**Cambio en `codegen.rs`:**

Modificar línea 568-573 para permitir arrays como valores de query:

```rust
Bson::Array(_) => {
    let key_static_id = self.push_static(key.into());
    self.emit_goto2(DbOp::GetField, key_static_id, not_found_label);

    let value_static_id = self.push_static(value.clone());
    self.emit_push_value(value_static_id);

    self.emit(DbOp::ArrayEqual);  // Nuevo operador para comparación exacta
    self.emit_goto(DbOp::IfFalse, not_found_label);

    self.emit(DbOp::Pop);
    self.emit(DbOp::Pop);
}
```

**Implementar `DbOp::ArrayEqual` en vm.rs:**

```rust
DbOp::ArrayEqual => {
    let val1 = &self.stack[self.stack.len() - 2];
    let val2 = &self.stack[self.stack.len() - 1];

    self.r0 = match (val1, val2) {
        (Bson::Array(arr1), Bson::Array(arr2)) => {
            if arr1.len() != arr2.len() {
                0
            } else {
                let all_equal = arr1.iter().zip(arr2.iter()).all(|(a, b)| {
                    matches!(value_cmp(a, b), Ok(Ordering::Equal))
                });
                if all_equal { 1 } else { 0 }
            }
        }
        _ => 0
    };

    self.pc = self.pc.add(1);
}
```

---

### Fase 5: Acceso por Índice (`tags.0`)

**Archivos:**
- `src/polodb_core/utils/bson.rs` — modificar `try_get_document_value`

El método `recursively_get_field` ya divide por `.`, pero `try_get_document_value` no maneja índices numéricos en arrays.

**Propuesta:**

```rust
pub fn try_get_document_value(doc: &Document, key: &str) -> Option<Bson> {
    // Verificar si es acceso por índice a un array
    if let Ok(index) = key.parse::<usize>() {
        // Buscar si hay un campo array y acceder por índice
        // Esto requiere contexto del campo padre
        return None;  // Manejar en nivel superior
    }
    
    // Lógica existente...
    doc.get(key).cloned()
}
```

**Alternativa:** Modificar `recursively_get_field` en `codegen.rs`:

```rust
fn recursively_get_field(&mut self, key: &str, get_field_failed_label: Label) -> usize {
    let slices: Vec<&str> = key.split('.').collect();
    for slice in &slices {
        if let Ok(index) = slice.parse::<usize>() {
            // Emitir operación de acceso a array por índice
            self.emit(DbOp::ArrayGetIndex);
            self.emit_u32(index as u32);
        } else {
            let current_stat_id = self.push_static(slice.into());
            self.emit_goto2(DbOp::GetField, current_stat_id, get_field_failed_label);
        }
    }
    slices.len()
}
```

---

### Fase 6: Soporte de Índices Multikey para Arrays

**Archivos a modificar:**
- `src/polodb_core/index/index_helper.rs` — generación de claves multikey
- `src/polodb_core/collection.rs` — validación de índices únicos con arrays
- `src/polodb_core/cursor.rs` — búsqueda por índice con deduplicación

#### 6.1 Estrategia Multikey

Para un índice `{ "tags": 1 }` y un documento:

```javascript
{ _id: 1, tags: ["rojo", "grande", "metal"] }
```

se generan **múltiples entradas de índice**, una por cada elemento:

- `[$I, col_id, index_name, "rojo",   1]`
- `[$I, col_id, index_name, "grande", 1]`
- `[$I, col_id, index_name, "metal",  1]`

#### 6.2 Modificar `IndexHelper::index_item`

```rust
pub(crate) fn index_item(
    &self,
    doc: &Document,
    key_buffer: &mut BTreeMap<Box<[u8]>, Box<[u8]>>,
) -> Result<()> {
    let doc_id = doc.get("_id").ok_or_else(|| ...);
    let value = get_document_value(doc, &self.index_info.key_pattern);
    
    match value {
        Some(Bson::Array(arr)) => {
            // Multikey: generar una entrada por cada elemento
            for item in arr.iter() {
                let key = self.make_index_key(item, doc_id)?;
                key_buffer.insert(key, Box::new([]));
            }
        }
        Some(scalar_value) => {
            // Comportamiento actual: una sola entrada
            let key = self.make_index_key(&scalar_value, doc_id)?;
            key_buffer.insert(key, Box::new([]));
        }
        None => {
            // Campo no existe, indexar como null
            let key = self.make_index_key(&Bson::Null, doc_id)?;
            key_buffer.insert(key, Box::new([]));
        }
    }
    Ok(())
}
```

#### 6.3 Validación de Índices Únicos

Por simplicidad, en la primera versión **no permitir arrays en índices únicos**:

```rust
pub(crate) fn try_execute_with_index_info(
    &self,
    doc: &Document,
    index_info: &IndexInfo,
) -> Result<()> {
    let value = get_document_value(doc, &index_info.key_pattern);
    
    if index_info.is_unique() {
        if let Some(Bson::Array(_)) = value {
            return Err(Error::UniqueIndexViolation(
                "Arrays are not supported in unique indexes".into()
            ));
        }
    }
    // ...
}
```

#### 6.4 Eliminación de Índices Multikey

Al eliminar o actualizar un documento con array, se deben borrar todas las entradas:

```rust
pub(crate) fn delete_index_entries(
    &self,
    doc: &Document,
    kv_engine: &impl KvEngine,
) -> Result<()> {
    let value = get_document_value(doc, &self.index_info.key_pattern);
    let doc_id = doc.get("_id").unwrap();
    
    match value {
        Some(Bson::Array(arr)) => {
            for item in arr.iter() {
                let key = self.make_index_key(item, doc_id)?;
                kv_engine.delete(&key)?;
            }
        }
        Some(scalar_value) => {
            let key = self.make_index_key(&scalar_value, doc_id)?;
            kv_engine.delete(&key)?;
        }
        _ => {}
    }
    Ok(())
}
```

#### 6.5 Deduplicación en Consultas

Cuando se hace una consulta `$in` sobre un array indexado, el mismo documento puede aparecer múltiples veces. Es necesario deduplicar por `_id`:

```rust
// En Cursor o en el plan de ejecución
let mut seen_ids: HashSet<Bson> = HashSet::new();
while let Some(doc) = cursor.next()? {
    let id = doc.get("_id").unwrap();
    if seen_ids.insert(id.clone()) {
        results.push(doc);
    }
}
```

---

### Fase 7: Operador `$elemMatch` (Opcional)

> [!NOTE]
> Esta fase es **opcional** y se considera una mejora futura debido a su alta complejidad.

**Complejidad alta** — Requiere evaluar sub-queries sobre cada elemento del array.

```javascript
{ scores: { $elemMatch: { $gt: 80, $lt: 90 } } }
```

**Propuesta simplificada:**

```rust
"$elemMatch" => {
    let sub_doc = crate::try_unwrap_document!("$elemMatch", sub_value);
    
    let field_size = self.recursively_get_field(key, not_found_label);
    
    // Crear un loop que itere sobre cada elemento del array
    // y evalúe el sub_doc como query sobre ese elemento
    let loop_label = self.new_label();
    let found_label = self.new_label();
    let continue_label = self.new_label();
    
    self.emit(DbOp::ArrayIterStart);
    self.emit_label(loop_label);
    self.emit_goto(DbOp::ArrayIterNext, continue_label);
    
    // Evaluar sub-query sobre elemento actual
    self.emit_standard_query_doc(sub_doc, found_label, loop_label)?;
    
    self.emit_label(found_label);
    self.emit(DbOp::StoreR0_2);
    self.emit_u8(1);
    self.emit_goto(DbOp::Goto, continue_label);
    
    self.emit_label(continue_label);
    self.emit(DbOp::ArrayIterEnd);
    // ...
}
```

Esta fase requiere nuevos opcodes para iteración de arrays y es significativamente más compleja.

---

## Diseño de indexación para campos array

- **Clave de documento:** `stacked_key([col_id, _id])`.
- **Clave de índice actual (escalar):** `[$I, col_id, index_name, value, _id]`, generada por `IndexHelper::make_index_key`.
- **Búsqueda por índice:** `Cursor::reset_by_index_value` usa `make_index_key_with_query_key` para construir el prefijo `[$I, col_id, index_name, query_value]` y hacer `seek` a ese rango.

### Estrategia multikey

Para un índice `{ "tags": 1 }` y un documento:

```javascript
{ _id: 1, tags: ["rojo", "grande", "metal"] }
```

se generan varias entradas de índice:

- `[$I, col_id, index_name, "rojo",   1]`
- `[$I, col_id, index_name, "grande", 1]`
- `[$I, col_id, index_name, "metal",  1]`

Un documento con `tags: "rojo"` genera `[$I, col_id, index_name, "rojo", 2]`. Así, el prefijo `[$I, col_id, index_name, "rojo"]` cubre tanto escalares como arrays que contienen ese valor.

### Comportamiento de índices únicos

- Para la primera versión se propone **no permitir arrays** en índices con `unique = true`.
- Si `IndexInfo::is_unique()` y el valor indexado es `Bson::Array(_)`, `IndexHelper::try_execute_with_index_info` devolverá un error descriptivo.
- A futuro se podría soportar unicidad “por elemento” reutilizando `check_unique_key` por cada elemento indexado.

### Ordenación de resultados

- El orden físico de las claves es siempre `value ASC, _id ASC`.
- Para queries como `{ tags: "rojo" }` el escaneo de índice sobre `"rojo"` devuelve documentos ordenados por `_id` dentro de ese valor.
- Para `$in` con varios valores, una optimización posible es escanear los rangos en el orden de los valores y unir resultados deduplicando `_id`, produciendo un orden efectivo `(valor, _id)`.
- Cualquier `$sort` explícito en la pipeline de agregación sigue teniendo prioridad y puede reordenar completamente el resultado.

---

## Tests Propuestos

```rust
#[test]
fn test_array_contains_value() {
    let db = prepare_db("test-array-contains").unwrap();
    let col = db.collection::<Document>("items");

    col.insert_one(doc! {
        "name": "Item1",
        "tags": ["rojo", "grande", "metal"]
    }).unwrap();

    // Fase 1: Query simple
    let result = col.find_one(doc! { "tags": "rojo" }).unwrap();
    assert!(result.is_some());

    let result = col.find_one(doc! { "tags": "azul" }).unwrap();
    assert!(result.is_none());
}

#[test]
fn test_array_in_operator() {
    // Fase 2: $in bidireccional
    let result = col.find_one(doc! {
        "tags": { "$in": ["rojo", "azul"] }
    }).unwrap();
    assert!(result.is_some());
}

#[test]
fn test_array_all_operator() {
    // Fase 3: $all
    let result = col.find_one(doc! {
        "tags": { "$all": ["rojo", "grande"] }
    }).unwrap();
    assert!(result.is_some());

    let result = col.find_one(doc! {
        "tags": { "$all": ["rojo", "azul"] }
    }).unwrap();
    assert!(result.is_none());
}

#[test]
fn test_array_exact_match() {
    // Fase 4: Comparación exacta
    let result = col.find_one(doc! {
        "tags": ["rojo", "grande", "metal"]
    }).unwrap();
    assert!(result.is_some());

    let result = col.find_one(doc! {
        "tags": ["rojo", "grande"]  // Falta "metal"
    }).unwrap();
    assert!(result.is_none());
}

#[test]
fn test_array_index_access() {
    // Fase 5: Acceso por índice
    let result = col.find_one(doc! { "tags.0": "rojo" }).unwrap();
    assert!(result.is_some());

    let result = col.find_one(doc! { "tags.1": "grande" }).unwrap();
    assert!(result.is_some());
}
```

---

## Orden de Implementación Recomendado

1. **Fase 1** — Query simple `{ tags: "valor" }` (impacto alto, complejidad baja)
2. **Fase 2** — `$in` bidireccional (impacto alto, complejidad media)
3. **Fase 3** — Operador `$all` (impacto medio, complejidad media)
4. **Fase 5** — Acceso por índice `tags.0` (impacto medio, complejidad media)
5. **Fase 4** — Comparación exacta de arrays (impacto bajo, complejidad baja)
6. **Fase 6** — Índices multikey (impacto alto, complejidad alta)
7. **Fase 7** — `$elemMatch` (opcional, complejidad alta) — Mejora futura

---

## Estimación de Esfuerzo

| Fase | Funcionalidad | Complejidad | Tiempo Estimado |
|------|---------------|-------------|-----------------|
| 1    | Query simple  | Baja        | 2-4 horas       |
| 2    | `$in` bidireccional | Media | 2-3 horas       |
| 3    | `$all`        | Media       | 3-4 horas       |
| 4    | Array exacto  | Baja        | 1-2 horas       |
| 5    | Acceso índice | Media       | 3-5 horas       |
| 6    | Índices multikey | Alta     | 6-10 horas      |
| 7    | `$elemMatch` (opcional) | Alta | 8-12 horas |

**Total Fases 1-6:** ~20-28 horas de desarrollo + testing

---

## Compatibilidad

- No hay breaking changes en la API pública
- Los queries existentes seguirán funcionando igual
- Se extiende la funcionalidad sin afectar comportamiento actual
