//! Gateway-authenticated HTTP and local status-only stdio for the Komodo MCP.

pub mod auth;
pub mod config;
pub mod diagnostics;
mod lifecycle;
pub mod server;
pub mod stdio;

use komodo_mcp::TOOL_REGISTRY;

/// Render an annotation-native gateway manifest scaffold from the executable
/// tool registry.
///
/// The upstream is classified in `mcp_annotations` mode, so the sidecar's MCP
/// annotations are the sole source of `side_effects` and sensitivity; this
/// projection carries only the gateway-owned `risk` and never the legacy
/// per-tool `side_effects`/`pii` flags, which annotation-mode admission
/// rejects. It is a scaffold, not a publishable manifest: annotation admission
/// also requires an `approved_behavior_hash` per tool that only the gateway can
/// compute from the live server, so each must be filled from the gateway's
/// manifest-change preview before publishing.
#[must_use]
pub fn gateway_manifest() -> String {
    let mut output = String::from(
        "# Annotation-native scaffold. classification_mode: mcp_annotations makes\n\
         # the sidecar's MCP annotations the sole source of tool effects and\n\
         # sensitivity; only the gateway-owned risk is projected here. Before\n\
         # publishing, add an approved_behavior_hash (64 hex chars) to each tool\n\
         # from the gateway manifest-change preview's observed_behavior_hash.\n\
         name: komodo\ntransport: http\nurl: http://komodo-mcp:8000/mcp\nclassification_mode: mcp_annotations\nauth:\n  bearer_env: MCP_GATEWAY_UPSTREAM_BEARER_KOMODO\nsession:\n  isolation: per_call\ntools:\n",
    );
    for policy in TOOL_REGISTRY {
        output.push_str("  - name: ");
        output.push_str(policy.name);
        output.push_str("\n    risk: ");
        output.push_str(policy.risk);
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use komodo_mcp::TOOL_REGISTRY;

    use super::gateway_manifest;

    #[test]
    fn manifest_is_annotation_native_and_omits_legacy_classification_flags() {
        let manifest = gateway_manifest();
        for policy in TOOL_REGISTRY {
            assert_eq!(manifest.matches(policy.name).count(), 1, "{}", policy.name);
        }
        assert!(manifest.contains("classification_mode: mcp_annotations"));
        assert!(manifest.contains("- name: servers.status\n    risk: low\n"));
        // A mutation still carries only its gateway-owned risk; annotation mode
        // derives the effect from the sidecar's annotations at dispatch.
        assert!(manifest.contains("- name: stacks.deploy\n    risk: low\n"));
        assert!(manifest.contains("- name: stacks.stop\n    risk: low\n"));
        assert!(manifest.contains("auth:\n  bearer_env: MCP_GATEWAY_UPSTREAM_BEARER_KOMODO"));
        assert!(manifest.contains("isolation: per_call"));
        // The legacy per-tool classification flags must not be projected: in
        // annotation mode they are annotation-derived and admission rejects them.
        assert!(!manifest.contains("\n    side_effects:"));
        assert!(!manifest.contains("\n    pii:"));
    }
}
