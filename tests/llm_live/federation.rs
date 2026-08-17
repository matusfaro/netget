//! Live-LLM SAML federation suite (event-level): SAML Identity Provider and
//! SAML Service Provider.
//!
//! Both sit behind an HTTP surface, but the model's job is document
//! construction and state echo, which is transport-independent.
//!
//! Protocol facts these cases encode:
//! - **RelayState is opaque and must come back unchanged.** It is the only
//!   thing carrying the user's original destination across the round trip
//!   (SAML 2.0 bindings, §3.1.1); an IdP that rewrites or drops it lands the
//!   user on the wrong page after login, and no error is reported anywhere.
//! - **The assertion posts to the SP's AssertionConsumerService URL**, not to
//!   wherever the request came from. The `acs_url` parameter is what the
//!   generated auto-submit form targets.
//! - **`/metadata` is metadata, not an assertion.** A SAML metadata document
//!   is an `EntityDescriptor` carrying an `entityID`; answering with an
//!   assertion is a different document type entirely and no SP will consume
//!   it.
//! - **The SP's ACS endpoint consumes an assertion** and turns it into a
//!   session — `user_id` becomes the session cookie value, so the subject of
//!   the assertion is what must land there, not the IdP's name or the SP's.

use crate::helpers::llm_live::live_llm_enabled;
use crate::helpers::llm_live_case::{EventCase, ParamCheck};
use crate::helpers::E2EResult;
use serde_json::json;

// ---------------------------------------------------------------------------
// Identity provider
// ---------------------------------------------------------------------------

/// The SSO endpoint: an assertion posted to the SP's ACS, with the SP's
/// RelayState handed straight back.
#[tokio::test]
async fn saml_idp_sso_echoes_relay_state_and_targets_the_acs() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SamlIdp",
        "You are a SAML identity provider with entity ID \
         https://idp.netget.test/metadata. The only service provider you \
         serve is https://sp.netget.test, whose AssertionConsumerService URL \
         is https://sp.netget.test/acs. Authenticate every request as the \
         user jane.doe@netget.test.",
        "saml_idp_request",
        json!({
            "method": "GET",
            "path": "/sso",
            "query": "SAMLRequest=fVJNb9swDP0rhu6O5DhpEiEJkDUYFqDbgibbYZdCkelGgCx5otwt%2B%2FWT7RTtDusL&RelayState=https%3A%2F%2Fsp.netget.test%2Fdashboard%3Ftab%3Dreports",
            "headers": {
                "host": "idp.netget.test",
                "user-agent": "Mozilla/5.0"
            },
            "body": null,
            "client_ip": "203.0.113.100"
        }),
    )
    .expect_action("send_saml_response")
    .check(ParamCheck::custom(
        "acs_url",
        "the SP's AssertionConsumerService URL",
        |v| {
            let s = v.as_str().unwrap_or("");
            if s.trim_end_matches('/') == "https://sp.netget.test/acs" {
                Ok(())
            } else {
                Err(format!(
                    "the form must post to the SP's ACS (https://sp.netget.test/acs), \
                     got {:?}",
                    v
                ))
            }
        },
    ))
    .check(ParamCheck::custom(
        "relay_state",
        "the SP's RelayState, returned unchanged",
        |v| {
            let s = v.as_str().unwrap_or("");
            // Accept the URL-decoded form too: the value is opaque, but it is
            // the *same* value either way.
            let decoded = s
                .replace("%3A", ":")
                .replace("%2F", "/")
                .replace("%3F", "?")
                .replace("%3D", "=");
            if decoded == "https://sp.netget.test/dashboard?tab=reports" {
                Ok(())
            } else {
                Err(format!(
                    "RelayState is opaque and must come back unchanged — it is the only \
                     record of where the user was going. Expected \
                     https://sp.netget.test/dashboard?tab=reports, got {:?}",
                    v
                ))
            }
        },
    ))
    .check(ParamCheck::custom(
        "assertion_xml",
        "a SAML Assertion naming the authenticated subject",
        |v| {
            let s = v.as_str().unwrap_or("");
            if !s.contains("Assertion") {
                return Err(format!(
                    "assertion_xml must be a SAML <Assertion> document, got {:?}",
                    s.chars().take(120).collect::<String>()
                ));
            }
            if !s.contains("jane.doe@netget.test") {
                return Err(format!(
                    "the assertion must name the authenticated user \
                     (jane.doe@netget.test); got {:?}",
                    s.chars().take(200).collect::<String>()
                ));
            }
            if !s.contains("Subject") && !s.contains("NameID") {
                return Err(
                    "a SAML assertion identifies its subject with a <Subject>/<NameID>"
                        .to_string(),
                );
            }
            Ok(())
        },
    ))
    .run()
    .await
}

