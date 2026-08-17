//! Live-LLM suite for the datastore and messaging protocols.
//!
//! These speak binary wire formats whose clients (a real MySQL driver, a real
//! AMQP client) are heavyweight to drive here, so the cases work at the event
//! layer — but each asserts what that protocol's client actually needs back:
//! a result set with columns *and* rows, an ack echoing the packet id it must
//! correlate on, a metadata response naming the requested topic.
//!
//! COVERS: mqtt: mqtt_connect, mqtt_publish, mqtt_subscribe, mqtt_unsubscribe
//! COVERS: amqp: amqp_connection_open, amqp_queue_declare, amqp_basic_consume, amqp_basic_publish
//! COVERS: kafka: kafka_metadata_request, kafka_produce_request, kafka_fetch_request, kafka_offset_commit_request
//! COVERS: zookeeper: zookeeper_request
//! COVERS: etcd: etcd_range_request, etcd_put_request, etcd_delete_request, etcd_txn_request
//! COVERS: mongodb: mongodb_command
//! COVERS: mysql: mysql_query
//! COVERS: mssql: mssql_query
//! COVERS: postgresql: postgresql_query
//! COVERS: db2: db2_connect, db2_query
//! COVERS: cassandra: cassandra_startup, cassandra_options, cassandra_query, cassandra_prepare, cassandra_execute, cassandra_auth
//! COVERS: memcached: memcached_store, memcached_delete, memcached_arithmetic, memcached_touch, memcached_stats, memcached_version, memcached_flush_all, memcached_unknown_command

use crate::helpers::llm_live::live_llm_enabled;
use crate::helpers::llm_live_case::{EventCase, ParamCheck};
use crate::helpers::E2EResult;
use serde_json::{json, Value};

/// A result set is only usable if it has both a column description and rows,
/// and every row has one cell per column. Shared by the SQL protocols, whose
/// drivers all decode that same shape.
fn rows_match_columns(action: &Value) -> Result<(), String> {
    let columns = action["columns"]
        .as_array()
        .ok_or_else(|| format!("columns must be an array, got {}", action["columns"]))?;
    if columns.is_empty() {
        return Err("a result set with no columns describes nothing".to_string());
    }
    for (i, c) in columns.iter().enumerate() {
        if c["name"].as_str().is_none() {
            return Err(format!("columns[{}] has no name: {}", i, c));
        }
        if c["type"].as_str().is_none() {
            return Err(format!(
                "columns[{}] has no type — a driver needs it to decode the cell: {}",
                i, c
            ));
        }
    }
    let rows = action["rows"]
        .as_array()
        .ok_or_else(|| format!("rows must be an array, got {}", action["rows"]))?;
    if rows.is_empty() {
        return Err("expected the instructed row, got an empty result set".to_string());
    }
    for (i, row) in rows.iter().enumerate() {
        let cells = row
            .as_array()
            .ok_or_else(|| format!("rows[{}] must be an array of cells, got {}", i, row))?;
        if cells.len() != columns.len() {
            return Err(format!(
                "rows[{}] has {} cell(s) for {} column(s); a driver reads them positionally",
                i,
                cells.len(),
                columns.len()
            ));
        }
    }
    Ok(())
}

/// The SELECT case shared by MySQL, MSSQL and PostgreSQL: same instruction,
/// same assertion, different protocol and action names.
macro_rules! sql_select_case {
    ($name:ident, $protocol:literal, $event:literal, $action:literal, $flavour:literal) => {
        #[tokio::test]
        async fn $name() -> E2EResult<()> {
            if !live_llm_enabled() {
                return Ok(());
            }
            EventCase::new(
                $protocol,
                concat!(
                    "You are a ", $flavour, " server. The table users holds exactly one row: \
                     id 1, name Alice. Answer queries against it with that data."
                ),
                $event,
                json!({ "query": "SELECT id, name FROM users" }),
            )
            .expect_action($action)
            .check_action(rows_match_columns)
            .check(ParamCheck::custom(
                "rows",
                "contains the instructed value Alice",
                |v| {
                    if v.to_string().contains("Alice") {
                        Ok(())
                    } else {
                        Err(format!("expected the instructed row for Alice, got {}", v))
                    }
                },
            ))
            .run()
            .await
        }
    };
}

