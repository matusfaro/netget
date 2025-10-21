use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct LlmResponse {
    #[serde(default)]
    pub output: Option<String>,

    #[serde(default)]
    pub close_connection: bool,

    #[serde(default)]
    pub wait_for_more: bool,
}

fn main() {
    let schema = schema_for!(LlmResponse);
    let schema_json = serde_json::to_string_pretty(&schema).unwrap();
    println!("Full schema:\n{}", schema_json);

    // Extract just the schema part without metadata
    let simple_schema = &schema.schema;
    let simple_json = serde_json::to_string_pretty(&simple_schema).unwrap();
    println!("\nSimplified schema:\n{}", simple_json);
}