/// `/metadata` is a different document: an EntityDescriptor, carrying this
/// IdP's entityID. Answering with an assertion is a category error.
#[tokio::test]
async fn saml_idp_metadata_is_an_entity_descriptor() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SamlIdp",
        "You are a SAML identity provider with entity ID \
         https://idp.netget.test/metadata, whose single sign-on endpoint is \
         https://idp.netget.test/sso. Publish your federation metadata when \
         asked for it.",
        "saml_idp_request",
        json!({
            "method": "GET",
            "path": "/metadata",
            "query": "",
            "headers": { "host": "idp.netget.test" },
            "body": null,
            "client_ip": "203.0.113.100"
        }),
    )
    .expect_action("send_metadata")
    .check(ParamCheck::custom(
        "metadata_xml",
        "an EntityDescriptor carrying this IdP's entityID",
        |v| {
            let s = v.as_str().unwrap_or("");
            if !s.contains("EntityDescriptor") {
                return Err(format!(
                    "SAML metadata is an <EntityDescriptor>; got {:?}",
                    s.chars().take(120).collect::<String>()
                ));
            }
            if !s.contains("entityID") {
                return Err("an EntityDescriptor must carry its entityID".to_string());
            }
            if !s.contains("https://idp.netget.test") {
                return Err(format!(
                    "the entityID must be this IdP's \
                     (https://idp.netget.test/metadata); got {:?}",
                    s.chars().take(200).collect::<String>()
                ));
            }
            if !s.contains("IDPSSODescriptor") {
                return Err(
                    "an identity provider publishes an <IDPSSODescriptor>; an \
                     SPSSODescriptor would describe the other side of the federation"
                        .to_string(),
                );
            }
            Ok(())
        },
    ))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// Service provider
// ---------------------------------------------------------------------------