sql_select_case!(
    mysql_select_returns_result_set,
    "MySQL",
    "mysql_query",
    "mysql_query_response",
    "MySQL"
);
sql_select_case!(
    mssql_select_returns_result_set,
    "MSSQL",
    "mssql_query",
    "mssql_query_response",
    "Microsoft SQL Server"
);
sql_select_case!(
    postgresql_select_returns_result_set,
    "PostgreSQL",
    "postgresql_query",
    "postgresql_query_response",
    "PostgreSQL"
);

// ---------------------------------------------------------------------------
// MQTT — every reply that owes a packet id must echo the one it answers.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mqtt_connect_is_accepted() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "MQTT",
        "You are an MQTT 3.1.1 broker that accepts every client. Acknowledge a \
         connection attempt with the success return code.",
        "mqtt_connect",
        json!({
            "client_id": "netget-live-1",
            "username": null,
            "has_password": false,
            "clean_session": true,
            "keep_alive": 60,
            "protocol_name": "MQTT",
            "protocol_level": 4,
            "will_topic": null,
            "will_message": null
        }),
    )
    .expect_action("mqtt_connack")
    .check(ParamCheck::equals("return_code", json!(0)))
    .run()
    .await
}

#[tokio::test]
async fn mqtt_subscribe_echoes_packet_id() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "MQTT",
        "You are an MQTT broker that grants every subscription at the QoS the \
         client asked for. Acknowledge the subscription.",
        "mqtt_subscribe",
        json!({
            "client_id": "netget-live-1",
            "packet_id": 4711,
            "topics": [{ "filter": "sensors/temperature", "qos": 1 }]
        }),
    )
    .expect_action("mqtt_suback")
    .check(ParamCheck::equals("packet_id", json!(4711)))
    .check(ParamCheck::custom(
        "granted_qos",
        "grants the QoS the client requested",
        |v| {
            let list = v
                .as_array()
                .ok_or_else(|| format!("granted_qos must be an array, got {}", v))?;
            match list.first().and_then(|q| q.as_u64()) {
                Some(q) if q <= 2 => Ok(()),
                _ => Err(format!("expected a granted QoS level per topic, got {}", v)),
            }
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn mqtt_publish_qos1_is_acknowledged() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "MQTT",
        "You are an MQTT broker. A QoS 1 publish must be acknowledged, or the \
         publisher retransmits it forever.",
        "mqtt_publish",
        json!({
            "client_id": "netget-live-1",
            "topic": "sensors/temperature",
            "payload": "21.5",
            "payload_is_text": true,
            "payload_size": 4,
            "qos": 1,
            "retain": false,
            "duplicate": false,
            "packet_id": 8321,
            "connected_clients": []
        }),
    )
    .expect_action("mqtt_puback")
    .or_action("mqtt_pubrec")
    .check(ParamCheck::equals("packet_id", json!(8321)))
    .run()
    .await
}

#[tokio::test]
async fn mqtt_unsubscribe_echoes_packet_id() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "MQTT",
        "You are an MQTT broker. Acknowledge an unsubscribe request so the \
         client can complete it.",
        "mqtt_unsubscribe",
        json!({
            "client_id": "netget-live-1",
            "packet_id": 9002,
            "topics": ["sensors/temperature"]
        }),
    )
    .expect_action("mqtt_unsuback")
    .check(ParamCheck::equals("packet_id", json!(9002)))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// AMQP
// ---------------------------------------------------------------------------

#[tokio::test]
async fn amqp_connection_is_opened() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "AMQP",
        "You are an AMQP 0-9-1 broker that admits clients to the / virtual \
         host. Let this connection in.",
        "amqp_connection_open",
        json!({
            "virtual_host": "/",
            "username": "guest",
            "has_password": true,
            "mechanism": "PLAIN",
            "locale": "en_US",
            "client_properties": { "product": "netget-live" },
            "peer_address": "127.0.0.1:50101",
            "frame_max": 131072,
            "heartbeat_secs": 60
        }),
    )
    .expect_action("amqp_connection_open_ok")
    .run()
    .await
}

