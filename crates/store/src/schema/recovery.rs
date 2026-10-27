//! Exact schema attestation for untrusted diagnostic archives.
use hermes_tern_core::error::{Error, Result};
use rusqlite::Connection;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SchemaObject {
    kind: String,
    name: String,
    table: String,
    sql: Option<String>,
}

fn differs() -> Error {
    Error::Store("archive schema definition differs")
}

pub(crate) fn validate_archive(db: &Connection) -> Result<()> {
    let actual = schema_objects(db)?;
    let mut supported = Connection::open_in_memory().map_err(|_| differs())?;
    super::initialize(&mut supported, "archive-schema-reference").map_err(|_| differs())?;
    if actual != schema_objects(&supported)? {
        return Err(differs());
    }
    Ok(())
}

fn schema_objects(db: &Connection) -> Result<Vec<SchemaObject>> {
    // Keep the version, bounds and collection on one read snapshot. Production
    // supplies an already captured read-only image, never a live journal path.
    let tx = db.unchecked_transaction().map_err(|_| differs())?;
    let version: i64 = tx
        .pragma_query_value(Some("main"), "user_version", |row| row.get(0))
        .map_err(|_| differs())?;
    // These are materialization bounds, not a total SQLite memory/latency claim.
    let bounded: bool = tx
        .query_row(
            "SELECT count(*)<=64
         AND coalesce(sum(length(CAST(type AS BLOB))+length(CAST(name AS BLOB))
             +length(CAST(tbl_name AS BLOB))+coalesce(length(CAST(sql AS BLOB)),0)),0)<=65536
         AND NOT EXISTS(SELECT 1 FROM main.sqlite_schema WHERE
             typeof(type)!='text' OR typeof(name)!='text' OR typeof(tbl_name)!='text'
             OR typeof(sql) NOT IN ('text','null')
             OR length(CAST(type AS BLOB)) NOT BETWEEN 1 AND 256
             OR length(CAST(name AS BLOB)) NOT BETWEEN 1 AND 256
             OR length(CAST(tbl_name AS BLOB)) NOT BETWEEN 1 AND 256
             OR length(CAST(sql AS BLOB))>16384)
         FROM main.sqlite_schema",
            [],
            |row| row.get(0),
        )
        .map_err(|_| differs())?;
    if version != 2 || !bounded {
        return Err(differs());
    }
    let objects = {
        let mut statement = tx.prepare(
            "SELECT type,name,tbl_name,sql FROM main.sqlite_schema
             ORDER BY type COLLATE BINARY,name COLLATE BINARY,tbl_name COLLATE BINARY,sql COLLATE BINARY LIMIT 64",
        ).map_err(|_| differs())?;
        statement
            .query_map([], |row| {
                Ok(SchemaObject {
                    kind: row.get(0)?,
                    name: row.get(1)?,
                    table: row.get(2)?,
                    sql: row.get(3)?,
                })
            })
            .map_err(|_| differs())?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|_| differs())?
    };
    tx.commit().map_err(|_| differs())?;
    Ok(objects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::types::Value;

    fn initialized() -> Connection {
        let mut db = Connection::open_in_memory().expect("open isolated fixture");
        super::super::initialize(&mut db, "schema-attestation-fixture")
            .expect("initialize supported v2 fixture");
        db
    }

    fn snapshot(db: &Connection) -> Vec<Vec<Value>> {
        let mut statement = db
            .prepare(
                "SELECT typeof(type),CAST(type AS BLOB),typeof(name),CAST(name AS BLOB),
                 typeof(tbl_name),CAST(tbl_name AS BLOB),rootpage,typeof(sql),CAST(sql AS BLOB)
                 FROM main.sqlite_schema ORDER BY rowid",
            )
            .expect("read fixture schema");
        let columns = statement.column_count();
        statement
            .query_map([], |row| {
                (0..columns).map(|column| row.get(column)).collect()
            })
            .expect("query fixture schema")
            .collect::<rusqlite::Result<_>>()
            .expect("collect fixture schema")
    }

    fn rejected_without_changes(db: &Connection) {
        let before = snapshot(db);
        let changes = db.total_changes();
        let version: i64 = db
            .query_row("PRAGMA main.schema_version", [], |row| row.get(0))
            .expect("read fixture schema cookie");
        assert!(
            matches!(
                validate_archive(db),
                Err(Error::Store("archive schema definition differs"))
            ),
            "mutated archive must fail with the safe schema refusal"
        );
        assert_eq!(snapshot(db), before, "refusal must not rewrite the archive");
        assert_eq!(db.total_changes(), changes, "refusal must not write data");
        let after: i64 = db
            .query_row("PRAGMA main.schema_version", [], |row| row.get(0))
            .expect("read unchanged schema cookie");
        assert_eq!(after, version, "refusal must preserve the schema cookie");
    }

    fn rewrite_sql(db: &Connection, name: &str, from: &str, to: &str) {
        let old: String = db
            .query_row(
                "SELECT sql FROM main.sqlite_schema WHERE name=?1",
                [name],
                |row| row.get(0),
            )
            .expect("read fixture definition");
        let new = old.replace(from, to);
        assert_ne!(new, old, "fixture mutation must change its definition");
        db.execute_batch("PRAGMA writable_schema=ON")
            .expect("enable fixture mutation");
        assert_eq!(
            db.execute(
                "UPDATE main.sqlite_schema SET sql=?1 WHERE name=?2",
                (&new, name)
            )
            .expect("rewrite fixture definition"),
            1,
            "exactly one fixture definition must change"
        );
        db.execute_batch("PRAGMA writable_schema=OFF")
            .expect("disable fixture mutation");
    }

    #[test]
    fn supported_fresh_v2_and_nullable_automatic_indexes_are_accepted() {
        let db = initialized();
        let objects = schema_objects(&db).expect("read supported schema");
        assert!(
            objects
                .iter()
                .any(|object| object.kind == "index" && object.sql.is_none()),
            "fixture must contain genuine NULL-SQL automatic indexes"
        );
        let before = snapshot(&db);
        validate_archive(&db).expect("accept exact supported v2");
        assert_eq!(
            snapshot(&db),
            before,
            "successful attestation must be read-only"
        );
    }

    #[test]
    fn unrelated_workspace_values_and_other_databases_are_ignored() {
        let db = initialized();
        db.execute(
            "UPDATE meta SET value='other-fixture' WHERE key='workspace_root'",
            [],
        )
        .expect("change fixture workspace metadata");
        db.execute_batch("CREATE TEMP TABLE unrelated(value); ATTACH DATABASE ':memory:' AS other; CREATE TABLE other.unrelated(value);")
            .expect("create unrelated non-main objects");
        validate_archive(&db).expect("only main schema objects are compared");
    }

    #[test]
    fn physical_rootpages_are_not_compared() {
        let db = initialized();
        db.execute_batch("PRAGMA writable_schema=ON; UPDATE main.sqlite_schema SET rootpage=rootpage+100 WHERE rootpage>0; PRAGMA writable_schema=OFF;")
            .expect("change only fixture physical metadata");
        validate_archive(&db).expect("physical page allocation is not schema identity");
    }

    #[test]
    fn extra_trigger_is_rejected() {
        let db = initialized();
        db.execute_batch("CREATE TRIGGER extra AFTER INSERT ON actors BEGIN SELECT 1; END;")
            .expect("add fixture trigger");
        rejected_without_changes(&db);
    }

    #[test]
    fn extra_view_is_rejected() {
        let db = initialized();
        db.execute_batch("CREATE VIEW extra AS SELECT id FROM actors;")
            .expect("add fixture view");
        rejected_without_changes(&db);
    }

    #[test]
    fn extra_index_is_rejected() {
        let db = initialized();
        db.execute_batch("CREATE INDEX extra ON actors(profile);")
            .expect("add fixture index");
        rejected_without_changes(&db);
    }

    #[test]
    fn missing_table_is_rejected() {
        let db = initialized();
        db.execute_batch("DROP TABLE events;")
            .expect("remove fixture table");
        rejected_without_changes(&db);
    }

    #[test]
    fn changed_declared_type_is_rejected() {
        let db = initialized();
        rewrite_sql(&db, "actors", "token_hash TEXT", "token_hash BLOB");
        rejected_without_changes(&db);
    }

    #[test]
    fn weakened_check_is_rejected() {
        let db = initialized();
        rewrite_sql(&db, "actors", "CHECK(kind IN ('hermes','omp'))", "CHECK(1)");
        rejected_without_changes(&db);
    }

    #[test]
    fn removed_unique_constraint_and_automatic_index_are_rejected() {
        let db = initialized();
        rewrite_sql(
            &db,
            "actors",
            "token_hash TEXT UNIQUE NOT NULL",
            "token_hash TEXT NOT NULL",
        );
        db.execute_batch("PRAGMA writable_schema=ON; DELETE FROM main.sqlite_schema WHERE name='sqlite_autoindex_actors_2'; PRAGMA writable_schema=OFF;")
            .expect("remove corresponding fixture automatic index");
        let remaining: i64 = db
            .query_row(
                "SELECT count(*) FROM main.sqlite_schema WHERE name='sqlite_autoindex_actors_2'",
                [],
                |row| row.get(0),
            )
            .expect("confirm automatic index removal");
        assert_eq!(
            remaining, 0,
            "fixture must actually omit the automatic index"
        );
        rejected_without_changes(&db);
    }

    #[test]
    fn changed_automatic_index_sql_is_rejected() {
        let db = initialized();
        db.execute_batch("PRAGMA writable_schema=ON; UPDATE main.sqlite_schema SET sql='' WHERE name='sqlite_autoindex_actors_2'; PRAGMA writable_schema=OFF;")
            .expect("replace automatic-index NULL with empty text");
        rejected_without_changes(&db);
    }

    #[test]
    fn equivalent_sql_spelling_is_deliberately_rejected() {
        let db = initialized();
        rewrite_sql(&db, "actors", "CREATE TABLE", "CREATE  TABLE");
        rejected_without_changes(&db);
    }

    #[test]
    fn unsupported_user_versions_are_rejected() {
        for version in [0, 1, 3] {
            let db = initialized();
            db.pragma_update(Some("main"), "user_version", version)
                .expect("change fixture version");
            rejected_without_changes(&db);
        }
    }

    #[test]
    fn changed_object_identity_fields_are_rejected() {
        for assignment in ["type='view'", "name='different'", "tbl_name='different'"] {
            let db = initialized();
            db.execute_batch(&format!("PRAGMA writable_schema=ON; UPDATE main.sqlite_schema SET {assignment} WHERE name='actors'; PRAGMA writable_schema=OFF;"))
                .expect("change fixture object identity");
            rejected_without_changes(&db);
        }
    }

    #[test]
    fn non_text_and_missing_required_metadata_are_rejected() {
        for assignment in [
            "type=CAST(type AS BLOB)",
            "name=CAST(name AS BLOB)",
            "tbl_name=CAST(tbl_name AS BLOB)",
            "sql=CAST(sql AS BLOB)",
            "type=NULL",
            "name=NULL",
            "tbl_name=NULL",
            "sql=NULL",
            "sql=CAST(X'80' AS TEXT)",
        ] {
            let db = initialized();
            db.execute_batch(&format!("PRAGMA writable_schema=ON; UPDATE main.sqlite_schema SET {assignment} WHERE name='actors'; PRAGMA writable_schema=OFF;"))
                .expect("change fixture metadata storage class");
            rejected_without_changes(&db);
        }
    }

    #[test]
    fn identifier_bound_counts_utf8_bytes_not_characters() {
        let db = initialized();
        let name = "é".repeat(129);
        db.execute_batch(&format!(r#"CREATE VIEW "{name}" AS SELECT 1"#))
            .expect("create oversized multibyte identifier");
        assert!(
            schema_objects(&db).is_err(),
            "reader must reject more than 256 UTF-8 bytes"
        );
        rejected_without_changes(&db);
    }

    #[test]
    fn individual_sql_is_bounded_before_collection() {
        let db = initialized();
        db.execute_batch(&format!(
            "CREATE VIEW oversized AS SELECT /*{}*/ 1",
            "x".repeat(16_385)
        ))
        .expect("create oversized fixture SQL");
        assert!(
            schema_objects(&db).is_err(),
            "reader must reject SQL beyond 16384 bytes"
        );
        rejected_without_changes(&db);
    }

    #[test]
    fn total_metadata_is_bounded_before_collection() {
        let db = initialized();
        for index in 0..5 {
            db.execute_batch(&format!(
                "CREATE VIEW bulk_{index} AS SELECT /*{}*/ 1",
                "x".repeat(13_000)
            ))
            .expect("create individually bounded fixture SQL");
        }
        let (total, largest): (i64, i64) = db.query_row(
            "SELECT sum(length(CAST(type AS BLOB))+length(CAST(name AS BLOB))+length(CAST(tbl_name AS BLOB))+coalesce(length(CAST(sql AS BLOB)),0)),max(length(CAST(sql AS BLOB))) FROM main.sqlite_schema", [],
            |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("measure fixture metadata");
        assert!(
            total > 65_536 && largest <= 16_384,
            "only the aggregate byte budget must overflow"
        );
        assert!(
            schema_objects(&db).is_err(),
            "reader must enforce the complete metadata budget"
        );
        rejected_without_changes(&db);
    }

    #[test]
    fn object_count_is_bounded_before_collection() {
        let db = initialized();
        let count: i64 = db
            .query_row("SELECT count(*) FROM main.sqlite_schema", [], |row| {
                row.get(0)
            })
            .expect("count supported fixture objects");
        assert!(count < 64, "fixture must fit the supported object budget");
        for index in count..65 {
            db.execute_batch(&format!("CREATE VIEW excess_{index} AS SELECT 1"))
                .expect("create excess fixture object");
        }
        assert!(
            schema_objects(&db).is_err(),
            "reader must reject a sixty-fifth object"
        );
        rejected_without_changes(&db);
    }

    #[test]
    fn reader_accepts_exact_count_and_multibyte_identifier_limits() {
        let db = initialized();
        let name = "é".repeat(128);
        db.execute_batch(&format!(r#"CREATE VIEW "{name}" AS SELECT 1"#))
            .expect("create exactly 256-byte fixture identifier");
        let count: i64 = db
            .query_row("SELECT count(*) FROM main.sqlite_schema", [], |row| {
                row.get(0)
            })
            .expect("count fixture schema");
        for index in count..64 {
            db.execute_batch(&format!("CREATE VIEW at_cap_{index} AS SELECT 1"))
                .expect("fill fixture to exact object cap");
        }
        assert_eq!(
            schema_objects(&db).expect("read exactly 64 objects").len(),
            64
        );
        // Materialization within bounds is not permission to adopt extra DDL.
        rejected_without_changes(&db);
    }

    #[test]
    fn reader_accepts_exact_individual_sql_limit() {
        let db = initialized();
        let prefix = "CREATE VIEW at_sql_cap AS SELECT /*";
        let suffix = "*/ 1";
        let sql = format!(
            "{prefix}{}{suffix}",
            "x".repeat(16_384 - prefix.len() - suffix.len())
        );
        db.execute_batch(&sql)
            .expect("create exactly bounded fixture SQL");
        let objects = schema_objects(&db).expect("read exactly 16384 SQL bytes");
        let object = objects
            .iter()
            .find(|row| row.name == "at_sql_cap")
            .expect("bounded view present in reader output");
        assert_eq!(object.sql.as_ref().expect("view has SQL").len(), 16_384);
        rejected_without_changes(&db);
    }

    #[test]
    fn reader_accepts_exact_total_budget_then_refuses_one_extra_byte() {
        let db = initialized();
        for index in 0..4 {
            db.execute_batch(&format!(
                "CREATE VIEW total_cap_{index} AS SELECT /*{}*/ 1",
                "x".repeat(12_000)
            ))
            .expect("create bounded aggregate fixture");
        }
        let total: i64 = db.query_row(
            "SELECT sum(length(CAST(type AS BLOB))+length(CAST(name AS BLOB))+length(CAST(tbl_name AS BLOB))+coalesce(length(CAST(sql AS BLOB)),0)) FROM main.sqlite_schema",
            [], |row| row.get(0)).expect("measure fixture aggregate");
        let prefix = "CREATE VIEW last_cap AS SELECT /*";
        let suffix = "*/ 1";
        let fixed = "view".len() + "last_cap".len() * 2 + prefix.len() + suffix.len();
        let padding = 65_536 - usize::try_from(total).expect("positive bounded total") - fixed;
        db.execute_batch(&format!("{prefix}{}{suffix}", "x".repeat(padding)))
            .expect("fill exact aggregate byte budget");
        schema_objects(&db).expect("reader accepts exactly 65536 metadata bytes");
        rewrite_sql(&db, "last_cap", "/*", "/*x");
        assert!(
            schema_objects(&db).is_err(),
            "one byte beyond total budget must refuse"
        );
        rejected_without_changes(&db);
    }
}
