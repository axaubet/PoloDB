// Copyright 2024 Vincent Chan
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use polodb_core::bson::{doc, Document};
use polodb_core::{CollectionT, IndexModel, IndexOptions, Result};

mod common;

use common::prepare_db;

/// Test that a simple scalar query on an array field matches documents
/// where the array contains that value (MongoDB behavior)
#[test]
fn test_array_contains_value() {
    let db = prepare_db("test-array-contains-value").unwrap();
    let col = db.collection::<Document>("items");

    col.insert_many(vec![
        doc! {
            "name": "Item1",
            "tags": ["rojo", "grande", "metal"]
        },
        doc! {
            "name": "Item2",
            "tags": ["azul", "pequeño", "plastico"]
        },
        doc! {
            "name": "Item3",
            "tags": ["rojo", "pequeño", "madera"]
        },
    ])
    .unwrap();

    // Query for documents where tags array contains "rojo"
    let result = col
        .find(doc! { "tags": "rojo" })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 2);
    assert!(result
        .iter()
        .any(|d| d.get("name").unwrap().as_str().unwrap() == "Item1"));
    assert!(result
        .iter()
        .any(|d| d.get("name").unwrap().as_str().unwrap() == "Item3"));

    // Query for value not in any array
    let result = col
        .find(doc! { "tags": "negro" })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 0);

    // Query for a value only in one document
    let result = col
        .find(doc! { "tags": "azul" })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].get("name").unwrap().as_str().unwrap(), "Item2");
}

/// Test that scalar fields still work correctly with the new EqualOrContains operator
#[test]
fn test_scalar_equality_still_works() {
    let db = prepare_db("test-scalar-equality-still-works").unwrap();
    let col = db.collection::<Document>("items");

    col.insert_many(vec![
        doc! {
            "name": "Item1",
            "color": "rojo"
        },
        doc! {
            "name": "Item2",
            "color": "azul"
        },
    ])
    .unwrap();

    // Query for scalar value should still work
    let result = col
        .find(doc! { "color": "rojo" })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].get("name").unwrap().as_str().unwrap(), "Item1");
}

/// Test that numeric values in arrays work correctly
#[test]
fn test_array_contains_numeric() {
    let db = prepare_db("test-array-contains-numeric").unwrap();
    let col = db.collection::<Document>("items");

    col.insert_many(vec![
        doc! {
            "name": "Item1",
            "scores": [10, 20, 30]
        },
        doc! {
            "name": "Item2",
            "scores": [40, 50, 60]
        },
        doc! {
            "name": "Item3",
            "scores": [10, 50, 90]
        },
    ])
    .unwrap();

    // Query for documents where scores contains 10
    let result = col
        .find(doc! { "scores": 10 })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 2);
    assert!(result
        .iter()
        .any(|d| d.get("name").unwrap().as_str().unwrap() == "Item1"));
    assert!(result
        .iter()
        .any(|d| d.get("name").unwrap().as_str().unwrap() == "Item3"));

    // Query for a value only in one document
    let result = col
        .find(doc! { "scores": 60 })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].get("name").unwrap().as_str().unwrap(), "Item2");
}

/// Test find_one with array contains
#[test]
fn test_array_contains_find_one() {
    let db = prepare_db("test-array-contains-find-one").unwrap();
    let col = db.collection::<Document>("items");

    col.insert_one(doc! {
        "name": "Item1",
        "tags": ["rojo", "grande", "metal"]
    })
    .unwrap();

    let result = col.find_one(doc! { "tags": "rojo" }).unwrap();
    assert!(result.is_some());
    assert_eq!(
        result.unwrap().get("name").unwrap().as_str().unwrap(),
        "Item1"
    );

    let result = col.find_one(doc! { "tags": "negro" }).unwrap();
    assert!(result.is_none());
}

/// Test combined query with array and scalar fields
#[test]
fn test_array_contains_with_other_fields() {
    let db = prepare_db("test-array-contains-with-other-fields").unwrap();
    let col = db.collection::<Document>("items");

    col.insert_many(vec![
        doc! {
            "name": "Item1",
            "category": "A",
            "tags": ["rojo", "grande"]
        },
        doc! {
            "name": "Item2",
            "category": "A",
            "tags": ["azul", "pequeño"]
        },
        doc! {
            "name": "Item3",
            "category": "B",
            "tags": ["rojo", "pequeño"]
        },
    ])
    .unwrap();

    // Query combining scalar and array
    let result = col
        .find(doc! {
            "category": "A",
            "tags": "rojo"
        })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].get("name").unwrap().as_str().unwrap(), "Item1");
}

// ============================================
// Phase 2: $in Bidirectional Tests
// ============================================