#[tokio::test]
async fn amqp_queue_declare_echoes_queue_name() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "AMQP",
        "You are an AMQP broker. Declaring a queue always succeeds here, and \
         the queue starts empty with no consumers.",
        "amqp_queue_declare",
        json!({
            "channel": 1,
            "queue": "netget.live.orders",
            "passive": false,
            "durable": true,
            "exclusive": false,
            "auto_delete": false,
            "arguments": {}
        }),
    )
    .expect_action("amqp_queue_declare_ok")
    .check(ParamCheck::equals("queue", json!("netget.live.orders")))
    .run()
    .await
}

#[tokio::test]
async fn amqp_basic_consume_echoes_consumer_tag() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "AMQP",
        "You are an AMQP broker. Accept the consumer, keeping the consumer tag \
         it asked for — the client routes deliveries by that tag.",
        "amqp_basic_consume",
        json!({
            "channel": 1,
            "queue": "netget.live.orders",
            "consumer_tag": "ctag-live-7431",
            "no_local": false,
            "no_ack": true,
            "exclusive": false,
            "arguments": {}
        }),
    )
    .expect_action("amqp_basic_consume_ok")
    .check(ParamCheck::equals("consumer_tag", json!("ctag-live-7431")))
    .run()
    .await
}

#[tokio::test]
async fn amqp_publish_is_delivered_to_the_consumer() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "AMQP",
        "You are an AMQP broker routing messages to whoever is consuming. \
         Deliver a published message to the active consumer, keeping its body \
         and routing key.",
        "amqp_basic_publish",
        json!({
            "channel": 1,
            "exchange": "",
            "routing_key": "netget.live.orders",
            "mandatory": false,
            "immediate": false,
            "body": "{\"id\": 7}",
            "body_is_text": true,
            "body_size": 10,
            "properties": { "content_type": "application/json" },
            "active_consumers": [
                { "consumer_tag": "ctag-live-7431", "queue": "netget.live.orders" }
            ]
        }),
    )
    .expect_action("amqp_basic_deliver")
    .check(ParamCheck::equals("consumer_tag", json!("ctag-live-7431")))
    .check(ParamCheck::contains("body", "\"id\""))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// Kafka — the model-level echo is topic + partition.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn kafka_metadata_names_the_requested_topic() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "KAFKA",
        "You are a single-broker Kafka cluster at localhost:9092 with broker \
         id 0. The topic orders has one partition, 0, led by that broker.",
        "kafka_metadata_request",
        json!({
            "requested_topics": ["orders"],
            "all_topics": false,
            "client_id": "netget-live",
            "api_version": 9
        }),
    )
    .expect_action("metadata_response")
    .check(ParamCheck::custom(
        "topics",
        "describes the requested topic with at least one partition",
        |v| {
            let topics = v
                .as_array()
                .ok_or_else(|| format!("topics must be an array, got {}", v))?;
            let orders = topics
                .iter()
                .find(|t| t["name"].as_str() == Some("orders"))
                .ok_or_else(|| format!("no metadata for the requested topic 'orders': {}", v))?;
            let partitions = orders["partitions"]
                .as_array()
                .ok_or_else(|| format!("topic carries no partitions array: {}", orders))?;
            if partitions.is_empty() {
                return Err(format!(
                    "a topic with no partitions is unusable to a client: {}",
                    orders
                ));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn kafka_produce_echoes_topic_and_partition() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "KAFKA",
        "You are a Kafka broker. Accept produced records and report the offset \
         they landed at, with no error.",
        "kafka_produce_request",
        json!({
            "topic": "orders",
            "partition": 0,
            "record_count": 1,
            "first_key": "order123",
            "first_value_preview": "{\"item\": \"laptop\"}",
            "records": [{
                "offset": 0,
                "timestamp": 0,
                "key": "order123",
                "key_encoding": "utf8",
                "value": "{\"item\": \"laptop\"}",
                "value_encoding": "utf8"
            }],
            "acks": 1,
            "client_id": "netget-live"
        }),
    )
    .expect_action("produce_response")
    .check(ParamCheck::equals("topic", json!("orders")))
    .check(ParamCheck::equals("partition", json!(0)))
    .check(ParamCheck::custom(
        "error_code",
        "reports success (0)",
        |v| match v
            .as_i64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        {
            Some(0) | None => Ok(()),
            Some(other) => Err(format!(
                "expected error_code 0 for an accepted produce, got {}",
                other
            )),
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn kafka_fetch_returns_records_for_the_partition() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "KAFKA",
        "You are a Kafka broker. The topic orders, partition 0, holds one \
         record at offset 40 with key order123 and value {\"item\": \"laptop\"}. \
         Serve fetches from it.",
        "kafka_fetch_request",
        json!({
            "topic": "orders",
            "partition": 0,
            "fetch_offset": 40,
            "max_bytes": 1048576,
            "client_id": "netget-live"
        }),
    )
    .expect_action("fetch_response")
    .check(ParamCheck::equals("topic", json!("orders")))
    .check(ParamCheck::equals("partition", json!(0)))
    .check(ParamCheck::custom(
        "records",
        "carries the record at the requested offset",
        |v| {
            let records = v
                .as_array()
                .ok_or_else(|| format!("records must be an array, got {}", v))?;
            if records.is_empty() {
                return Err("expected the instructed record, got an empty fetch".to_string());
            }
            if !v.to_string().contains("order123") {
                return Err(format!("expected the instructed record key, got {}", v));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn kafka_offset_commit_is_acknowledged() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "KAFKA",
        "You are a Kafka broker that accepts every offset commit from a \
         consumer group.",
        "kafka_offset_commit_request",
        json!({
            "group_id": "netget-live-group",
            "topic": "orders",
            "partition": 0,
            "offset": 41,
            "client_id": "netget-live"
        }),
    )
    .expect_action("offset_commit_response")
    .check(ParamCheck::equals("topic", json!("orders")))
    .check(ParamCheck::equals("partition", json!(0)))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// ZooKeeper — xid is the correlator a client desyncs without.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn zookeeper_get_data_echoes_xid() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "ZooKeeper",
        "You are a ZooKeeper ensemble. The znode /config/db holds the string \
         postgres://localhost:5432. Serve reads of it.",
        "zookeeper_request",
        json!({
            "xid": 6021,
            "operation": "getData",
            "op_code": 4,
            "path": "/config/db"
        }),
    )
    .expect_action("zookeeper_data")
    .check(ParamCheck::equals("xid", json!(6021)))
    .check(ParamCheck::contains("data", "postgres://localhost:5432"))
    .check(ParamCheck::custom(
        "zxid",
        "carries a transaction id",
        |v| match v
            .as_i64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        {
            Some(_) => Ok(()),
            None => Err(format!("zxid must be a number, got {}", v)),
        },
    ))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// etcd
// ---------------------------------------------------------------------------

#[tokio::test]
async fn etcd_range_returns_the_key() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "etcd",
        "You are an etcd key-value store holding exactly one key, foo, whose \
         value is bar. Serve range (get) requests from it.",
        "etcd_range_request",
        json!({ "key": "foo", "range_end": null, "limit": 0 }),
    )
    .expect_action("etcd_range_response")
    .check(ParamCheck::custom(
        "kvs",
        "returns foo=bar as a key-value entry",
        |v| {
            let kvs = v
                .as_array()
                .ok_or_else(|| format!("kvs must be an array, got {}", v))?;
            let hit = kvs.iter().find(|kv| kv["key"].as_str() == Some("foo"));
            match hit {
                Some(kv) if kv["value"].as_str() == Some("bar") => Ok(()),
                Some(kv) => Err(format!("foo must carry the value bar, got {}", kv)),
                None => Err(format!("no entry for the requested key foo: {}", v)),
            }
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn etcd_put_reports_a_revision() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "etcd",
        "You are an etcd key-value store. Accept writes and report the store's \
         new revision, which increases with every write.",
        "etcd_put_request",
        json!({ "key": "foo", "value": "bar", "lease": 0 }),
    )
    .expect_action("etcd_put_response")
    .check(ParamCheck::custom(
        "revision",
        "is a positive revision number",
        |v| match v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        {
            Some(r) if r > 0 => Ok(()),
            Some(r) => Err(format!("revision {} does not identify a write", r)),
            None => Err(format!("revision must be a number, got {}", v)),
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn etcd_delete_reports_the_count() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "etcd",
        "You are an etcd key-value store holding exactly one key, foo. Deleting \
         it removes exactly one key.",
        "etcd_delete_request",
        json!({ "key": "foo", "range_end": null }),
    )
    .expect_action("etcd_delete_range_response")
    .check(ParamCheck::equals("deleted", json!(1)))
    .run()
    .await
}

#[tokio::test]
async fn etcd_txn_reports_whether_it_succeeded() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "etcd",
        "You are an etcd key-value store. The key foo exists at mod_revision 1, \
         so a transaction comparing it against 1 has its comparison hold.",
        "etcd_txn_request",
        json!({
            "compare_count": 1,
            "success_count": 1,
            "failure_count": 0,
            "compares": [{ "key": "foo", "target": "MOD", "mod_revision": 1 }]
        }),
    )
    .expect_action("etcd_txn_response")
    .check(ParamCheck::equals("succeeded", json!(true)))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// MongoDB
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mongodb_find_returns_documents() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "MongoDB",
        "You are a MongoDB server. The users collection in the netget database \
         holds one document: name Alice, age 30. Serve finds against it.",
        "mongodb_command",
        json!({
            "command": "find",
            "database": "netget",
            "collection": "users",
            "filter": {},
            "document": null
        }),
    )
    .expect_action("find_response")
    .check(ParamCheck::custom(
        "documents",
        "returns the instructed document",
        |v| {
            let docs = v
                .as_array()
                .ok_or_else(|| format!("documents must be an array, got {}", v))?;
            if docs.is_empty() {
                return Err("expected the instructed document, got an empty batch".to_string());
            }
            if !v.to_string().contains("Alice") {
                return Err(format!("expected the document for Alice, got {}", v));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// Db2 — no result-set path exists, so a query answers with an SQLCA.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn db2_connect_is_accepted() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Db2",
        "You are a Db2 server. The user db2inst1 is authorised for the SAMPLE \
         database; admit them.",
        "db2_connect",
        json!({
            "user_id": "db2inst1",
            "rdb_name": "SAMPLE",
            "has_password": true
        }),
    )
    .expect_action("db2_accept_connection")
    .run()
    .await
}

#[tokio::test]
async fn db2_query_reports_success_sqlcode() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Db2",
        "You are a Db2 server. Statements against the SAMPLE database succeed; \
         report success with SQLCODE 0.",
        "db2_query",
        json!({
            "sql_text": "INSERT INTO users (id, name) VALUES (1, 'Alice')",
            "statement_type": "execute_immediate"
        }),
    )
    .expect_action("db2_query_ok")
    .check(ParamCheck::custom(
        "sqlcode",
        "is 0, meaning success",
        |v| match v
            .as_i64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        {
            Some(0) | None => Ok(()),
            Some(other) => Err(format!("SQLCODE {} is not success (0)", other)),
        },
    ))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// Cassandra
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cassandra_startup_is_answered_ready() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Cassandra",
        "You are a Cassandra node that requires no authentication. A client's \
         STARTUP is answered by telling it the connection is ready to use.",
        "cassandra_startup",
        json!({
            "protocol_version": 4,
            "options": { "CQL_VERSION": "3.0.0" }
        }),
    )
    .expect_action("cassandra_ready")
    .run()
    .await
}

#[tokio::test]
async fn cassandra_options_lists_supported_versions() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Cassandra",
        "You are a Cassandra node speaking CQL 3.0.0 with no compression. \
         Answer an OPTIONS request with what you support.",
        "cassandra_options",
        json!({}),
    )
    .expect_action("cassandra_supported")
    .check(ParamCheck::custom(
        "options",
        "advertises a CQL version",
        |v| {
            if v.to_string().to_uppercase().contains("CQL_VERSION") {
                Ok(())
            } else {
                Err(format!(
                    "expected CQL_VERSION among the supported options, got {}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn cassandra_query_returns_rows() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Cassandra",
        "You are a Cassandra node. The table users holds exactly one row: id 1, \
         name Alice. Answer CQL queries against it with that data.",
        "cassandra_query",
        json!({
            "query": "SELECT id, name FROM users",
            "consistency": "ONE"
        }),
    )
    .expect_action("cassandra_result_rows")
    .check_action(rows_match_columns)
    .run()
    .await
}

#[tokio::test]
async fn cassandra_prepare_describes_the_statement() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Cassandra",
        "You are a Cassandra node. The table users has columns id (int) and \
         name (varchar). Preparing a statement means describing the columns it \
         returns and the parameters it takes.",
        "cassandra_prepare",
        json!({
            "query": "SELECT id, name FROM users WHERE id = ?",
            "statement_id": "a1b2c3d4",
            "param_count": 1
        }),
    )
    .expect_action("cassandra_prepared")
    .check(ParamCheck::custom(
        "columns",
        "describes the result columns with names and types",
        |v| {
            let cols = v
                .as_array()
                .ok_or_else(|| format!("columns must be an array, got {}", v))?;
            if cols.is_empty() {
                return Err("a prepared statement that describes no columns is unusable".into());
            }
            for c in cols {
                if c["name"].as_str().is_none() || c["type"].as_str().is_none() {
                    return Err(format!("each column needs a name and a type: {}", c));
                }
            }
            Ok(())
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn cassandra_execute_returns_rows() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Cassandra",
        "You are a Cassandra node. The table users holds exactly one row: id 1, \
         name Alice. Executing a prepared SELECT bound to id 1 returns it.",
        "cassandra_execute",
        json!({
            "query": "SELECT id, name FROM users WHERE id = ?",
            "statement_id": "a1b2c3d4",
            "parameters": [1]
        }),
    )
    .expect_action("cassandra_result_rows")
    .check_action(rows_match_columns)
    .run()
    .await
}

#[tokio::test]
async fn cassandra_auth_accepts_valid_credentials() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Cassandra",
        "You are a Cassandra node whose only valid login is user cassandra \
         with password cassandra. Admit that login.",
        "cassandra_auth",
        json!({ "username": "cassandra", "password": "cassandra" }),
    )
    .expect_action("cassandra_auth_success")
    .run()
    .await
}

// ---------------------------------------------------------------------------
// Memcached — the text protocol's status words are exact tokens.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn memcached_store_is_acknowledged() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Memcached",
        "You are a memcached server that accepts every store.",
        "memcached_store",
        json!({
            "command": "set",
            "key": "greeting",
            "flags": 0,
            "exptime": 0,
            "bytes": 5,
            "cas_unique": 0,
            "value": "hello",
            "value_encoding": "utf8"
        }),
    )
    .expect_action("send_memcached_status")
    .check(ParamCheck::equals("status", json!("STORED")))
    .run()
    .await
}

#[tokio::test]
async fn memcached_delete_reports_deleted() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Memcached",
        "You are a memcached server holding the key greeting. Deleting a key \
         that exists succeeds.",
        "memcached_delete",
        json!({ "key": "greeting" }),
    )
    .expect_action("send_memcached_status")
    .check(ParamCheck::equals("status", json!("DELETED")))
    .run()
    .await
}

#[tokio::test]
async fn memcached_incr_returns_the_new_value() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Memcached",
        "You are a memcached server. The key counter currently holds 42. \
         Incrementing it by 1 makes it 43; answer with the new value.",
        "memcached_arithmetic",
        json!({ "command": "incr", "key": "counter", "delta": 1 }),
    )
    .expect_action("send_memcached_number")
    .check(ParamCheck::equals("value", json!(43)))
    .run()
    .await
}

#[tokio::test]
async fn memcached_touch_reports_touched() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Memcached",
        "You are a memcached server holding the key greeting. Updating the \
         expiry of a key that exists succeeds.",
        "memcached_touch",
        json!({ "key": "greeting", "exptime": 300 }),
    )
    .expect_action("send_memcached_status")
    .check(ParamCheck::equals("status", json!("TOUCHED")))
    .run()
    .await
}

