use super::*;
use sim_kernel::{Datum, Symbol};
use sim_relation_core::*;
use sim_relation_plan::*;
use sim_relation_schema::*;

fn name<T: TryFrom<Symbol>>(value: &str) -> T
where
    T::Error: std::fmt::Debug,
{
    T::try_from(Symbol::new(value)).unwrap()
}
fn i64_datum(value: i64) -> Datum {
    Datum::Number(sim_kernel::NumberLiteral {
        domain: Symbol::qualified("core", "i64"),
        canonical: value.to_string(),
    })
}
fn fixture() -> (DomainCatalog, Schema) {
    let domains = DomainCatalog::new([BaseDomain::I64.spec(), BaseDomain::Text.spec()]).unwrap();
    let table = TableBuilder::new(name("order"))
        .column(ColumnBuilder::required(name("id"), BaseDomain::I64.id()).build())
        .column(ColumnBuilder::required(name("select"), BaseDomain::Text.id()).build())
        .constraint(Constraint::Primary(PrimaryKey {
            name: name("pk"),
            columns: vec![name("id")],
        }))
        .build();
    let schema = SchemaBuilder::new(name("app"))
        .table(table)
        .build(&domains, &AcceptAllValues)
        .unwrap();
    (domains, schema)
}
fn checked_query() -> CheckedQuery {
    let (domains, schema) = fixture();
    let bind: BindingName = name("x\"; DROP TABLE audit; --");
    let plan = Rel::Filter {
        input: Box::new(Rel::Scan {
            source: name("main"),
            table: name("order"),
            bind: bind.clone(),
        }),
        predicate: Scalar::Call(
            ScalarOp::Eq,
            vec![
                Scalar::Field(FieldRef {
                    binding: bind,
                    field: name("select"),
                }),
                Scalar::Literal(Cell::new(
                    BaseDomain::Text.id(),
                    Some(Datum::String("Robert'); DROP TABLE students;--".into())),
                )),
            ],
        ),
    };
    admit_query(
        plan,
        &schema,
        &domains,
        RowType::new([]).unwrap(),
        AdmissionLimits::default(),
    )
    .unwrap()
}

#[test]
fn dialects_quote_the_same_attack_and_bind_every_value_differently() {
    let query = checked_query();
    let sqlite = prepare_query(&query, &SqliteDialect).unwrap();
    let postgres = prepare_query(&query, &PostgreSqlDialect).unwrap();
    assert!(sqlite.text().contains("\"x\"\"; DROP TABLE audit; --\""));
    assert!(!sqlite.text().contains("Robert"));
    assert!(sqlite.text().contains("?1"));
    assert!(
        sqlite.text().contains(
            ") AS \"x\"\"; DROP TABLE audit; --\" WHERE (\"x\"\"; DROP TABLE audit; --\".\"select\""
        ),
        "{}",
        sqlite.text()
    );
    assert!(postgres.text().contains("$1"));
    assert_ne!(sqlite.text(), postgres.text());
    assert_eq!(sqlite.bindings().len(), 1);
    assert_eq!(sqlite.cache_key().schema_id, *query.schema_id());
    assert_eq!(sqlite.cache_key().catalog_id, *query.catalog_id());
    assert_eq!(sqlite.cache_key().plan_id, *query.plan_id());
    assert_eq!(sqlite.role(), StatementRole::Query);
}

#[test]
fn sqlite_values_and_unary_relations_keep_executable_binding_scope() {
    let (domains, schema) = fixture();
    let input: BindingName = name("input");
    let row_type = RowType::new([
        FieldType {
            name: name("id"),
            domain: BaseDomain::I64.id(),
            nullable: false,
        },
        FieldType {
            name: name("select"),
            domain: BaseDomain::Text.id(),
            nullable: false,
        },
    ])
    .unwrap();
    let row = Row::new(
        row_type.clone(),
        [
            Cell::new(BaseDomain::I64.id(), Some(i64_datum(7))),
            Cell::new(BaseDomain::Text.id(), Some(Datum::String("kept".into()))),
        ],
    )
    .unwrap();
    let mutation = admit_mutation(
        Mutation::Insert {
            table: name("order"),
            columns: vec![name("id"), name("select")],
            input: Box::new(Rel::Values {
                bind: input,
                row_type,
                rows: vec![row],
            }),
            conflict: ConflictAction::Fail,
            returning: vec![],
        },
        &schema,
        &domains,
        RowType::new([]).unwrap(),
        AdmissionLimits::default(),
    )
    .unwrap();
    let prepared = prepare_mutation(&mutation, &SqliteDialect).unwrap();
    assert_eq!(
        prepared.text(),
        "INSERT INTO \"order\" (\"id\", \"select\") SELECT * FROM (SELECT ?1 AS \"id\", ?2 AS \"select\") AS \"input\""
    );

    let row_binding: BindingName = name("row");
    let query = admit_query(
        Rel::Project {
            input: Box::new(Rel::Limit {
                input: Box::new(Rel::Order {
                    input: Box::new(Rel::Filter {
                        input: Box::new(Rel::Scan {
                            source: name("main"),
                            table: name("order"),
                            bind: row_binding.clone(),
                        }),
                        predicate: Scalar::Call(
                            ScalarOp::Eq,
                            vec![
                                Scalar::Field(FieldRef {
                                    binding: row_binding.clone(),
                                    field: name("id"),
                                }),
                                Scalar::Literal(Cell::new(
                                    BaseDomain::I64.id(),
                                    Some(i64_datum(7)),
                                )),
                            ],
                        ),
                    }),
                    keys: vec![OrderKey {
                        scalar: Scalar::Field(FieldRef {
                            binding: row_binding.clone(),
                            field: name("id"),
                        }),
                        direction: OrderDirection::Asc,
                    }],
                }),
                count: Some(1),
                offset: 0,
            }),
            bind: name("output"),
            fields: vec![NamedScalar {
                name: name("select"),
                scalar: Scalar::Field(FieldRef {
                    binding: row_binding,
                    field: name("select"),
                }),
            }],
        },
        &schema,
        &domains,
        RowType::new([]).unwrap(),
        AdmissionLimits::default(),
    )
    .unwrap();
    let prepared = prepare_query(&query, &SqliteDialect).unwrap();
    assert!(
        prepared
            .text()
            .contains("AS \"row\" WHERE (\"row\".\"id\" = ?1)"),
        "{}",
        prepared.text()
    );
    assert!(
        prepared
            .text()
            .contains("AS \"row\" ORDER BY \"row\".\"id\" ASC")
    );
    assert!(prepared.text().contains("AS \"row\" LIMIT 1 OFFSET 0"));
    assert!(prepared.text().ends_with("AS \"row\""));
    assert!(!prepared.text().contains("AS \"output\""));
}

