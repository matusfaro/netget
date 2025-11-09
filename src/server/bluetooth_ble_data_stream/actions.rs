//! BLE Data Stream protocol actions
use crate::llm::actions::{protocol_trait::{ActionResult, Protocol, Server}, ActionDefinition, Parameter, ParameterDefinition};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

pub static STREAM_STARTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("stream_started", "Data stream started").with_parameters(vec![
        Parameter::new("stream_id", "Stream identifier"),
        Parameter::new("sample_rate", "Samples per second"),
    ])
});

pub static STREAM_DATA_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("stream_data", "Stream data packet received").with_parameters(vec![
        Parameter::new("stream_id", "Stream identifier"),
        Parameter::new("sequence", "Packet sequence number"),
    ])
});

pub struct BluetoothBleDataStreamProtocol;
impl BluetoothBleDataStreamProtocol { pub fn new() -> Self { Self } }

impl Protocol for BluetoothBleDataStreamProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![ParameterDefinition {
            name: "device_name".to_string(),
            type_hint: "string".to_string(),
            description: "Device name (default: NetGet-Stream)".to_string(),
            required: false,
            example: json!("MySensor"),
        }]
    }

    fn get_async_actions(&self, _: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "start_stream".to_string(),
                description: "Start data streaming".to_string(),
                parameters: vec![
                    Parameter { name: "stream_id".to_string(), type_hint: "string".to_string(), description: "Stream identifier".to_string(), required: true},
                    Parameter { name: "sample_rate".to_string(), type_hint: "number".to_string(), description: "Samples per second (1-100)".to_string(), required: true},
                    Parameter { name: "data_type".to_string(), type_hint: "string".to_string(), description: "Data type (sensor, gps, imu, audio)".to_string(), required: true},
                ],
            example: json!({
            "type": "start_stream",
            "stream_id": "example_stream_id",
            "sample_rate": 42,
            "data_type": "example_data_type"
        }),
    },
            ActionDefinition {
                name: "send_stream_data".to_string(),
                description: "Send stream data packet".to_string(),
                parameters: vec![
                    Parameter { name: "stream_id".to_string(), type_hint: "string".to_string(), description: "Stream identifier".to_string(), required: true},
                    Parameter { name: "data".to_string(), type_hint: "object".to_string(), description: "Data payload (JSON)".to_string(), required: true},
                ],
                example: json!({
                    "type": "send_stream_data",
                    "stream_id": "imu_sensor",
                    "data": {"x": 1.2, "y": 2.3, "z": -0.8}
                }),
            },
            ActionDefinition {
                name: "stop_stream".to_string(),
                description: "Stop data streaming".to_string(),
                parameters: vec![
                    Parameter { name: "stream_id".to_string(), type_hint: "string".to_string(), description: "Stream identifier".to_string(), required: true},
                ],
            example: json!({
            "type": "stop_stream",
            "stream_id": "example_stream_id"
        }),
    },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> { vec![] }
    fn protocol_name(&self) -> &'static str { "BLUETOOTH_BLE_DATA_STREAM" }
    fn get_event_types(&self) -> Vec<EventType> { vec![STREAM_STARTED_EVENT.clone(), STREAM_DATA_EVENT.clone()] }
    fn stack_name(&self) -> &'static str { "DATALINK" }
    fn keywords(&self) -> Vec<&'static str> { vec!["bluetooth", "ble", "stream", "data", "sensor"] }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};
        ProtocolMetadataV2::builder().state(DevelopmentState::Experimental)
            .implementation("BLE real-time data streaming (custom GATT)")
            .llm_control("Actions: start_stream, send_stream_data, stop_stream")
            .e2e_testing("Requires BLE device")
            .notes("Custom GATT service for real-time sensor data streaming")
            .build()
    }

    fn description(&self) -> &'static str { "BLE data streaming - real-time sensor/telemetry data" }
    fn example_prompt(&self) -> &'static str { "Stream IMU data at 10 Hz with random accelerometer values" }
    fn group_name(&self) -> &'static str { "Network" }
}

impl Server for BluetoothBleDataStreamProtocol {
    fn spawn(&self, ctx: crate::protocol::SpawnContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>> {
        Box::pin(async move {
            let device_name = ctx.startup_params.as_ref().and_then(|p| p.get_optional_string("device_name")).and_then(|v| v.as_str()).unwrap_or("NetGet-Stream").to_string();
            crate::server::bluetooth_ble_data_stream::BluetoothBleDataStream::spawn_with_llm_actions(
                device_name, ctx.llm_client, ctx.state, ctx.status_tx, ctx.server_id, ctx.instruction
            ).await
        })
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action["type"].as_str().context("Action must have 'type' field")?;
        match action_type {
            "start_stream" | "send_stream_data" | "stop_stream" => Ok(ActionResult::Custom { name: action_type.to_string(), data: action }),
            _ => Err(anyhow::anyhow!("Unknown data stream action: {}", action_type)),
        }
    }
}
