use std::process::Command;

use serde_json::Value;

#[test]
fn binary_exports_selected_profiles_without_runtime_credentials() {
    for (profile, expected_count) in [
        ("status", 14),
        ("read-only", 22),
        ("operations", 22),
        ("full", 37),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_komodo-mcp-rs"))
            .env_clear()
            .args(["--emit-tools-json", "--tool-profile", profile])
            .output()
            .unwrap();
        assert!(output.status.success(), "profile export must be accepted");
        assert!(output.stderr.is_empty());
        let catalog: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(catalog["tools"].as_array().unwrap().len(), expected_count);
        let contract = Command::new(env!("CARGO_BIN_EXE_komodo-mcp-rs"))
            .env_clear()
            .args(["--emit-gateway-contract-json", "--tool-profile", profile])
            .output()
            .unwrap();
        assert!(contract.status.success());
        assert!(contract.stderr.is_empty());
        let contract: Value = serde_json::from_slice(&contract.stdout).unwrap();
        assert_eq!(contract["tools"].as_array().unwrap().len(), expected_count);
        for tool in catalog["tools"].as_array().unwrap() {
            let name = format!("komodo.{}", tool["name"].as_str().unwrap());
            assert!(
                contract["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|entry| entry["name"] == name)
            );
        }
        let manifest = Command::new(env!("CARGO_BIN_EXE_komodo-mcp-rs"))
            .env_clear()
            .args(["--emit-gateway-manifest", "--tool-profile", profile])
            .output()
            .unwrap();
        assert!(manifest.status.success());
        let manifest = String::from_utf8(manifest.stdout).unwrap();
        let names: Vec<_> = manifest
            .lines()
            .filter_map(|line| line.strip_prefix("  - name: "))
            .collect();
        assert_eq!(names.len(), expected_count);
        for tool in catalog["tools"].as_array().unwrap() {
            assert!(names.contains(&tool["name"].as_str().unwrap()));
        }
    }
    for args in [
        vec!["--emit-tools-json", "--tool-profile", "unknown"],
        vec!["--stdio", "--tool-profile", "full"],
        vec!["--healthcheck", "--emit-gateway-manifest"],
        vec!["--emit-gateway-contract-json", "--emit-tools-json"],
        vec!["--emit-gateway-contract-json", "--stdio"],
        vec!["--emit-gateway-contract-json", "--healthcheck"],
        vec!["--emit-gateway-contract-json", "--emit-gateway-manifest"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_komodo-mcp-rs"))
            .env_clear()
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
}
