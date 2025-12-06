# Plan de Implementación: Optimización de Array Query Support

**Fecha**: 2024-12-06  
**Estado**: ✅ Completado  
**Objetivo**: Recuperar la eficiencia de queries sobre campos no-array manteniendo soporte para implicit field projection en arrays.

---

## Resumen Ejecutivo

La implementación actual de array queries genera múltiples instrucciones VM por cada segmento de path (e.g., `"a.b"` → `GetField("a")` + `GetField("b")`). Este plan propone mover la lógica de array traversal a la función helper `try_get_document_by_slices` para permitir paths completos en una sola instrucción.

---

## Pre-requisitos

### Baseline de Tests
Antes de comenzar, ejecutar la suite completa y guardar resultados:

```bash
cd /Users/alberto.xaubet/Documents/GitHub/PoloDB
cargo test --workspace 2>&1 | tee baseline_tests.log
```

### Crear Branch de Trabajo
```bash
git checkout -b feature/array-query-optimization
```

---
## Fase 1: Implementar Array Traversal en `bson.rs`

**Objetivo**: Modificar `try_get_document_by_slices` para manejar arrays durante el traversal.

### 1.1 Modificar `try_get_document_by_slices`

```rust
// src/polodb_core/utils/bson.rs

fn try_get_document_by_slices(doc: &Document, keys: &[&str]) -> Option<Bson> {
    let first = keys.first()?;
    let remains = &keys[1..];
    let value = doc.get(*first)?;

    match value {
        Bson::Document(nested_doc) => {
            if remains.is_empty() {
                Some(Bson::Document(nested_doc.clone()))
            } else {
                try_get_document_by_slices(nested_doc, remains)
            }
        }
        Bson::Array(arr) => {
            if remains.is_empty() {
                // Devolver el array tal cual si no hay más path
                Some(Bson::Array(arr.clone()))
            } else {
                // Array projection: extraer campo de cada elemento
                let results: Vec<Bson> = arr
                    .iter()
                    .filter_map(|item| {
                        if let Bson::Document(item_doc) = item {
                            try_get_document_by_slices(item_doc, remains)
                        } else {
                            None
                        }
                    })
                    .collect();

                if results.is_empty() {
                    None
                } else {
                    Some(Bson::Array(results))
                }
            }
        }
        other => {
            if remains.is_empty() {
                Some(other.clone())
            } else {
                None // No se puede navegar más en un escalar
            }
        }
    }
}
```

### 1.2 Verificación

```bash
# Tests de bson.rs
cargo test -p polodb_core utils::bson::tests

# Suite completa para detectar regresiones
cargo test --workspace
```

**Criterio de éxito**: 
- ✅ Suite completa sin regresiones

---

## Fase 2: Simplificar Codegen

**Objetivo**: Revertir `emit_query_tuple` para emitir una sola instrucción `GetField` para paths con dots.

### 2.1 Eliminar `recursively_get_field` para paths simples

En `src/polodb_core/vm/codegen.rs`, modificar el bloque `_ =>` en `emit_query_tuple`:

```rust
// ANTES (actual):
_ => {
    let segments: Vec<&str> = key.split('.').collect();
    let first_numeric_idx = segments.iter().position(|s| s.parse::<i32>().is_ok());

    if let Some(idx) = first_numeric_idx {
        // ... lógica de GetArrayElement ...
    } else {
        // Simple key without numeric indices
        let field_size = self.recursively_get_field(key, not_found_label);
        // ...
        self.emit(DbOp::Pop2);
        self.emit_u32((field_size + 1) as u32);
    }
}

// DESPUÉS (optimizado):
_ => {
    let segments: Vec<&str> = key.split('.').collect();
    let first_numeric_idx = segments.iter().position(|s| s.parse::<i32>().is_ok());

    if let Some(idx) = first_numeric_idx {
        // Mantener lógica de GetArrayElement para índices numéricos
        // ... sin cambios ...
    } else {
        // Path sin índices numéricos: una sola instrucción GetField
        let key_static_id = self.push_static(key.into());
        self.emit_goto2(DbOp::GetField, key_static_id, not_found_label);

        let value_static_id = self.push_static(value.clone());
        self.emit_push_value(value_static_id);

        self.emit(DbOp::EqualOrContains);
        self.emit_goto(DbOp::IfFalse, not_found_label);

        self.emit(DbOp::Pop); // query value
        self.emit(DbOp::Pop); // field value
    }
}
```

### 2.2 Actualizar `emit_query_tuple_document_kv`

Aplicar el mismo patrón a los operadores `$eq`, `$gt`, `$in`, etc.:

```rust
// Ejemplo para $eq - ANTES:
"$eq" => {
    let field_size = self.recursively_get_field(key, not_found_label);
    // ...
    self.emit(DbOp::Pop2);
    self.emit_u32((field_size + 1) as u32);
}

// DESPUÉS:
"$eq" => {
    let key_static_id = self.push_static(key.into());
    self.emit_goto2(DbOp::GetField, key_static_id, not_found_label);

    let stat_val_id = self.push_static(sub_value.clone());
    self.emit_push_value(stat_val_id);
    self.emit_logical(DbOp::Equal, is_in_not);

    self.emit_goto(DbOp::IfFalse, not_found_label);

    self.emit(DbOp::Pop);
    self.emit(DbOp::Pop);
}
```

### 2.3 Verificación

