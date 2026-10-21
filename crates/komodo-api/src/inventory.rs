//! Bounded inventory pages and exact selector resolution.

use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::{ApiError, Client, ResourceListItem};

const PAGE_SIZE: usize = 100;
const MAX_SELECTOR_PAGES: u32 = 16;

/// A single inventory page, or candidates for an exact id/name selector.
#[derive(Debug, Clone, Copy)]
pub enum InventoryRequest<'a> {
    Page(u32),
    Selector(&'a str),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Credentials, KomodoApi, StackInfo};
    use std::time::Duration;
    use url::Url;
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::body_json};

    fn client(server: &MockServer) -> Client {
        Client::new(
            Url::parse(&server.uri()).unwrap(),
            Credentials::new("synthetic-read".into(), "synthetic-secret".into()).unwrap(),
            Duration::from_secs(2),
        )
        .unwrap()
    }

    fn item(id: &str, name: &str) -> Value {
        json!({"id":id,"type":"Stack","name":name,"info":{"state":"Running","repo":"x".repeat(800)}})
    }

    async fn version(server: &MockServer, version: &str) {
        Mock::given(body_json(json!({"type":"GetVersion","params":{}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"version":version})))
            .expect(1)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn native_pages_and_exact_ids_do_not_download_the_complete_inventory() {
        let server = MockServer::start().await;
        let id = "0123456789abcdef01234567";
        let all = vec![item(id, "application"); 2500];
        assert!(serde_json::to_vec(&all).unwrap().len() > crate::MAXIMUM_RESPONSE_BYTES);
        // The old unfiltered request would exceed the transport's unchanged body cap.
        Mock::given(body_json(
            json!({"type":"ListStacks","params":{"query":{},"limit":0}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(all))
        .expect(0)
        .mount(&server)
        .await;
        Mock::given(body_json(
            json!({"type":"ListStacks","params":{"query":{"names":[id]},"limit":0}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([item(id, "application")])))
        .expect(1)
        .mount(&server)
        .await;
        version(&server, "2.3.3").await;
        for (page, items) in [
            (0, vec![item(id, "application"); PAGE_SIZE]),
            (1, vec![item("last", "last")]),
        ] {
            Mock::given(body_json(json!({"type":"ListStacks","params":{"query":{},"page":page,"limit":PAGE_SIZE,"sort_by":"Name","sort_desc":false}})))
                .respond_with(ResponseTemplate::new(200).set_body_json(items)).expect(1).mount(&server).await;
        }
        let client = client(&server);
        let selected = client
            .stacks_inventory(InventoryRequest::Selector(id))
            .await
            .unwrap();
        assert_eq!(selected.items[0].id, id);
        let first = client
            .stacks_inventory(InventoryRequest::Page(0))
            .await
            .unwrap();
        assert_eq!(first.items.len(), PAGE_SIZE);
        assert_eq!(first.next_page, Some(1));
        let last = client
            .stacks_inventory(InventoryRequest::Page(first.next_page.unwrap()))
            .await
            .unwrap();
        assert_eq!(last.items[0].id, "last");
        assert_eq!(last.next_page, None);
    }

    #[tokio::test]
    async fn name_resolution_retains_case_ambiguity_across_core_pages() {
        let server = MockServer::start().await;
        version(&server, "2.3.3").await;
        for (page, items) in [
            (0, vec![item("substring", "application-extra"); PAGE_SIZE]),
            (
                1,
                vec![item("first", "Application"), item("second", "APPLICATION")],
            ),
        ] {
            Mock::given(body_json(json!({"type":"ListStacks","params":{"query":{"terms":["application"]},"page":page,"limit":PAGE_SIZE,"sort_by":"Name","sort_desc":false}})))
                .respond_with(ResponseTemplate::new(200).set_body_json(items)).expect(1).mount(&server).await;
        }
        let selected = client(&server)
            .stacks_inventory(InventoryRequest::Selector("application"))
            .await
            .unwrap();
        assert_eq!(
            selected
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
    }

    #[tokio::test]
    async fn broad_names_fail_with_id_guidance_instead_of_returning_an_unproven_match() {
        let server = MockServer::start().await;
        version(&server, "2.3.3").await;
        for page in 0..MAX_SELECTOR_PAGES {
            Mock::given(body_json(json!({"type":"ListStacks","params":{"query":{"terms":["app"]},"page":page,"limit":PAGE_SIZE,"sort_by":"Name","sort_desc":false}})))
                .respond_with(ResponseTemplate::new(200).set_body_json(vec![item("substring", "app-extra"); PAGE_SIZE]))
                .expect(1).mount(&server).await;
        }
        assert!(matches!(
            client(&server)
                .stacks_inventory(InventoryRequest::Selector("app"))
                .await,
            Err(ApiError::InventorySelectorTooBroad)
        ));
    }

    #[tokio::test]
    async fn older_core_keeps_bounded_legacy_page_zero_without_claiming_pagination() {
        let server = MockServer::start().await;
        version(&server, "2.1.2").await;
        Mock::given(body_json(
            json!({"type":"ListStacks","params":{"query":{},"limit":0}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([item("id", "application")])))
        .expect(1)
        .mount(&server)
        .await;
        let client = client(&server);
        let page = client
            .stacks_inventory(InventoryRequest::Page(0))
            .await
            .unwrap();
        assert_eq!(page.next_page, None);
        assert_eq!(page.items[0].id, "id");
        assert!(matches!(
            client.stacks_inventory(InventoryRequest::Page(1)).await,
            Err(ApiError::InventoryPaginationUnsupported)
        ));
        assert!(matches!(
            client
                .inventory::<StackInfo>("ListStacks", InventoryRequest::Selector(""))
                .await,
            Err(ApiError::InvalidConfiguration(_))
        ));
    }
}

/// One bounded Core response and its continuation.
#[derive(Debug)]
pub struct InventoryPage<I> {
    pub items: Vec<ResourceListItem<I>>,
    pub next_page: Option<u32>,
}

impl<I> InventoryPage<I> {
    /// Adapt an API implementation without native inventory pagination.
    ///
    /// # Errors
    /// Returns an error for nonzero pages, which the implementation cannot serve.
    pub fn from_legacy(
        items: Vec<ResourceListItem<I>>,
        request: InventoryRequest<'_>,
    ) -> Result<Self, ApiError> {
        if matches!(request, InventoryRequest::Page(1..)) {
            return Err(ApiError::InventoryPaginationUnsupported);
        }
        Ok(Self {
            items,
            next_page: None,
        })
    }
}

impl Client {
    pub(crate) async fn inventory<I: DeserializeOwned>(
        &self,
        variant: &'static str,
        request: InventoryRequest<'_>,
    ) -> Result<InventoryPage<I>, ApiError> {
        let read = self.inventory_inner(variant, request);
        if matches!(request, InventoryRequest::Selector(_)) {
            tokio::time::timeout(std::time::Duration::from_secs(30), read)
                .await
                .map_err(|_| ApiError::Unavailable)?
        } else {
            read.await
        }
    }

    async fn inventory_inner<I: DeserializeOwned>(
        &self,
        variant: &'static str,
        request: InventoryRequest<'_>,
    ) -> Result<InventoryPage<I>, ApiError> {
        if let InventoryRequest::Selector(selector) = request {
            if selector.is_empty() || selector.len() > 128 || selector.chars().any(char::is_control)
            {
                return Err(ApiError::InvalidConfiguration("inventory selector"));
            }
            // Object ids can be queried directly on every supported Core release.
            if selector.len() == 24 && selector.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                let items: Vec<ResourceListItem<I>> = self
                    .post(
                        "read",
                        variant,
                        json!({"query":{"names":[selector]},"limit":0}),
                        true,
                    )
                    .await?;
                if items.iter().any(|item| item.id == selector) {
                    return Ok(InventoryPage {
                        items,
                        next_page: None,
                    });
                }
            }
        }
        if !self.core_2_3_or_later().await? {
            if matches!(request, InventoryRequest::Page(1..)) {
                return Err(ApiError::InventoryPaginationUnsupported);
            }
            return InventoryPage::from_legacy(self.read(variant).await?, request);
        }
        match request {
            InventoryRequest::Page(page) => self.inventory_page(variant, json!({}), page).await,
            InventoryRequest::Selector(selector) => {
                // Core escapes terms before matching names case-insensitively. Retain
                // exact matches locally so substring matches cannot select a mutation.
                let mut matches = Vec::new();
                for page in 0..MAX_SELECTOR_PAGES {
                    let response = self
                        .inventory_page::<I>(variant, json!({"terms":[selector]}), page)
                        .await?;
                    for item in response.items {
                        if item.name.eq_ignore_ascii_case(selector) {
                            matches.push(item);
                            if matches.len() == 2 {
                                return Ok(InventoryPage {
                                    items: matches,
                                    next_page: None,
                                });
                            }
                        }
                    }
                    if response.next_page.is_none() {
                        return Ok(InventoryPage {
                            items: matches,
                            next_page: None,
                        });
                    }
                }
                Err(ApiError::InventorySelectorTooBroad)
            }
        }
    }

    async fn inventory_page<I: DeserializeOwned>(
        &self,
        variant: &'static str,
        query: Value,
        page: u32,
    ) -> Result<InventoryPage<I>, ApiError> {
        let items: Vec<ResourceListItem<I>> = self.post(
            "read", variant,
            json!({"query":query,"page":page,"limit":PAGE_SIZE,"sort_by":"Name","sort_desc":false}), true,
        ).await?;
        if items.len() > PAGE_SIZE {
            return Err(ApiError::IncompatibleResponse);
        }
        let next_page = if items.len() == PAGE_SIZE {
            Some(page.checked_add(1).ok_or(ApiError::IncompatibleResponse)?)
        } else {
            None
        };
        Ok(InventoryPage { items, next_page })
    }
}
