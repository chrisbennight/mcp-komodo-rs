//! Gateway-authenticated HTTP and local stdio for the semantic Komodo MCP.

pub mod auth;
pub mod config;
pub mod diagnostics;
mod files;
mod lifecycle;
pub mod server;
pub mod stdio;

use komodo_mcp::{KomodoMcp, ToolProfile};

/// Render a gateway connection scaffold containing the selected tool identities.
/// The operator must supply risk and approval policy and approve behavior hashes
/// from the gateway preview before publishing it.
#[must_use]
pub fn gateway_manifest() -> String {
    gateway_manifest_for_profile(ToolProfile::Full)
}

/// Render the scaffold for the same capability profile used by HTTP discovery.
#[must_use]
pub fn gateway_manifest_for_profile(profile: ToolProfile) -> String {
    let catalog = KomodoMcp::list_tools_for_profile(profile);
    let mut output = String::from(
        "# Incomplete connection scaffold: operator policy is required.\n\
         # Supply each tool's risk and approved_behavior_hash from the gateway\n\
         # preview, and configure authorization, disclosure, and approvals.\n\
         name: komodo\ntransport: http\nurl: http://komodo-mcp:8000/mcp\nclassification_mode: mcp_annotations\nauth:\n  bearer_env: MCP_GATEWAY_UPSTREAM_BEARER_KOMODO\nsession:\n  isolation: per_call\ntools:\n",
    );
    for tool in catalog.tools {
        output.push_str("  - name: ");
        output.push_str(&tool.name);
        output.push('\n');
    }
    output
}

/// Export the standard MCP declarations with gateway-qualified tool identities.
/// Schemas, descriptions, and factual behavior are source-owned. Authorization,
/// disclosure, risk, roles, and approval policy are operator-owned.
#[must_use]
pub fn gateway_contract_for_profile(profile: ToolProfile) -> serde_json::Value {
    let tools: Vec<_> = KomodoMcp::list_tools_for_profile(profile)
        .tools
        .into_iter()
        .map(|mut tool| {
            tool.name = format!("komodo.{}", tool.name).into();
            tool.meta = None;
            tool
        })
        .collect();
    serde_json::json!({"sourceVersion": env!("CARGO_PKG_VERSION"), "tools": tools})
}

#[cfg(test)]
mod tests {
    use komodo_mcp::TOOL_REGISTRY;

    use super::gateway_manifest;

    #[test]
    fn contract_preserves_standard_declarations_without_operator_policy() {
        use komodo_mcp::{KomodoMcp, ToolProfile};
        for profile in [
            ToolProfile::Full,
            ToolProfile::Status,
            ToolProfile::ReadOnly,
            ToolProfile::Operations,
        ] {
            let contract = super::gateway_contract_for_profile(profile);
            let tools = contract["tools"].as_array().unwrap();
            let catalog = KomodoMcp::list_tools_for_profile(profile);
            assert_eq!(tools.len(), catalog.tools.len());
            for (declaration, source) in tools.iter().zip(catalog.tools) {
                let mut expected = serde_json::to_value(source).unwrap();
                expected["name"] = format!("komodo.{}", expected["name"].as_str().unwrap()).into();
                expected.as_object_mut().unwrap().remove("_meta");
                assert_eq!(declaration, &expected);
                for field in ["governance", "risk", "pii", "requiresReview", "_meta"] {
                    assert!(declaration.get(field).is_none());
                }
            }
        }
    }

    #[test]
    fn manifest_requires_operator_policy_without_selecting_it() {
        let manifest = gateway_manifest();
        assert!(manifest.contains("operator policy is required"));
        for field in [
            "risk",
            "side_effects",
            "pii",
            "requires_approval",
            "approved_behavior_hash",
        ] {
            assert!(!manifest.contains(&format!("\n    {field}:")));
        }
    }

    #[test]
    fn manifest_preserves_connection_and_exact_tool_coverage() {
        let manifest = gateway_manifest();
        let names: Vec<_> = manifest
            .lines()
            .filter_map(|line| line.strip_prefix("  - name: "))
            .collect();
        assert_eq!(
            names,
            TOOL_REGISTRY
                .iter()
                .map(|spec| spec.name)
                .collect::<Vec<_>>()
        );
        assert!(manifest.contains("classification_mode: mcp_annotations"));
        assert!(manifest.contains("auth:\n  bearer_env: MCP_GATEWAY_UPSTREAM_BEARER_KOMODO"));
        assert!(manifest.contains("isolation: per_call"));
    }
}