```bash
# Tests de VM y codegen
cargo test -p polodb_core vm::

# Suite completa
cargo test --workspace
```

**Criterio de éxito**:
- ✅ Suite completa pasa
- ⚠️ Tests de bytecode en `subprogram.rs` fallarán (esperado, se actualizan en Fase 4)

---

## Fase 3: Actualizar Tests de Bytecode

**Objetivo**: Actualizar las expectativas de bytecode en `subprogram.rs`.

### 3.1 Tests a Modificar

| Test | Cambio Esperado |
|------|-----------------|
| `print_query_embedded_document` | `GetField("info")` + `GetField("color")` → `GetField("info.color")` |
| `print_complex_print` | `GetField("child")` + `GetField("age")` → `GetField("child.age")` |
| Otros con paths con dots | Similar simplificación |

### 3.2 Ejemplo de Actualización

```rust
#[test]
fn print_query_embedded_document() {
    // ...
    let expect = r#"Program:

0: OpenRead("test")
// ...

80: Label(0, "compare_function")
85: GetField("info.color", 110)  // ANTES: dos GetField separados
94: PushValue("yellow")
99: EqualOrContains
100: FalseJump(110)
105: Pop
106: Pop                         // ANTES: Pop2(3)

// NOTA: offsets cambiarán, recalcular
"#;
}
```

### 3.3 Verificación

```bash
cargo test -p polodb_core vm::subprogram::tests
```

**Criterio de éxito**: ✅ Todos los tests de bytecode pasan con nuevas expectativas.

---

## Fase 4: Tests de Integración E2E

**Objetivo**: Verificar comportamiento correcto con la base de datos real.

### 4.1 Ejecutar Tests de Integración Existentes

```bash
# Tests de collection
cargo test -p polodb_core --test '*'

# Si hay tests en py-polodb
cd py-polodb && python -m pytest tests/
```

### 4.2 Tests Manuales de Array Queries

Crear o verificar que existen tests para:

```rust
#[test]
fn test_array_field_projection_query() {
    let db = Database::open_memory().unwrap();
    let col = db.collection::<Document>("test");
    
    col.insert_many([
        doc! { "items": [{ "price": 10 }, { "price": 20 }] },
        doc! { "items": [{ "price": 5 }, { "price": 15 }] },
        doc! { "items": [{ "price": 100 }] },
    ]).unwrap();
    
    // Query: documentos donde algún item tiene price = 10
    let results: Vec<_> = col.find(doc! { "items.price": 10 }).unwrap().collect();
    assert_eq!(results.len(), 1);
    
    // Query: documentos donde algún item tiene price > 15
    let results: Vec<_> = col.find(doc! { "items.price": { "$gt": 15 } }).unwrap().collect();
    assert_eq!(results.len(), 2); // El de price:20 y el de price:100
}
```

### 4.3 Verificación Final

```bash
cargo test --workspace 2>&1 | tee final_tests.log
diff baseline_tests.log final_tests.log
```

**Criterio de éxito**: 
- ✅ Todos los tests pasan
- ✅ No hay regresiones vs baseline

---

## Fase 5: Cleanup y Documentación

### 5.1 Eliminar Código Muerto

- [x] Verificar si `recursively_get_field` puede eliminarse completamente → **NO**, se usa para paths con índices numéricos
- [x] Remover comentarios TODO obsoletos → No había pendientes
- [x] Verificar que el arm de `Bson::Array` en `DbOp::GetField` (vm.rs) sigue siendo necesario → Sí, para robustez

### 5.2 Actualizar Documentación

- [x] Actualizar `docs/Query.md` si existe documentación de queries → No existe
- [x] Añadir comentarios en `bson.rs` explicando la semántica de array projection → ✅ Añadidos

### 5.3 Commit Final

```bash
git add -A
git commit -m "perf: optimize array query support with single GetField instruction

- Move array traversal logic to try_get_document_by_slices in bson.rs
- Emit single GetField for dot-separated paths without numeric indices
- Maintain GetArrayElement for explicit index access (items.0.price)
- Update bytecode test expectations

Fixes performance regression introduced in array query support."
```

---

## Rollback Plan

Si en cualquier fase los tests fallan y no se puede resolver:

```bash
git checkout main -- src/polodb_core/utils/bson.rs
git checkout main -- src/polodb_core/vm/codegen.rs
git checkout main -- src/polodb_core/vm/vm.rs
cargo test --workspace  # Verificar que volvemos al estado funcional
```

---

## Comandos de Referencia

```bash
# Suite completa
cargo test --workspace

# Solo polodb_core
cargo test -p polodb_core

# Con output verbose
cargo test --workspace -- --nocapture
```

---

## Timeline Estimado

| Fase | Duración | Dependencias |
|------|----------|--------------|
| 1. Implementar en bson.rs | 1 hora | - |
| 2. Simplificar Codegen | 1-2 horas | Fase 1 |
| 3. Actualizar Tests Bytecode | 1 hora | Fase 2 |
| 4. Tests E2E | 30 min | Fase 3 |
| 5. Cleanup | 30 min | Fase 4 |

**Total estimado**: 4-5 horas

---

## Métricas de Éxito

1. **Funcionalidad**: Todas las queries existentes siguen funcionando
2. **Bytecode**: Paths simples como `"a.b"` generan una sola instrucción `GetField`
3. **Performance**: Reducción de instrucciones VM por query (medible comparando bytecode)
4. **Tests**: 100% de la suite pasa sin modificar comportamiento observable
