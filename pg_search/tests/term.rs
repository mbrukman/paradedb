mod fixtures;

use fixtures::*;
use pretty_assertions::assert_eq;
use rstest::*;
use sqlx::PgConnection;

#[rstest]
fn boolean_term(mut conn: PgConnection) {
    r#"
    CREATE TABLE test_table (
    	id SERIAL PRIMARY KEY,
    	value BOOLEAN
    );

    INSERT INTO test_table (value) VALUES (true), (false), (false), (true);
    "#
    .execute(&mut conn);

    r#"
    CALL paradedb.create_bm25(
    	table_name => 'test_table',
    	index_name => 'test_index',
    	key_field => 'id',
    	boolean_fields => '{"value": {}}'
    );
    "#
    .execute(&mut conn);

    let rows: Vec<(i32, bool)> = r#"
    SELECT * FROM test_index.search(
    	query => paradedb.term(field => 'value', value => true),
    	stable_sort => true
    );
    "#
    .fetch_collect(&mut conn);
    assert_eq!(rows, vec![(1, true), (4, true)]);
}

#[rstest]
fn integer_term(mut conn: PgConnection) {
    r#"
    CREATE TABLE test_table (
    	id SERIAL PRIMARY KEY,
    	value_int2 SMALLINT,
    	value_int4 INTEGER,
    	value_int8 BIGINT
    );

    INSERT INTO test_table (value_int2, value_int4, value_int8) VALUES 
    	(-11, -1111, -11111111),
    	(22, 2222, 22222222), 
    	(33, 3333, 33333333), 
    	(44, 4444, 44444444);
    "#
    .execute(&mut conn);

    r#"
    CALL paradedb.create_bm25(
    	table_name => 'test_table',
    	index_name => 'test_index',
    	key_field => 'id',
    	numeric_fields => '{"value_int2": {}, "value_int4": {}, "value_int8": {}}'
    );
    "#
    .execute(&mut conn);

    // INT2
    let rows: Vec<(i32, i16)> = r#"
    SELECT id, value_int2 FROM test_index.search(
    	query => paradedb.term(field => 'value_int2', value => -11),
    	stable_sort => true
    );
    "#
    .fetch_collect(&mut conn);
    assert_eq!(rows, vec![(1, -11)]);

    // INT4
    let rows: Vec<(i32, i32)> = r#"
    SELECT id, value_int4 FROM test_index.search(
    	query => paradedb.term(field => 'value_int4', value => 2222),
    	stable_sort => true
    );
    "#
    .fetch_collect(&mut conn);
    assert_eq!(rows, vec![(2, 2222)]);

    // INT8
    let rows: Vec<(i32, i64)> = r#"
    SELECT id, value_int8 FROM test_index.search(
    	query => paradedb.term(field => 'value_int8', value => 33333333),
    	stable_sort => true
    );
    "#
    .fetch_collect(&mut conn);
    assert_eq!(rows, vec![(3, 33333333)]);
}

#[rstest]
fn float_term(mut conn: PgConnection) {
    r#"
    CREATE TABLE test_table (
    	id SERIAL PRIMARY KEY,
    	value_float4 FLOAT4,
    	value_float8 FLOAT8
    );

    INSERT INTO test_table (value_float4, value_float8) VALUES 
    	(1.1, 1111.1111), 
    	(2.2, 2222.2222), 
    	(3.3, 3333.3333), 
    	(4.4, 4444.4444);
    "#
    .execute(&mut conn);

    r#"
    CALL paradedb.create_bm25(
    	table_name => 'test_table',
    	index_name => 'test_index',
    	key_field => 'id',
    	numeric_fields => '{"value_float4": {}, "value_float8": {}}'
    );
    "#
    .execute(&mut conn);

    // FLOAT4
    let rows: Vec<(i32, f32)> = r#"
    SELECT id, value_float4 FROM test_index.search(
    	query => paradedb.term(field => 'value_float4', value => 1.1::float4),
    	stable_sort => true
    );
    "#
    .fetch_collect(&mut conn);
    assert_eq!(rows, vec![(1, 1.1)]);

    // FLOAT8
    let rows: Vec<(i32, f64)> = r#"
    SELECT id, value_float8 FROM test_index.search(
    	query => paradedb.term(field => 'value_float8', value => 4444.4444::float8),
    	stable_sort => true
    );
    "#
    .fetch_collect(&mut conn);
    assert_eq!(rows, vec![(4, 4444.4444)]);
}

