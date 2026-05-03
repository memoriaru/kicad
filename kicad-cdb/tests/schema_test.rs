use kicad_cdb::ComponentDb;

#[test]
fn test_create_db_in_memory() {
    let db = ComponentDb::open_in_memory();
    assert!(db.is_ok(), "Failed to create in-memory database");
}

#[test]
fn test_create_all_tables() {
    let db = ComponentDb::open_in_memory().unwrap();

    // Verify all 8 tables exist
    let tables: Vec<String> = db.conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    assert!(tables.contains(&"categories".to_string()));
    assert!(tables.contains(&"components".to_string()));
    assert!(tables.contains(&"pins".to_string()));
    assert!(tables.contains(&"parameters".to_string()));
    assert!(tables.contains(&"simulation_models".to_string()));
    assert!(tables.contains(&"design_rules".to_string()));
    assert!(tables.contains(&"supply_info".to_string()));
    assert!(tables.contains(&"reference_circuits".to_string()));
}

#[test]
fn test_categories_crud() {
    let db = ComponentDb::open_in_memory().unwrap();

    // Insert
    let id = db.insert_category(&kicad_cdb::Category {
        id: None,
        name: "Resistor".to_string(),
        parent_id: None,
        description: Some("Resistors".to_string()),
    }).unwrap();
    assert!(id > 0);

    // Get
    let cat = db.get_category(id).unwrap().unwrap();
    assert_eq!(cat.name, "Resistor");
    assert!(cat.parent_id.is_none());

    // Get by name
    let cat2 = db.get_category_by_name("Resistor").unwrap().unwrap();
    assert_eq!(cat2.id, Some(id));

    // Delete
    assert!(db.delete_category(id).unwrap());
    assert!(db.get_category(id).unwrap().is_none());
}

#[test]
fn test_categories_tree() {
    let db = ComponentDb::open_in_memory().unwrap();

    let parent_id = db.insert_category(&kicad_cdb::Category {
        id: None,
        name: "IC".to_string(),
        parent_id: None,
        description: None,
    }).unwrap();

    let child1 = db.insert_category(&kicad_cdb::Category {
        id: None,
        name: "MCU".to_string(),
        parent_id: Some(parent_id),
        description: None,
    }).unwrap();

    let child2 = db.insert_category(&kicad_cdb::Category {
        id: None,
        name: "FPGA".to_string(),
        parent_id: Some(parent_id),
        description: None,
    }).unwrap();

    let children = db.get_child_categories(parent_id).unwrap();
    assert_eq!(children.len(), 2);
    assert!(children.iter().any(|c| c.id == Some(child1)));
    assert!(children.iter().any(|c| c.id == Some(child2)));
}

#[test]
fn test_indexes_exist() {
    let db = ComponentDb::open_in_memory().unwrap();

    let indexes: Vec<String> = db.conn
        .prepare("SELECT name FROM sqlite_master WHERE type='index' AND name LIKE 'idx_%' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    assert!(indexes.len() >= 10, "Expected at least 10 indexes, got {}", indexes.len());
}

#[test]
fn test_foreign_keys_enabled() {
    let db = ComponentDb::open_in_memory().unwrap();
    let fk_enabled: i32 = db.conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .unwrap();
    assert_eq!(fk_enabled, 1, "Foreign keys should be enabled");
}
