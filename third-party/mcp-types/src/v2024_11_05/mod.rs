mod types;

pub use types::*;

use std::hash::{Hash, Hasher};

impl Hash for RequestId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.to_string().hash(state);
    }
}

impl Eq for RequestId {}

/// Trait for getting the request ID from a [`JsonrpcMessage`]
pub trait GetRequestId {
    fn get_request_id(&self) -> Option<RequestId>;
}

impl GetRequestId for JsonrpcMessage {
    fn get_request_id(&self) -> Option<RequestId> {
        match self {
            JsonrpcMessage::Request(request) => Some(request.id.clone()),
            JsonrpcMessage::Response(response) => Some(response.id.clone()),
            JsonrpcMessage::Error(error) => Some(error.id.clone()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Map};

    use crate::v2024_11_05::{
        GetRequestId, JsonrpcError, JsonrpcMessage, JsonrpcNotification, JsonrpcRequest,
        JsonrpcRequestParams, JsonrpcRequestParamsMeta, JsonrpcResponse, RequestId,
        Result as McpResult,
    };

    #[test]
    fn test_get_request_id() {
        let message = JsonrpcMessage::Request(JsonrpcRequest {
            jsonrpc: "2.0".to_string(),
            id: RequestId::Integer(1),
            method: "test".to_string(),
            params: Some(JsonrpcRequestParams {
                meta: Some(JsonrpcRequestParamsMeta {
                    progress_token: None,
                }),
            }),
        });

        assert_eq!(message.get_request_id(), Some(RequestId::Integer(1)));

        let message = JsonrpcMessage::Response(JsonrpcResponse {
            jsonrpc: "2.0".to_string(),
            id: RequestId::Integer(1),
            result: McpResult { meta: Map::new() },
        });
        assert_eq!(message.get_request_id(), Some(RequestId::Integer(1)));
    }

    #[test]
    fn test_serialize_request_as_message() {
        let value = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "test",
            "params": {},
        });

        let request: JsonrpcRequest =
            serde_json::from_value(value.clone()).expect("failed to convert value to request");
        let message: JsonrpcMessage =
            serde_json::from_value(value).expect("failed to convert value to message");
        assert_eq!(message, JsonrpcMessage::Request(request));
    }

    #[test]
    fn test_serialize_response_as_message() {
        let value = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "test": "test",
            },
        });

        let response: JsonrpcResponse =
            serde_json::from_value(value.clone()).expect("failed to convert value to response");
        let message: JsonrpcMessage =
            serde_json::from_value(value).expect("failed to convert value to message");
        assert_eq!(message, JsonrpcMessage::Response(response));
    }

    #[test]
    fn test_serialize_error_as_message() {
        let value = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32000,
                "message": "test",
                "data": "test",
            },
        });

        let error: JsonrpcError =
            serde_json::from_value(value.clone()).expect("failed to convert value to error");
        let message: JsonrpcMessage =
            serde_json::from_value(value).expect("failed to convert value to message");
        assert_eq!(message, JsonrpcMessage::Error(error));
    }

    #[test]
    fn test_serialize_notification_as_message() {
        let value = json!({
            "jsonrpc": "2.0",
            "method": "test",
            "params": {},
        });

        let notification: JsonrpcNotification =
            serde_json::from_value(value.clone()).expect("failed to convert value to notification");
        let message: JsonrpcMessage =
            serde_json::from_value(value).expect("failed to convert value to message");
        assert_eq!(message, JsonrpcMessage::Notification(notification));
    }
}