#[tokio::test]
async fn memcached_stats_are_self_consistent() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Memcached",
        "You are a memcached server that has been up for an hour and holds no \
         items. Report your statistics.",
        "memcached_stats",
        json!({ "argument": "" }),
    )
    .expect_action("send_memcached_stats")
    .check(ParamCheck::custom(
        "stats",
        "reports the standard counters a stats reader expects",
        |v| {
            let obj = v
                .as_object()
                .ok_or_else(|| format!("stats must be an object of name/value pairs, got {}", v))?;
            for key in ["pid", "uptime"] {
                if !obj.contains_key(key) {
                    return Err(format!(
                        "stats omit '{}', which every memcached reports: {}",
                        key, v
                    ));
                }
            }
            Ok(())
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn memcached_version_is_reported() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Memcached",
        "You are a memcached server claiming to be version 1.6.45.",
        "memcached_version",
        json!({}),
    )
    .expect_action("send_memcached_version")
    .check(ParamCheck::contains("version", "1.6.45"))
    .run()
    .await
}

#[tokio::test]
async fn memcached_flush_all_is_acknowledged() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Memcached",
        "You are a memcached server that allows clients to invalidate the \
         whole cache.",
        "memcached_flush_all",
        json!({ "delay": 0 }),
    )
    .expect_action("send_memcached_status")
    .check(ParamCheck::equals("status", json!("OK")))
    .run()
    .await
}

