//! Typed filters for Komodo update history.

use serde_json::{Map, Value, json};

use crate::{OperationItem, OperationTarget};

/// Known Core update states, with an explicit match for future states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStatusFilter {
    Queued,
    InProgress,
    Complete,
    Other,
}

impl UpdateStatusFilter {
    fn matches(self, status: &str) -> bool {
        match self {
            Self::Queued => status == "Queued",
            Self::InProgress => status == "InProgress",
            Self::Complete => status == "Complete",
            Self::Other => !matches!(status, "Queued" | "InProgress" | "Complete"),
        }
    }
}

/// Fixed update fields supported by the upstream query; no caller-supplied operators.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OperationQuery {
    pub target: Option<OperationTarget>,
    pub operation: Option<String>,
    pub status: Option<UpdateStatusFilter>,
    pub success: Option<bool>,
    pub started_after: Option<i64>,
    pub started_before: Option<i64>,
}

impl OperationQuery {
    pub(crate) fn upstream_query(&self) -> Option<Value> {
        let mut query = Map::new();
        if let Some(target) = &self.target {
            query.insert("target.type".into(), json!(target.kind));
            query.insert("target.id".into(), json!(target.id));
        }
        if let Some(operation) = &self.operation {
            query.insert("operation".into(), json!(operation));
        }
        if let Some(status) = self.status {
            query.insert(
                "status".into(),
                match status {
                    UpdateStatusFilter::Queued => json!("Queued"),
                    UpdateStatusFilter::InProgress => json!("InProgress"),
                    UpdateStatusFilter::Complete => json!("Complete"),
                    UpdateStatusFilter::Other => json!({"$nin":["Queued","InProgress","Complete"]}),
                },
            );
        }
        if let Some(success) = self.success {
            query.insert("success".into(), json!(success));
        }
        let mut time = Map::new();
        if let Some(after) = self.started_after {
            time.insert("$gte".into(), json!(after));
        }
        if let Some(before) = self.started_before {
            time.insert("$lt".into(), json!(before));
        }
        if !time.is_empty() {
            query.insert("start_ts".into(), Value::Object(time));
        }
        (!query.is_empty()).then_some(Value::Object(query))
    }

    /// Apply the same fixed-field filter for API implementations that read a page locally.
    #[must_use]
    pub fn matches(&self, item: &OperationItem) -> bool {
        self.target
            .as_ref()
            .is_none_or(|target| item.target.as_ref() == Some(target))
            && self
                .operation
                .as_ref()
                .is_none_or(|operation| item.operation == *operation)
            && self
                .status
                .is_none_or(|status| status.matches(&item.status))
            && self.success.is_none_or(|success| item.success == success)
            && self
                .started_after
                .is_none_or(|after| item.start_ts >= after)
            && self
                .started_before
                .is_none_or(|before| item.start_ts < before)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Client, Credentials, KomodoApi};
    use std::time::Duration;
    use url::Url;
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::body_json};

    #[tokio::test]
    async fn filtered_operation_read_uses_fixed_upstream_fields_and_preserves_continuation() {
        let server = MockServer::start().await;
        Mock::given(body_json(
            json!({"type":"ListUpdates","params":{"page":7,"query":{
                "target.type":"Stack", "target.id":"stack-id", "operation":"DeployStack",
                "status":"Complete", "success":true, "start_ts":{"$gte":100,"$lt":200}
            }}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"updates":[{
                "id":"operation-id", "operation":"DeployStack", "status":"Complete", "success":true,
                "start_ts":150, "target":{"type":"Stack","id":"stack-id"}
            }],"next_page":8})))
        .expect(1)
        .mount(&server)
        .await;
        let client = Client::new(
            Url::parse(&server.uri()).unwrap(),
            Credentials::new("synthetic-read".into(), "synthetic-secret".into()).unwrap(),
            Duration::from_secs(2),
        )
        .unwrap();
        let query = OperationQuery {
            target: Some(OperationTarget {
                kind: "Stack".into(),
                id: "stack-id".into(),
            }),
            operation: Some("DeployStack".into()),
            status: Some(UpdateStatusFilter::Complete),
            success: Some(true),
            started_after: Some(100),
            started_before: Some(200),
        };
        let page = client.operations_query(7, &query).await.unwrap();
        assert_eq!(page.next_page, Some(8));
        assert_eq!(page.operations[0].id, "operation-id");
        assert!(query.matches(&page.operations[0]));
    }
}