/// Test $in operator with array field - should find documents where array
/// contains ANY of the query values
#[test]
fn test_array_in_operator() {
    let db = prepare_db("test-array-in-operator").unwrap();
    let col = db.collection::<Document>("items");

    col.insert_many(vec![
        doc! {
            "name": "Item1",
            "tags": ["rojo", "grande", "metal"]
        },
        doc! {
            "name": "Item2",
            "tags": ["azul", "pequeño", "plastico"]
        },
        doc! {
            "name": "Item3",
            "tags": ["verde", "grande", "madera"]
        },
    ])
    .unwrap();

    // $in should match if array has ANY of the query values
    let result = col
        .find(doc! {
            "tags": { "$in": ["rojo", "azul"] }
        })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 2);
    assert!(result
        .iter()
        .any(|d| d.get("name").unwrap().as_str().unwrap() == "Item1"));
    assert!(result
        .iter()
        .any(|d| d.get("name").unwrap().as_str().unwrap() == "Item2"));

    // $in with no matching values
    let result = col
        .find(doc! {
            "tags": { "$in": ["negro", "blanco"] }
        })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 0);

    // $in with one common element
    let result = col
        .find(doc! {
            "tags": { "$in": ["grande"] }
        })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 2);
    assert!(result
        .iter()
        .any(|d| d.get("name").unwrap().as_str().unwrap() == "Item1"));
    assert!(result
        .iter()
        .any(|d| d.get("name").unwrap().as_str().unwrap() == "Item3"));
}

/// Test $in with scalar field still works correctly
#[test]
fn test_in_with_scalar_field() {
    let db = prepare_db("test-in-with-scalar-field").unwrap();
    let col = db.collection::<Document>("items");

    col.insert_many(vec![
        doc! { "name": "Item1", "color": "rojo" },
        doc! { "name": "Item2", "color": "azul" },
        doc! { "name": "Item3", "color": "verde" },
    ])
    .unwrap();

    let result = col
        .find(doc! {
            "color": { "$in": ["rojo", "azul"] }
        })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 2);
}

/// Test $in with numeric array
#[test]
fn test_in_with_numeric_array() {
    let db = prepare_db("test-in-with-numeric-array").unwrap();
    let col = db.collection::<Document>("items");

    col.insert_many(vec![
        doc! { "name": "Item1", "scores": [10, 20, 30] },
        doc! { "name": "Item2", "scores": [40, 50, 60] },
        doc! { "name": "Item3", "scores": [10, 50, 90] },
    ])
    .unwrap();

    let result = col
        .find(doc! {
            "scores": { "$in": [10, 40] }
        })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 3);
}

// ============================================
// Phase 3: $all Operator Tests
// ============================================

/// Test $all operator - should find documents where array contains ALL query values
#[test]
fn test_array_all_operator() {
    let db = prepare_db("test-array-all-operator").unwrap();
    let col = db.collection::<Document>("items");

    col.insert_many(vec![
        doc! {
            "name": "Item1",
            "tags": ["rojo", "grande", "metal"]
        },
        doc! {
            "name": "Item2",
            "tags": ["azul", "pequeño", "plastico"]
        },
        doc! {
            "name": "Item3",
            "tags": ["rojo", "grande", "madera"]
        },
    ])
    .unwrap();

    // $all should match only if array has ALL of the query values
    let result = col
        .find(doc! {
            "tags": { "$all": ["rojo", "grande"] }
        })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 2);
    assert!(result
        .iter()
        .any(|d| d.get("name").unwrap().as_str().unwrap() == "Item1"));
    assert!(result
        .iter()
        .any(|d| d.get("name").unwrap().as_str().unwrap() == "Item3"));

    // $all with one missing value should not match
    let result = col
        .find(doc! {
            "tags": { "$all": ["rojo", "azul"] }  // no document has both
        })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 0);

    // $all with single value works like contains
    let result = col
        .find(doc! {
            "tags": { "$all": ["metal"] }
        })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].get("name").unwrap().as_str().unwrap(), "Item1");
}

/// Test $all with numeric arrays
#[test]
fn test_all_with_numeric_array() {
    let db = prepare_db("test-all-with-numeric-array").unwrap();
    let col = db.collection::<Document>("items");

    col.insert_many(vec![
        doc! { "name": "Item1", "scores": [10, 20, 30] },
        doc! { "name": "Item2", "scores": [10, 50, 60] },
        doc! { "name": "Item3", "scores": [10, 20, 90] },
    ])
    .unwrap();

    let result = col
        .find(doc! {
            "scores": { "$all": [10, 20] }
        })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 2);
    assert!(result
        .iter()
        .any(|d| d.get("name").unwrap().as_str().unwrap() == "Item1"));
    assert!(result
        .iter()
        .any(|d| d.get("name").unwrap().as_str().unwrap() == "Item3"));
}

// ============================================
// Phase 4: Exact Array Comparison Tests
// ============================================