#[tokio::test]
async fn memcached_unknown_command_is_refused() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Memcached",
        "You are a memcached server that implements only the standard text \
         commands. A verb you do not recognise gets the protocol's error \
         response, exactly as real memcached answers it.",
        "memcached_unknown_command",
        json!({ "line": "frobnicate greeting" }),
    )
    .expect_action("send_memcached_error")
    .check(ParamCheck::custom(
        "kind",
        "is one of the protocol's error words",
        |v| {
            let s = v.as_str().unwrap_or("").to_uppercase();
            if s == "ERROR" || s == "CLIENT_ERROR" || s == "SERVER_ERROR" {
                Ok(())
            } else {
                Err(format!(
                    "expected ERROR, CLIENT_ERROR or SERVER_ERROR (the only words \
                     the text protocol defines), got {:?}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

/// The socket is already closed by the time this fires — the event is
/// `with_no_actions()`, so a MongoDB wire-protocol reply is not merely
/// pointless but unavailable. Recording why the client went away is the
/// answer, and `reason` distinguishes a clean client disconnect from a
/// protocol error the server itself raised.
#[tokio::test]
async fn mongodb_disconnect_is_recorded_with_its_reason() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "mongodb",
        "You are a MongoDB server. Keep a record of how each client session \
         ended, so a malformed-message disconnect can be told apart from a \
         client that simply closed.",
        "mongodb_disconnected",
        json!({
            "reason": "malformed_op_msg",
            "client_ip": "203.0.113.140:49700",
            "connection_id": "conn-3"
        }),
    )
    .expect_action("append_to_log")
    .or_action("append_memory")
    .or_action("show_message")
    .check_action(|a| {
        let flat = a.to_string().to_lowercase();
        if flat.contains("malformed") {
            Ok(())
        } else {
            Err(format!(
                "the record must keep the reason the connection ended \
                 (malformed_op_msg) — that is the only thing distinguishing a protocol \
                 error from a normal close. Got {}",
                a
            ))
        }
    })
    .run()
    .await
}
