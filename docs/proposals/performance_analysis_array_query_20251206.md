# Análisis de Rendimiento: Soporte de Queries en Arrays

## Resumen de Cambios

Se han introducido cambios en el Virtual Machine (VM) y en el Generador de Código (Codegen) para soportar la proyección implícita de campos en arrays de objetos (e.g., `{ "items.price": { $gt: 10 } }`).

### Componentes Afectados
1.  **`src/polodb_core/vm/vm.rs` - `DbOp::GetField`**: Se añadió lógica para manejar `Bson::Array`.
2.  **`src/polodb_core/vm/codegen.rs` - `emit_query_tuple`**: Se cambió la emisión de instrucciones para usar `recursively_get_field`, descomponiendo paths como `"a.b"` en `GetField("a")` -> `GetField("b")`.

---

## Análisis de Eficiencia

### 1. Costo Computacional (CPU)

*   **Iteración Lineal**: La implementación actual realiza una iteración completa sobre el array (`for item in arr`) cada vez que se accede a un campo de un array.
    *   **Complejidad**: $O(K \times M)$ por documento, donde $K$ es la profundidad del path (número de segmentos) y $M$ es el tamaño promedio de los arrays intermedios.
    *   **Impacto**: Para documentos con arrays pequeños (< 100 elementos), el impacto es despreciable. Para arrays muy grandes (e.g., miles de elementos), la iteración repetida en cada paso de evaluación del query puede ser costosa, especialmente porque no hay "short-circuiting" en la proyección (se extraen *todos* los valores antes de comparar).

### 2. Costo de Memoria (Allocations)

Este es el punto más crítico de la implementación actual.

*   **Clonación de Valores (`Clone`)**:
    En `vm.rs`:
    ```rust
    if let Some(val) = crate::utils::bson::try_get_document_value(doc, key_name) {
        result.push(val); // <--- Clone implícito u explícito
    }
    ```
    Cada vez que proyectamos un campo de un array, estamos **copiando** los datos.
    *   Si el campo es un escalar (Int, Bool), es barato.
    *   Si el campo es un `String` o un `Document`, la clonación involucra allocaciones en el heap.

*   **Vectores Intermedios (`Vec<Bson>`)**:
    ```rust
    let mut result = Vec::new(); // <--- Allocación 1
    // ...
    self.stack.push(Bson::Array(result)); // <--- Allocación 2 (Variant wrapping)
    ```
    Se crea un nuevo vector para almacenar los resultados intermedios. Si una query accede a `items.subitems.value`, se crean vectores temporales para `items` (si fuera array), luego para `subitems`, etc.

### 3. Comparación (`EqualOrContains`)

El operador `EqualOrContains` itera nuevamente sobre el array resultante para verificar la condición. Esto añade otra pasada $O(R)$ donde $R$ es el número de elementos proyectados.

---

## Impacto Negativo Potencial

1.  **Garbage Collection / Memory Pressure**: En un escaneo de colección completa (`CollScan`) sobre una colección grande, la creación y destrucción constante de vectores `Vec<Bson>` y la clonación de Strings puede generar fragmentación o presión sobre el asignador de memoria.
2.  **Latencia en Arrays Grandes**: Queries sobre arrays con miles de objetos serán notablemente más lentas que en MongoDB, que suele usar optimizaciones de iteradores o índices para evitar materializar proyecciones completas si no es necesario (e.g., para un `$elemMatch` o un simple `$eq` podría detenerse al encontrar el primer match, aunque nuestra implementación de `GetField` materializa todo primero).

---

## Conclusiones

La solución es **funcionalmente correcta** y sigue la semántica esperada, pero es **inificiente en términos de memoria** para casos de alta carga.

### Veredicto
*   🟢 **Funcionalidad**: Correcta.
*   🟡 **CPU**: Aceptable para uso general.
*   🔴 **Memoria**: Ineficiente debido a clonaciones y allocaciones temporales.

### Recomendaciones (Futuras)

1.  **Iteradores Lazy**: Modificar el VM para soportar iteradores sobre BSON sin clonar hasta que sea necesario. Esto es complejo por el modelo de ownership de Rust y `bson`.
2.  **Short-circuiting**: Implementar un operador dedicado (e.g., `ScanAndFieldCheck`) que combine la proyección y la comparación, deteniéndose al primer match para operadores como `$eq` o `$in`, evitando construir el array de resultados completo.
3.  **Cow (Clone-on-Write)**: Usar `Cow<Bson>` en el stack del VM para evitar clonaciones de lectura, aunque esto requeriría un refactor mayor del VM.
