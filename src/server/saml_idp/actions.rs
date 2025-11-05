// Temporary stub - will be replaced with full implementation
pub const SAML_IDP_REQUEST_EVENT: &str = "SAML_IDP_REQUEST_EVENT";

pub struct SamlIdpProtocol;
impl SamlIdpProtocol {
    pub fn new() -> Self { Self }
}

impl crate::llm::actions::protocol_trait::Server for SamlIdpProtocol {
    fn spawn(&self, _ctx: crate::protocol::SpawnContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>> {
        Box::pin(async { Err(anyhow::anyhow!("Not yet implemented")) })
    }
    fn get_async_actions(&self, _state: &crate::state::app_state::AppState) -> Vec<crate::llm::actions::ActionDefinition> { Vec::new() }
    fn get_sync_actions(&self) -> Vec<crate::llm::actions::ActionDefinition> { Vec::new() }
    fn execute_action(&self, _action: serde_json::Value) -> anyhow::Result<crate::llm::actions::protocol_trait::ActionResult> {
        Ok(crate::llm::actions::protocol_trait::ActionResult::NoAction)
    }
    fn protocol_name(&self) -> &'static str { "SamlIdp" }
    fn get_event_types(&self) -> Vec<crate::protocol::EventType> { Vec::new() }
    fn stack_name(&self) -> &'static str { "ETH>IP>TCP>HTTP>SAML-IDP" }
    fn keywords(&self) -> Vec<&'static str> { vec!["saml-idp"] }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        crate::protocol::metadata::ProtocolMetadataV2::builder()
            .state(crate::protocol::metadata::DevelopmentState::Experimental)
            .implementation("SAML IDP - WIP")
            .build()
    }
    fn description(&self) -> &'static str { "SAML Identity Provider (WIP)" }
    fn example_prompt(&self) -> &'static str { "SAML IDP server" }
    fn group_name(&self) -> &'static str { "Authentication" }
}