/// The ACS endpoint consumes the assertion and starts a session. `user_id`
/// becomes the session cookie value, so it must be the assertion's subject.
#[tokio::test]
async fn saml_sp_acs_creates_a_session_for_the_asserted_subject() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    // A readable (unencoded) SAMLResponse: the point under test is which
    // identity the model lifts out of it, not base64/deflate decoding.
    let assertion = "<samlp:Response xmlns:samlp=\"urn:oasis:names:tc:SAML:2.0:protocol\" \
         xmlns:saml=\"urn:oasis:names:tc:SAML:2.0:assertion\">\
         <saml:Issuer>https://idp.netget.test/metadata</saml:Issuer>\
         <saml:Assertion><saml:Subject>\
         <saml:NameID Format=\"urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress\">\
         jane.doe@netget.test</saml:NameID></saml:Subject>\
         <saml:AttributeStatement>\
         <saml:Attribute Name=\"email\"><saml:AttributeValue>jane.doe@netget.test\
         </saml:AttributeValue></saml:Attribute>\
         <saml:Attribute Name=\"role\"><saml:AttributeValue>auditor\
         </saml:AttributeValue></saml:Attribute>\
         </saml:AttributeStatement></saml:Assertion></samlp:Response>";

    EventCase::new(
        "SamlSp",
        "You are a SAML service provider at https://sp.netget.test. Trust \
         assertions issued by https://idp.netget.test/metadata and log the \
         asserted user in.",
        "saml_sp_request",
        json!({
            "method": "POST",
            "path": "/acs",
            "query": "",
            "headers": { "content-type": "application/x-www-form-urlencoded" },
            "body": format!("SAMLResponse={}&RelayState=https://sp.netget.test/dashboard", assertion),
            "client_ip": "203.0.113.100"
        }),
    )
    .expect_action("process_assertion")
    .check(ParamCheck::custom(
        "user_id",
        "the assertion's subject (it becomes the session cookie value)",
        |v| {
            let s = v.as_str().unwrap_or("");
            if s.contains("jane.doe") {
                Ok(())
            } else {
                Err(format!(
                    "the session belongs to the asserted subject \
                     (jane.doe@netget.test), not the issuer or the SP; got {:?}",
                    v
                ))
            }
        },
    ))
    .check(ParamCheck::custom(
        "attributes",
        "the attributes the assertion carried",
        |v| {
            let flat = v.to_string().to_lowercase();
            if flat.contains("auditor") {
                Ok(())
            } else {
                Err(format!(
                    "the assertion carried role=auditor; the session should keep it, \
                     got {}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

/// An unauthenticated hit on a protected page starts the flow: an
/// AuthnRequest aimed at the IdP's SSO endpoint, carrying where the user was
/// trying to go as RelayState.
#[tokio::test]
async fn saml_sp_login_starts_the_flow_at_the_idp() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SamlSp",
        "You are a SAML service provider with entity ID \
         https://sp.netget.test/metadata. Your identity provider's single \
         sign-on endpoint is https://idp.netget.test/sso. When an \
         unauthenticated user asks to log in, send them there, remembering \
         where they were headed.",
        "saml_sp_request",
        json!({
            "method": "GET",
            "path": "/login",
            "query": "next=https%3A%2F%2Fsp.netget.test%2Fdashboard%3Ftab%3Dreports",
            "headers": { "host": "sp.netget.test" },
            "body": null,
            "client_ip": "203.0.113.100"
        }),
    )
    .expect_action("send_authn_request")
    .check(ParamCheck::custom(
        "idp_sso_url",
        "the IdP's SSO endpoint",
        |v| {
            let s = v.as_str().unwrap_or("").trim_end_matches('/');
            if s == "https://idp.netget.test/sso" {
                Ok(())
            } else {
                Err(format!(
                    "the AuthnRequest goes to the IdP's SSO endpoint \
                     (https://idp.netget.test/sso), not to the SP; got {:?}",
                    v
                ))
            }
        },
    ))
    .check(ParamCheck::custom(
        "request_xml",
        "a samlp:AuthnRequest issued by this SP",
        |v| {
            let s = v.as_str().unwrap_or("");
            if !s.contains("AuthnRequest") {
                return Err(format!(
                    "request_xml must be a <samlp:AuthnRequest>; got {:?}",
                    s.chars().take(120).collect::<String>()
                ));
            }
            if !s.contains("Issuer") {
                return Err("an AuthnRequest carries the SP's <Issuer>".to_string());
            }
            if !s.contains("sp.netget.test") {
                return Err(format!(
                    "the Issuer must be this SP (https://sp.netget.test/metadata); got \
                     {:?}",
                    s.chars().take(200).collect::<String>()
                ));
            }
            Ok(())
        },
    ))
    .check(ParamCheck::custom(
        "relay_state",
        "where the user was trying to go",
        |v| {
            let s = v
                .as_str()
                .unwrap_or("")
                .replace("%3A", ":")
                .replace("%2F", "/")
                .replace("%3F", "?")
                .replace("%3D", "=");
            if s.contains("/dashboard") {
                Ok(())
            } else {
                Err(format!(
                    "RelayState carries the original destination \
                     (https://sp.netget.test/dashboard?tab=reports) across the round \
                     trip; got {:?}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}