#[rstest]
fn text_term(mut conn: PgConnection) {
    r#"
    CREATE TABLE test_table (
        id SERIAL PRIMARY KEY,
        value_text TEXT,
        value_varchar VARCHAR(64),
        value_uuid UUID
    );

    INSERT INTO test_table (value_text, value_varchar, value_uuid) VALUES
        ('abc', 'var abc', 'a99e7330-37e6-4f14-8c95-985052ee74f3'::uuid),
        ('def', 'var def', '2fe779f1-2a74-4035-9f1a-9477bae0364c'::uuid),
        ('ghi', 'var ghi', 'b9592b87-82ea-4d7b-8865-f6be819d4f0f'::uuid),
        ('jkl', 'var jkl', 'ae9d4a8c-8382-452d-96fb-a9a1c4192a03'::uuid);
    "#
    .execute(&mut conn);

    r#"
    CALL paradedb.create_bm25(
        table_name => 'test_table',
        index_name => 'test_index',
        key_field => 'id',
        text_fields => '{"value_text": {}, "value_varchar": {}, "value_uuid": {tokenizer: { type: "raw" }, normalizer: "raw", record: "basic", fieldnorms: false}}'
    );
    "#
    .execute(&mut conn);

    // TEXT
    let rows: Vec<(i32, String)> = r#"
    SELECT id, value_text FROM test_index.search(
        query => paradedb.term(field => 'value_text', value => 'abc'),
        stable_sort => true
    );
    "#
    .fetch_collect(&mut conn);
    assert_eq!(rows, vec![(1, "abc".into())]);

    // VARCHAR
    let rows: Vec<(i32, String)> = r#"
    SELECT id, value_varchar FROM test_index.search(
        query => paradedb.term(field => 'value_varchar', value => 'ghi'),
        stable_sort => true
    );
    "#
    .fetch_collect(&mut conn);
    assert_eq!(rows, vec![(3, "var ghi".into())]);

    // UUID - sqlx doesn't have a uuid type, so we just look for id
    let rows: Vec<(i32,)> = r#"
    SELECT id FROM test_index.search(
        query => paradedb.term(field => 'value_uuid', value => 'ae9d4a8c-8382-452d-96fb-a9a1c4192a03'),
        stable_sort => true
    );
    "#
    .fetch_collect(&mut conn);
    assert_eq!(rows, vec![(4,)]);
}

