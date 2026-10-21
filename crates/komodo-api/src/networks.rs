//! Typed Docker network facts reported by Komodo Core.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Network inventory, including addressing and attachment facts when reported.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct NetworkListItem {
    pub name: Option<String>,
    pub id: Option<String>,
    pub created: Option<String>,
    pub scope: Option<String>,
    pub driver: Option<String>,
    pub enable_ipv6: Option<bool>,
    pub ipam_driver: Option<String>,
    pub ipam_subnet: Option<String>,
    pub ipam_gateway: Option<String>,
    pub internal: Option<bool>,
    pub attachable: Option<bool>,
    pub ingress: Option<bool>,
    pub in_use: Option<bool>,
}

/// One network's supported settings and attached endpoints, without container inspect data.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct Network {
    #[serde(alias = "Name")]
    pub name: Option<String>,
    #[serde(alias = "Id")]
    pub id: Option<String>,
    #[serde(alias = "Created")]
    pub created: Option<String>,
    #[serde(alias = "Scope")]
    pub scope: Option<String>,
    #[serde(alias = "Driver")]
    pub driver: Option<String>,
    #[serde(alias = "EnableIPv6")]
    pub enable_ipv6: Option<bool>,
    #[serde(alias = "IPAM")]
    pub ipam: Option<Ipam>,
    #[serde(alias = "Internal")]
    pub internal: Option<bool>,
    #[serde(alias = "Attachable")]
    pub attachable: Option<bool>,
    #[serde(alias = "Ingress")]
    pub ingress: Option<bool>,
    #[serde(alias = "Containers")]
    pub containers: Option<Vec<NetworkContainer>>,
    #[serde(alias = "Options")]
    pub options: Option<BTreeMap<String, String>>,
    #[serde(alias = "Labels")]
    pub labels: Option<BTreeMap<String, String>>,
}

/// IP address management driver and configuration.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct Ipam {
    #[serde(alias = "Driver")]
    pub driver: Option<String>,
    #[serde(alias = "Config")]
    pub config: Option<Vec<IpamConfig>>,
    #[serde(alias = "Options")]
    pub options: Option<BTreeMap<String, String>>,
}

/// Subnet, gateway, address range, and auxiliary addresses reported by Docker.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct IpamConfig {
    #[serde(alias = "Subnet")]
    pub subnet: Option<String>,
    #[serde(alias = "IPRange")]
    pub ip_range: Option<String>,
    #[serde(alias = "Gateway")]
    pub gateway: Option<String>,
    #[serde(alias = "AuxiliaryAddresses")]
    pub auxiliary_addresses: Option<BTreeMap<String, String>>,
}

/// An attached container endpoint's identifiers and network addresses.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct NetworkContainer {
    #[serde(alias = "ContainerID")]
    pub container_id: Option<String>,
    #[serde(alias = "Name")]
    pub name: Option<String>,
    #[serde(alias = "EndpointID")]
    pub endpoint_id: Option<String>,
    #[serde(alias = "MacAddress")]
    pub mac_address: Option<String>,
    #[serde(alias = "IPv4Address")]
    pub ipv4_address: Option<String>,
    #[serde(alias = "IPv6Address")]
    pub ipv6_address: Option<String>,
}

