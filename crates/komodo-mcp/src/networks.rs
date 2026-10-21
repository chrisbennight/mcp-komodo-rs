//! Typed server network inventory and addressing reads.

use komodo_api::networks::{Network, NetworkListItem};
use rmcp::model::{CallToolRequestParams, CallToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    CollectionPage, CollectionWindow, KomodoMcp, McpError, collection_page, parse, select,
    structured, validate_query, validate_selector,
};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SearchInput {
    /// Exact Komodo server name or id, 1 to 128 UTF-8 bytes without controls.
    #[schemars(
        length(min = 1, max = 128),
        pattern(r"^[^\u0000-\u001F\u007F-\u009F]*$")
    )]
    server: String,
    /// Optional ASCII-case-insensitive match against network name or id.
    /// At most 128 UTF-8 bytes before trimming, without control characters.
    #[schemars(length(max = 128), pattern(r"^[^\u0000-\u001F\u007F-\u009F]*$"))]
    query: Option<String>,
    /// Select a page from the bounded network inventory, defaulting to 250 records.
    #[serde(default)]
    window: CollectionWindow,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadInput {
    /// Exact Komodo server name or id, 1 to 128 UTF-8 bytes without controls.
    #[schemars(
        length(min = 1, max = 128),
        pattern(r"^[^\u0000-\u001F\u007F-\u009F]*$")
    )]
    server: String,
    /// Network name or id sent to Core, 1 to 128 UTF-8 bytes without controls.
    #[schemars(
        length(min = 1, max = 128),
        pattern(r"^[^\u0000-\u001F\u007F-\u009F]*$")
    )]
    network: String,
}

#[derive(Debug, Serialize, JsonSchema)]
struct ServerIdentity {
    id: String,
    name: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct SearchOutput {
    server: ServerIdentity,
    items: Vec<NetworkListItem>,
    /// Completeness and continuation for the filtered inventory.
    items_page: CollectionPage,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ReadOutput {
    server: ServerIdentity,
    network: Network,
}

impl KomodoMcp {
    pub(super) async fn search_networks(
        &self,
        params: &CallToolRequestParams,
    ) -> Result<CallToolResult, McpError> {
        let input = parse::<SearchInput>(params)?;
        validate_selector(&input.server)?;
        input.window.validate()?;
        let query = validate_query(input.query.as_deref())?;
        let server = select(
            self.read
                .servers_inventory(komodo_api::inventory::InventoryRequest::Selector(
                    &input.server,
                ))
                .await
                .map_err(|error| self.api_error(error))?
                .items,
            &input.server,
        )?;
        let mut items = self
            .read
            .networks(&server.id)
            .await
            .map_err(|error| self.api_error(error))?;
        if let Some(query) = query {
            items.retain(|item| {
                [&item.name, &item.id].into_iter().any(|field| {
                    field
                        .as_ref()
                        .is_some_and(|value| value.to_ascii_lowercase().contains(&query))
                })
            });
        }
        items.sort_by(|left, right| {
            left.name
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .cmp(
                    &right
                        .name
                        .as_deref()
                        .unwrap_or_default()
                        .to_ascii_lowercase(),
                )
                .then_with(|| left.id.cmp(&right.id))
        });
        let (items, items_page) = collection_page(Some(items), input.window);
        structured(SearchOutput {
            server: ServerIdentity {
                id: server.id,
                name: server.name,
            },
            items,
            items_page,
        })
    }

    pub(super) async fn read_network(
        &self,
        params: &CallToolRequestParams,
    ) -> Result<CallToolResult, McpError> {
        let input = parse::<ReadInput>(params)?;
        validate_selector(&input.server)?;
        validate_selector(&input.network)?;
        let server = select(
            self.read
                .servers_inventory(komodo_api::inventory::InventoryRequest::Selector(
                    &input.server,
                ))
                .await
                .map_err(|error| self.api_error(error))?
                .items,
            &input.server,
        )?;
        let network = self
            .read
            .network(&server.id, &input.network)
            .await
            .map_err(|error| self.api_error(error))?;
        structured(ReadOutput {
            server: ServerIdentity {
                id: server.id,
                name: server.name,
            },
            network,
        })
    }
}