#[rstest]
fn datetime_term(mut conn: PgConnection) {
    r#"
    CREATE TABLE test_table (
        id SERIAL PRIMARY KEY,
        value_date DATE,
        value_timestamp TIMESTAMP,
        value_timestamptz TIMESTAMP WITH TIME ZONE,
        value_time TIME,
        value_timetz TIME WITH TIME ZONE
    );

    INSERT INTO test_table (value_date, value_timestamp, value_timestamptz, value_time, value_timetz) VALUES 
        (DATE '2023-05-03', TIMESTAMP '2023-04-15 13:27:09', TIMESTAMP WITH TIME ZONE '2023-04-15 13:27:09 PST', TIME '08:09:10', TIME WITH TIME ZONE '08:09:10 PST'),
        (DATE '2021-06-28', TIMESTAMP '2019-08-02 07:52:43.123', TIMESTAMP WITH TIME ZONE '2019-08-02 07:52:43.123 EST', TIME '11:43:21.456', TIME WITH TIME ZONE '11:43:21.456 EST');
    "#
    .execute(&mut conn);

    r#"
    CALL paradedb.create_bm25(
        table_name => 'test_table',
        index_name => 'test_index',
        key_field => 'id',
        datetime_fields => '{"value_date": {}, "value_timestamp": {}, "value_timestamptz": {}, "value_time": {}, "value_timetz": {}}'
    );
    "#
    .execute(&mut conn);

    // DATE
    let rows: Vec<(i32,)> = r#"
    SELECT * FROM test_index.search(
        query => paradedb.term(field => 'value_date', value => DATE '2023-05-03')
    );
    "#
    .fetch_collect(&mut conn);
    assert_eq!(rows, vec![(1,)]);

    // TIMESTAMP
    let rows: Vec<(i32,)> = r#"
    SELECT * FROM test_index.search(
        query => paradedb.term(field => 'value_timestamp', value => TIMESTAMP '2019-08-02 07:52:43.123')
    );
    "#
    .fetch_collect(&mut conn);
    assert_eq!(rows, vec![(2,)]);

    // TIMESTAMP WITH TIME ZONE
    let rows: Vec<(i32,)> = r#"
    SELECT * FROM test_index.search(
        query => paradedb.term(field => 'value_timestamptz', value => TIMESTAMP WITH TIME ZONE '2023-04-15 13:27:09 PST')
    );
    "#
    .fetch_collect(&mut conn);
    assert_eq!(rows, vec![(1,)]);

    // TIMESTAMP WITH TIME ZONE: Change time zone in query
    let rows: Vec<(i32,)> = r#"
    SELECT * FROM test_index.search(
        query => paradedb.term(field => 'value_timestamptz', value => TIMESTAMP WITH TIME ZONE '2023-04-15 16:27:09 EST')
    );
    "#
    .fetch_collect(&mut conn);
    assert_eq!(rows, vec![(1,)]);

    // TIME
    let rows: Vec<(i32,)> = r#"
    SELECT * FROM test_index.search(
        query => paradedb.term(field => 'value_time', value => TIME '11:43:21.456')
    );
    "#
    .fetch_collect(&mut conn);
    assert_eq!(rows, vec![(2,)]);

    // TIME WITH TIME ZONE
    let rows: Vec<(i32,)> = r#"
    SELECT * FROM test_index.search(
        query => paradedb.term(field => 'value_timetz', value => TIME WITH TIME ZONE '11:43:21.456 EST')
    );
    "#
    .fetch_collect(&mut conn);
    assert_eq!(rows, vec![(2,)]);

    // TIME WITH TIME ZONE: Change time zone in query
    let rows: Vec<(i32,)> = r#"
    SELECT * FROM test_index.search(
        query => paradedb.term(field => 'value_timetz', value => TIME WITH TIME ZONE '08:43:21.456 PST')
    );
    "#
    .fetch_collect(&mut conn);
    assert_eq!(rows, vec![(2,)]);

    // TIMESTAMP WITH TIME ZONE: Query no time zone with time zone
    let rows: Vec<(i32,)> = r#"
    SELECT * FROM test_index.search(
        query => paradedb.term(field => 'value_timestamp', value => TIMESTAMP WITH TIME ZONE '2023-04-15 13:27:09 GMT')
    );
    "#
    .fetch_collect(&mut conn);
    assert_eq!(rows, vec![(1,)]);

    // TIMESTAMP: Query time zone with no time zone (GMT = EST + 5)
    let rows: Vec<(i32,)> = r#"
    SELECT * FROM test_index.search(
        query => paradedb.term(field => 'value_timestamptz', value => TIMESTAMP '2019-08-02 12:52:43.123')
    );
    "#
    .fetch_collect(&mut conn);
    assert_eq!(rows, vec![(2,)]);
}