pub(crate) fn uses_current_names(version: &str) -> Option<bool> {
    let mut components = version.strip_prefix('v').unwrap_or(version).split('.');
    let major = components.next()?.parse::<u64>().ok()?;
    let minor = components.next()?.parse::<u64>().ok()?;
    Some(major > 2 || (major == 2 && minor >= 3))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Client, Credentials, KomodoApi};
    use serde_json::json;
    use std::time::{Duration, Instant};
    use url::Url;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, header, method, path},
    };

    fn read_client(upstream: &MockServer) -> Client {
        Client::new(
            Url::parse(&upstream.uri()).unwrap(),
            Credentials::new("synthetic-read-key".into(), "synthetic-read-secret".into()).unwrap(),
            Duration::from_secs(2),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn wire_reads_preserve_addresses_and_use_the_versioned_named_requests() {
        for (version, list, inspect) in [
            ("2.1.2", "ListDockerNetworks", "InspectDockerNetwork"),
            ("2.3.3", "ListNetworks", "InspectNetwork"),
        ] {
            let upstream = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/read"))
                .and(body_json(json!({"type":"GetVersion","params":{}})))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"version":version})))
                .expect(1)
                .mount(&upstream)
                .await;
            Mock::given(method("POST"))
                .and(path("/read"))
                .and(header("x-api-key", "synthetic-read-key"))
                .and(body_json(
                    json!({"type":list,"params":{"server":"server-1"}}),
                ))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
                    "name":"application","id":"network-1","ipam_subnet":"10.20.0.0/24",
                    "ipam_gateway":"10.20.0.1","in_use":true
                }])))
                .expect(1)
                .mount(&upstream)
                .await;
            Mock::given(method("POST")).and(path("/read"))
                .and(header("x-api-key", "synthetic-read-key"))
                .and(body_json(json!({"type":inspect,"params":{"server":"server-1","network":"application"}})))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "Name":"application","Id":"network-1","Driver":"bridge","EnableIPv6":true,
                    "IPAM":{"Driver":"default","Config":[{
                        "Subnet":"10.20.0.0/24","IPRange":"10.20.0.128/25","Gateway":"10.20.0.1",
                        "AuxiliaryAddresses":{"router":"10.20.0.2"}
                    }],"Options":{"allocation":"automatic"}},
                    "Containers":[{"ContainerID":"container-1","Name":"application-1","EndpointID":"endpoint-1",
                        "MacAddress":"02:42:0a:14:00:03","IPv4Address":"10.20.0.3/24","IPv6Address":"fd00::3/64"}],
                    "Options":{"com.docker.network.bridge.name":"br-app"},"Labels":{"owner":"$(untrusted-label)"}
                })))
                .expect(1).mount(&upstream).await;
            let client = read_client(&upstream);
            let listed = client.networks("server-1").await.unwrap();
            assert_eq!(listed[0].ipam_subnet.as_deref(), Some("10.20.0.0/24"));
            assert_eq!(listed[0].ipam_gateway.as_deref(), Some("10.20.0.1"));
            let network = client.network("server-1", "application").await.unwrap();
            let endpoint = &network.containers.as_ref().unwrap()[0];
            assert_eq!(endpoint.container_id.as_deref(), Some("container-1"));
            assert_eq!(endpoint.endpoint_id.as_deref(), Some("endpoint-1"));
            assert_eq!(endpoint.ipv4_address.as_deref(), Some("10.20.0.3/24"));
            assert_eq!(endpoint.ipv6_address.as_deref(), Some("fd00::3/64"));
            assert_eq!(endpoint.mac_address.as_deref(), Some("02:42:0a:14:00:03"));
            let ipam = network.ipam.as_ref().unwrap();
            let config = &ipam.config.as_ref().unwrap()[0];
            assert_eq!(config.ip_range.as_deref(), Some("10.20.0.128/25"));
            assert_eq!(
                config.auxiliary_addresses.as_ref().unwrap()["router"],
                "10.20.0.2"
            );
            assert_eq!(ipam.options.as_ref().unwrap()["allocation"], "automatic");
            assert_eq!(
                network.options.as_ref().unwrap()["com.docker.network.bridge.name"],
                "br-app"
            );
            assert_eq!(
                network.labels.as_ref().unwrap()["owner"],
                "$(untrusted-label)"
            );
        }
    }

    #[test]
    fn request_names_follow_the_documented_core_rename() {
        for version in ["2.1.2", "2.2.0"] {
            assert_eq!(uses_current_names(version), Some(false));
        }
        for version in ["2.3.0", "v2.3.3", "2.4.0", "3.0.0"] {
            assert_eq!(uses_current_names(version), Some(true));
        }
        assert_eq!(uses_current_names("unknown"), None);
    }

    #[tokio::test]
    async fn concurrent_network_reads_share_version_and_refresh_after_expiry() {
        let upstream = MockServer::start().await;
        Mock::given(body_json(json!({"type":"GetVersion","params":{}})))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(20))
                    .set_body_json(json!({"version":"2.1.2"})),
            )
            .expect(1)
            .mount(&upstream)
            .await;
        Mock::given(body_json(
            json!({"type":"ListDockerNetworks","params":{"server":"server-1"}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(2)
        .mount(&upstream)
        .await;
        let client = read_client(&upstream);
        let other = client.clone();
        let (first, second) = tokio::join!(client.networks("server-1"), other.networks("server-1"));
        assert!(first.is_ok() && second.is_ok());
        assert_eq!(upstream.received_requests().await.unwrap().len(), 3);
        upstream.reset().await;
        client
            .network_version
            .lock()
            .await
            .as_mut()
            .unwrap()
            .expires_at = Instant::now();
        Mock::given(body_json(json!({"type":"GetVersion","params":{}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"version":"2.3.3"})))
            .expect(1)
            .mount(&upstream)
            .await;
        Mock::given(body_json(
            json!({"type":"ListNetworks","params":{"server":"server-1"}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&upstream)
        .await;
        assert!(client.networks("server-1").await.is_ok());
    }

    #[tokio::test]
    async fn version_cache_does_not_mask_reachability_or_cache_invalid_versions() {
        let upstream = MockServer::start().await;
        Mock::given(body_json(json!({"type":"GetVersion","params":{}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"version":"2.3.3"})))
            .expect(1)
            .mount(&upstream)
            .await;
        let client = read_client(&upstream);
        assert!(client.current_network_names().await.unwrap());
        upstream.reset().await;
        Mock::given(body_json(json!({"type":"GetVersion","params":{}})))
            .respond_with(ResponseTemplate::new(503))
            .expect(2)
            .mount(&upstream)
            .await;
        assert!(client.version().await.is_err());
        upstream.reset().await;
        client
            .network_version
            .lock()
            .await
            .as_mut()
            .unwrap()
            .expires_at = Instant::now();
        Mock::given(body_json(json!({"type":"GetVersion","params":{}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"version":"unknown"})))
            .expect(2)
            .mount(&upstream)
            .await;
        for _ in 0..2 {
            assert_eq!(
                client.current_network_names().await,
                Err(crate::ApiError::IncompatibleResponse)
            );
        }
    }

    #[test]
    fn absent_facts_remain_distinct_from_reported_empty_or_false_values() {
        let absent: Network = serde_json::from_str("{}").unwrap();
        let present: Network =
            serde_json::from_str(r#"{"Internal":false,"Containers":[],"Labels":{},"Options":{}}"#)
                .unwrap();
        assert_eq!(absent.internal, None);
        assert_eq!(present.internal, Some(false));
        assert_eq!(absent.containers, None);
        assert_eq!(present.containers, Some(vec![]));
        assert_eq!(absent.labels, None);
        assert_eq!(present.labels, Some(BTreeMap::new()));
        assert_eq!(present.options, Some(BTreeMap::new()));
    }
}