/// Test exact array matching - must be same elements in same order
#[test]
fn test_array_exact_match() {
    let db = prepare_db("test-array-exact-match").unwrap();
    let col = db.collection::<Document>("items");

    col.insert_many(vec![
        doc! { "name": "Item1", "tags": ["rojo", "grande", "metal"] },
        doc! { "name": "Item2", "tags": ["azul", "pequeño"] },
        doc! { "name": "Item3", "tags": ["rojo", "grande"] },
    ])
    .unwrap();

    // Exact match - same elements, same order
    let result = col
        .find(doc! { "tags": ["rojo", "grande", "metal"] })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].get("name").unwrap().as_str().unwrap(), "Item1");

    // Partial match - should match Item3 exactly
    let result = col
        .find(doc! { "tags": ["rojo", "grande"] })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].get("name").unwrap().as_str().unwrap(), "Item3");

    // Different order - should NOT match
    let result = col
        .find(doc! { "tags": ["grande", "rojo", "metal"] })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 0);
}

/// Test exact match with numeric arrays
#[test]
fn test_array_exact_match_numeric() {
    let db = prepare_db("test-array-exact-match-numeric").unwrap();
    let col = db.collection::<Document>("items");

    col.insert_many(vec![
        doc! { "name": "Item1", "scores": [10, 20, 30] },
        doc! { "name": "Item2", "scores": [10, 20] },
    ])
    .unwrap();

    let result = col
        .find(doc! { "scores": [10, 20, 30] })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].get("name").unwrap().as_str().unwrap(), "Item1");
}

// ============================================
// Phase 6: Multikey Index Tests
// ============================================

/// Test creating an index on an array field and querying with it
#[test]
fn test_multikey_index_find() {
    let db = prepare_db("test-multikey-index-find").unwrap();
    let metrics = db.metrics();
    metrics.enable();
    let initial_count = metrics.find_by_index_count();

    let col = db.collection::<Document>("items");

    // Create index on tags field BEFORE inserting data
    col.create_index(IndexModel {
        keys: doc! { "tags": 1 },
        options: Some(IndexOptions {
            name: Some("tags_idx".to_string()),
            unique: Some(false),
        }),
    })
    .unwrap();

    // Insert documents with array fields
    col.insert_many(vec![
        doc! { "name": "Item1", "tags": ["rojo", "grande", "metal"] },
        doc! { "name": "Item2", "tags": ["azul", "pequeño", "plastico"] },
        doc! { "name": "Item3", "tags": ["rojo", "pequeño", "madera"] },
    ])
    .unwrap();

    // Query using the index (should use multikey entries)
    let result = col
        .find(doc! { "tags": "rojo" })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 2);
    assert!(result
        .iter()
        .any(|d| d.get("name").unwrap().as_str().unwrap() == "Item1"));
    assert!(result
        .iter()
        .any(|d| d.get("name").unwrap().as_str().unwrap() == "Item3"));

    // ✅ Verify the index was actually used (count incremented by 1)
    assert_eq!(
        metrics.find_by_index_count(),
        initial_count + 1,
        "Index should be used for query"
    );
}

/// Test update with multikey index
#[test]
fn test_multikey_index_update() {
    let db = prepare_db("test-multikey-index-update").unwrap();
    let col = db.collection::<Document>("items");

    col.create_index(IndexModel {
        keys: doc! { "tags": 1 },
        options: Some(IndexOptions {
            name: Some("tags_idx".to_string()),
            unique: Some(false),
        }),
    })
    .unwrap();

    col.insert_one(doc! {
        "name": "Item1",
        "tags": ["rojo", "grande"]
    })
    .unwrap();

    // Update the array
    col.update_one(
        doc! { "name": "Item1" },
        doc! { "$set": { "tags": ["azul", "pequeño"] } },
    )
    .unwrap();

    // Old value should not be found
    let result = col
        .find(doc! { "tags": "rojo" })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();
    assert_eq!(result.len(), 0);

    // New value should be found
    let result = col
        .find(doc! { "tags": "azul" })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();
    assert_eq!(result.len(), 1);
}

/// Test multikey index with default options (None)
#[test]
fn test_multikey_index_default_options() {
    let db = prepare_db("test-multikey-index-default-options").unwrap();
    let metrics = db.metrics();
    metrics.enable();
    let initial_count = metrics.find_by_index_count();

    let col = db.collection::<Document>("items");

    // Create index with options: None (default behavior)
    col.create_index(IndexModel {
        keys: doc! { "tags": 1 },
        options: None, // <- Default options
    })
    .unwrap();

    col.insert_many(vec![
        doc! { "name": "Item1", "tags": ["rojo", "verde"] },
        doc! { "name": "Item2", "tags": ["azul", "verde"] },
    ])
    .unwrap();

    let result = col
        .find(doc! { "tags": "verde" })
        .run()
        .unwrap()
        .collect::<Result<Vec<Document>>>()
        .unwrap();

    assert_eq!(result.len(), 2);
    assert_eq!(
        metrics.find_by_index_count(),
        initial_count + 1,
        "Index should be used"
    );
}