#[test]
fn sqlite_insert_select_disambiguates_conflict_from_join() {
    let (domains, schema) = fixture();
    let row_type = RowType::new([
        FieldType {
            name: name("id"),
            domain: BaseDomain::I64.id(),
            nullable: false,
        },
        FieldType {
            name: name("select"),
            domain: BaseDomain::Text.id(),
            nullable: false,
        },
    ])
    .unwrap();
    let mutation = admit_mutation(
        Mutation::Insert {
            table: name("order"),
            columns: vec![name("id"), name("select")],
            input: Box::new(Rel::Values {
                bind: name("input"),
                row_type: row_type.clone(),
                rows: vec![
                    Row::new(
                        row_type,
                        [
                            Cell::new(BaseDomain::I64.id(), Some(i64_datum(7))),
                            Cell::new(BaseDomain::Text.id(), Some(Datum::String("kept".into()))),
                        ],
                    )
                    .unwrap(),
                ],
            }),
            conflict: ConflictAction::DoNothing {
                target: sim_relation_plan::ConflictTarget::PrimaryKey,
            },
            returning: vec![],
        },
        &schema,
        &domains,
        RowType::new([]).unwrap(),
        AdmissionLimits::default(),
    )
    .unwrap();

    assert_eq!(
        prepare_mutation(&mutation, &SqliteDialect).unwrap().text(),
        "INSERT INTO \"order\" (\"id\", \"select\") SELECT * FROM (SELECT * FROM (SELECT ?1 AS \"id\", ?2 AS \"select\") AS \"input\") AS \"__sim_insert\" WHERE TRUE ON CONFLICT DO NOTHING"
    );
}

#[test]
fn capabilities_are_explicit_and_behavior_is_not_a_string_map() {
    let sqlite = SqliteDialect.caps();
    let postgres = PostgreSqlDialect.caps();
    assert!(sqlite.attach && sqlite.transaction_immediate && !sqlite.transaction_serializable);
    assert!(
        !postgres.attach && !postgres.transaction_immediate && postgres.transaction_serializable
    );
    assert!(sqlite.returning && postgres.returning && sqlite.conflict && postgres.ddl);
}

#[test]
fn emitted_ddl_round_trips_but_never_becomes_a_trusted_schema() {
    let codec = DdlCodec;
    let draft = codec
        .decode(
            "CREATE TABLE \"odd\"\"name\" (\"id\" INTEGER NOT NULL, body TEXT);",
            LegacyDdl::Sqlite,
        )
        .unwrap();
    let encoded = codec.encode(&draft).unwrap();
    let decoded = codec.decode(&encoded, LegacyDdl::Sqlite).unwrap();
    assert_eq!(decoded.tables, draft.tables);
    assert_eq!(decoded.tables[0].name, "odd\"name");
    // SchemaDraft intentionally exposes no admission-free Schema conversion.
    assert_eq!(decoded.grammar, LegacyDdl::Sqlite);
}

#[test]
fn exact_legacy_forms_lift_and_arbitrary_statements_fail_closed() {
    let codec = DdlCodec;
    let sqlite = codec
        .decode(
            "CREATE TEMP TABLE note (id INTEGER PRIMARY KEY, body TEXT)",
            LegacyDdl::Sqlite,
        )
        .unwrap();
    let hsqldb = codec.decode("CREATE CACHED TABLE ENTRY(ID INTEGER NOT NULL,NAME VARCHAR(255),CONSTRAINT PK PRIMARY KEY(ID))", LegacyDdl::Hsqldb).unwrap();
    assert_eq!(sqlite.tables[0].columns.len(), 2);
    assert_eq!(hsqldb.tables[0].name, "ENTRY");
    assert!(!hsqldb.diagnostics.is_empty());
    assert!(
        codec
            .decode("SELECT * FROM note", LegacyDdl::Sqlite)
            .is_err()
    );
    assert!(
        codec
            .decode("CREATE TABLE t (x TEXT); DROP TABLE t", LegacyDdl::Sqlite)
            .is_err()
    );
}

#[test]
fn registrations_keep_statement_decode_out_of_the_runtime_domain() {
    let registrations = sql_codec_registrations();
    assert_eq!(
        registrations
            .iter()
            .filter(|v| v.name == "codec/sql-statement" && v.decoder)
            .count(),
        0
    );
    assert!(registrations.contains(&SqlCodecRegistration {
        name: "codec/sql-ddl",
        decoder: true,
        position: CodecPosition::Data
    }));
}
