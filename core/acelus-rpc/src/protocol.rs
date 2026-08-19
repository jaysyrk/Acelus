use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const JSONRPC_VERSION: &str = "2.0";

pub mod codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;

    pub const INTERNAL: i32 = -32000;
    pub const NOT_FOUND: i32 = -32001;
    pub const ALREADY_EXISTS: i32 = -32002;
    pub const NETWORK: i32 = -32003;
    pub const INTEGRITY_FAILED: i32 = -32004;

    pub const AUTH_REQUIRED: i32 = -32010;
    pub const AUTH_EXPIRED: i32 = -32011;
    pub const XBOX_ACCOUNT_MISSING: i32 = -32012;
    pub const XBOX_CHILD_ACCOUNT: i32 = -32013;
    pub const XBOX_REGION_UNAVAILABLE: i32 = -32014;
    pub const XBOX_ACCOUNT_BANNED: i32 = -32015;
    pub const XBOX_ADULT_VERIFICATION_REQUIRED: i32 = -32016;
    pub const ENTITLEMENT_MISSING: i32 = -32017;
    pub const ENTITLEMENT_SIGNATURE_INVALID: i32 = -32018;
    pub const AZURE_APP_UNAPPROVED: i32 = -32019;

    pub const JAVA_UNAVAILABLE: i32 = -32030;
    pub const LOADER_UNSUPPORTED: i32 = -32031;
    pub const DEPENDENCY_CONFLICT: i32 = -32032;

    pub const INSTANCE_RUNNING: i32 = -32040;
    pub const SANDBOX_UNAVAILABLE: i32 = -32041;
}

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl Request {
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }

    pub fn is_well_formed(&self) -> bool {
        self.jsonrpc == JSONRPC_VERSION && !self.method.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    pub fn method_not_found(method: &str) -> Self {
        Self::new(codes::METHOD_NOT_FOUND, format!("no method named {method}"))
    }

    pub fn invalid_params(detail: impl Into<String>) -> Self {
        Self::new(codes::INVALID_PARAMS, detail)
    }

    pub fn internal(detail: impl Into<String>) -> Self {
        Self::new(codes::INTERNAL, detail)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(id: Value, error: RpcError) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            id,
            result: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub jsonrpc: &'static str,
    pub method: String,
    pub params: Value,
}

impl Notification {
    pub fn new(method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            method: method.into(),
            params,
        }
    }
}

pub fn encode<T: Serialize>(message: &T) -> String {
    let mut line = serde_json::to_string(message).unwrap_or_else(|error| {
        serde_json::json!({
            "jsonrpc": JSONRPC_VERSION,
            "error": {"code": codes::INTERNAL, "message": error.to_string()}
        })
        .to_string()
    });
    line.push('\n');
    line
}

pub fn decode(line: &str) -> std::result::Result<Request, RpcError> {
    let request: Request = serde_json::from_str(line)
        .map_err(|error| RpcError::new(codes::PARSE_ERROR, error.to_string()))?;

    if !request.is_well_formed() {
        return Err(RpcError::new(
            codes::INVALID_REQUEST,
            "a request must carry jsonrpc 2.0 and a method name",
        ));
    }

    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_well_formed_request_decodes() {
        let request = decode(r#"{"jsonrpc":"2.0","id":1,"method":"daemon.info","params":{}}"#)
            .expect("the request decodes");

        assert_eq!(request.method, "daemon.info");
        assert_eq!(request.id, Some(json!(1)));
        assert!(!request.is_notification());
    }

    #[test]
    fn a_request_without_an_id_is_a_notification() {
        let request = decode(r#"{"jsonrpc":"2.0","method":"daemon.info"}"#).unwrap();
        assert!(request.is_notification());
    }

    #[test]
    fn malformed_json_is_a_parse_error() {
        let error = decode("{not json").unwrap_err();
        assert_eq!(error.code, codes::PARSE_ERROR);
    }

    #[test]
    fn a_request_from_another_protocol_version_is_refused() {
        let error = decode(r#"{"jsonrpc":"1.0","id":1,"method":"daemon.info"}"#).unwrap_err();
        assert_eq!(error.code, codes::INVALID_REQUEST);
    }

    #[test]
    fn a_request_without_a_method_is_refused() {
        let error = decode(r#"{"jsonrpc":"2.0","id":1,"method":""}"#).unwrap_err();
        assert_eq!(error.code, codes::INVALID_REQUEST);
    }

    #[test]
    fn responses_are_newline_delimited_and_carry_one_of_result_or_error() {
        let success = encode(&Response::success(json!(1), json!({"ok": true})));
        assert!(success.ends_with('\n'));
        assert!(success.contains("\"result\""));
        assert!(!success.contains("\"error\""));

        let failure = encode(&Response::failure(
            json!(1),
            RpcError::method_not_found("nope"),
        ));
        assert!(failure.contains("\"error\""));
        assert!(!failure.contains("\"result\""));
    }

    #[test]
    fn an_encoded_message_contains_no_embedded_newline() {
        let encoded = encode(&Notification::new(
            "log.line",
            json!({"line": "first\nsecond"}),
        ));

        assert_eq!(
            encoded.matches('\n').count(),
            1,
            "an embedded newline would split one message into two frames"
        );
    }

    #[test]
    fn errors_can_carry_structured_detail() {
        let error =
            RpcError::invalid_params("version is required").with_data(json!({"field": "version"}));

        assert_eq!(error.code, codes::INVALID_PARAMS);
        assert_eq!(error.data.unwrap()["field"], "version");
    }
}
