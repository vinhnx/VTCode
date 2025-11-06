/// Error types.
pub mod error {
    /// Error from a TryFrom or FromStr implementation.
    pub struct ConversionError(::std::borrow::Cow<'static, str>);
    impl ::std::error::Error for ConversionError {}
    impl ::std::fmt::Display for ConversionError {
        fn fmt(
            &self,
            f: &mut ::std::fmt::Formatter<'_>,
        ) -> Result<(), ::std::fmt::Error> {
            ::std::fmt::Display::fmt(&self.0, f)
        }
    }
    impl ::std::fmt::Debug for ConversionError {
        fn fmt(
            &self,
            f: &mut ::std::fmt::Formatter<'_>,
        ) -> Result<(), ::std::fmt::Error> {
            ::std::fmt::Debug::fmt(&self.0, f)
        }
    }
    impl From<&'static str> for ConversionError {
        fn from(value: &'static str) -> Self {
            Self(value.into())
        }
    }
    impl From<String> for ConversionError {
        fn from(value: String) -> Self {
            Self(value.into())
        }
    }
}
///Base for objects that include optional annotations for the client. The client can use annotations to inform how objects are used or displayed
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Base for objects that include optional annotations for the client. The client can use annotations to inform how objects are used or displayed",
///  "type": "object",
///  "properties": {
///    "annotations": {
///      "type": "object",
///      "properties": {
///        "audience": {
///          "description": "Describes who the intended customer of this object or data is.\n\nIt can include multiple entries to indicate content useful for multiple audiences (e.g., `[\"user\", \"assistant\"]`).",
///          "type": "array",
///          "items": {
///            "$ref": "#/definitions/Role"
///          }
///        },
///        "priority": {
///          "description": "Describes how important this data is for operating the server.\n\nA value of 1 means \"most important,\" and indicates that the data is\neffectively required, while 0 means \"least important,\" and indicates that\nthe data is entirely optional.",
///          "type": "number",
///          "maximum": 1.0,
///          "minimum": 0.0
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct Annotated {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub annotations: ::std::option::Option<AnnotatedAnnotations>,
}
impl ::std::convert::From<&Annotated> for Annotated {
    fn from(value: &Annotated) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Annotated {
    fn default() -> Self {
        Self {
            annotations: Default::default(),
        }
    }
}
///AnnotatedAnnotations
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "audience": {
///      "description": "Describes who the intended customer of this object or data is.\n\nIt can include multiple entries to indicate content useful for multiple audiences (e.g., `[\"user\", \"assistant\"]`).",
///      "type": "array",
///      "items": {
///        "$ref": "#/definitions/Role"
///      }
///    },
///    "priority": {
///      "description": "Describes how important this data is for operating the server.\n\nA value of 1 means \"most important,\" and indicates that the data is\neffectively required, while 0 means \"least important,\" and indicates that\nthe data is entirely optional.",
///      "type": "number",
///      "maximum": 1.0,
///      "minimum": 0.0
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct AnnotatedAnnotations {
    /**Describes who the intended customer of this object or data is.

It can include multiple entries to indicate content useful for multiple audiences (e.g., `["user", "assistant"]`).*/
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub audience: ::std::vec::Vec<Role>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub priority: ::std::option::Option<f64>,
}
impl ::std::convert::From<&AnnotatedAnnotations> for AnnotatedAnnotations {
    fn from(value: &AnnotatedAnnotations) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for AnnotatedAnnotations {
    fn default() -> Self {
        Self {
            audience: Default::default(),
            priority: Default::default(),
        }
    }
}
///BlobResourceContents
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "blob",
///    "uri"
///  ],
///  "properties": {
///    "blob": {
///      "description": "A base64-encoded string representing the binary data of the item.",
///      "type": "string",
///      "format": "byte"
///    },
///    "mimeType": {
///      "description": "The MIME type of this resource, if known.",
///      "type": "string"
///    },
///    "uri": {
///      "description": "The URI of this resource.",
///      "type": "string",
///      "format": "uri"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct BlobResourceContents {
    ///A base64-encoded string representing the binary data of the item.
    pub blob: ::std::string::String,
    ///The MIME type of this resource, if known.
    #[serde(
        rename = "mimeType",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub mime_type: ::std::option::Option<::std::string::String>,
    ///The URI of this resource.
    pub uri: ::std::string::String,
}
impl ::std::convert::From<&BlobResourceContents> for BlobResourceContents {
    fn from(value: &BlobResourceContents) -> Self {
        value.clone()
    }
}
///Used by the client to invoke a tool provided by the server.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Used by the client to invoke a tool provided by the server.",
///  "type": "object",
///  "required": [
///    "method",
///    "params"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "tools/call"
///    },
///    "params": {
///      "type": "object",
///      "required": [
///        "name"
///      ],
///      "properties": {
///        "arguments": {
///          "type": "object",
///          "additionalProperties": {}
///        },
///        "name": {
///          "type": "string"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct CallToolRequest {
    pub method: ::std::string::String,
    pub params: CallToolRequestParams,
}
impl ::std::convert::From<&CallToolRequest> for CallToolRequest {
    fn from(value: &CallToolRequest) -> Self {
        value.clone()
    }
}
///CallToolRequestParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "name"
///  ],
///  "properties": {
///    "arguments": {
///      "type": "object",
///      "additionalProperties": {}
///    },
///    "name": {
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct CallToolRequestParams {
    #[serde(default, skip_serializing_if = "::serde_json::Map::is_empty")]
    pub arguments: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    pub name: ::std::string::String,
}
impl ::std::convert::From<&CallToolRequestParams> for CallToolRequestParams {
    fn from(value: &CallToolRequestParams) -> Self {
        value.clone()
    }
}
/**The server's response to a tool call.

Any errors that originate from the tool SHOULD be reported inside the result
object, with `isError` set to true, _not_ as an MCP protocol-level error
response. Otherwise, the LLM would not be able to see that an error occurred
and self-correct.

However, any errors in _finding_ the tool, an error indicating that the
server does not support tool calls, or any other exceptional conditions,
should be reported as an MCP error response.*/
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The server's response to a tool call.\n\nAny errors that originate from the tool SHOULD be reported inside the result\nobject, with `isError` set to true, _not_ as an MCP protocol-level error\nresponse. Otherwise, the LLM would not be able to see that an error occurred\nand self-correct.\n\nHowever, any errors in _finding_ the tool, an error indicating that the\nserver does not support tool calls, or any other exceptional conditions,\nshould be reported as an MCP error response.",
///  "type": "object",
///  "required": [
///    "content"
///  ],
///  "properties": {
///    "_meta": {
///      "description": "This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.",
///      "type": "object",
///      "additionalProperties": {}
///    },
///    "content": {
///      "type": "array",
///      "items": {
///        "anyOf": [
///          {
///            "$ref": "#/definitions/TextContent"
///          },
///          {
///            "$ref": "#/definitions/ImageContent"
///          },
///          {
///            "$ref": "#/definitions/EmbeddedResource"
///          }
///        ]
///      }
///    },
///    "isError": {
///      "description": "Whether the tool call ended in an error.\n\nIf not set, this is assumed to be false (the call was successful).",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct CallToolResult {
    pub content: ::std::vec::Vec<CallToolResultContentItem>,
    /**Whether the tool call ended in an error.

If not set, this is assumed to be false (the call was successful).*/
    #[serde(
        rename = "isError",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub is_error: ::std::option::Option<bool>,
    ///This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
}
impl ::std::convert::From<&CallToolResult> for CallToolResult {
    fn from(value: &CallToolResult) -> Self {
        value.clone()
    }
}
///CallToolResultContentItem
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "anyOf": [
///    {
///      "$ref": "#/definitions/TextContent"
///    },
///    {
///      "$ref": "#/definitions/ImageContent"
///    },
///    {
///      "$ref": "#/definitions/EmbeddedResource"
///    }
///  ]
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum CallToolResultContentItem {
    TextContent(TextContent),
    ImageContent(ImageContent),
    EmbeddedResource(EmbeddedResource),
}
impl ::std::convert::From<&Self> for CallToolResultContentItem {
    fn from(value: &CallToolResultContentItem) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<TextContent> for CallToolResultContentItem {
    fn from(value: TextContent) -> Self {
        Self::TextContent(value)
    }
}
impl ::std::convert::From<ImageContent> for CallToolResultContentItem {
    fn from(value: ImageContent) -> Self {
        Self::ImageContent(value)
    }
}
impl ::std::convert::From<EmbeddedResource> for CallToolResultContentItem {
    fn from(value: EmbeddedResource) -> Self {
        Self::EmbeddedResource(value)
    }
}
/**This notification can be sent by either side to indicate that it is cancelling a previously-issued request.

The request SHOULD still be in-flight, but due to communication latency, it is always possible that this notification MAY arrive after the request has already finished.

This notification indicates that the result will be unused, so any associated processing SHOULD cease.

A client MUST NOT attempt to cancel its `initialize` request.*/
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "This notification can be sent by either side to indicate that it is cancelling a previously-issued request.\n\nThe request SHOULD still be in-flight, but due to communication latency, it is always possible that this notification MAY arrive after the request has already finished.\n\nThis notification indicates that the result will be unused, so any associated processing SHOULD cease.\n\nA client MUST NOT attempt to cancel its `initialize` request.",
///  "type": "object",
///  "required": [
///    "method",
///    "params"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "notifications/cancelled"
///    },
///    "params": {
///      "type": "object",
///      "required": [
///        "requestId"
///      ],
///      "properties": {
///        "reason": {
///          "description": "An optional string describing the reason for the cancellation. This MAY be logged or presented to the user.",
///          "type": "string"
///        },
///        "requestId": {
///          "description": "The ID of the request to cancel.\n\nThis MUST correspond to the ID of a request previously issued in the same direction.",
///          "$ref": "#/definitions/RequestId"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct CancelledNotification {
    pub method: ::std::string::String,
    pub params: CancelledNotificationParams,
}
impl ::std::convert::From<&CancelledNotification> for CancelledNotification {
    fn from(value: &CancelledNotification) -> Self {
        value.clone()
    }
}
///CancelledNotificationParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "requestId"
///  ],
///  "properties": {
///    "reason": {
///      "description": "An optional string describing the reason for the cancellation. This MAY be logged or presented to the user.",
///      "type": "string"
///    },
///    "requestId": {
///      "description": "The ID of the request to cancel.\n\nThis MUST correspond to the ID of a request previously issued in the same direction.",
///      "$ref": "#/definitions/RequestId"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct CancelledNotificationParams {
    ///An optional string describing the reason for the cancellation. This MAY be logged or presented to the user.
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub reason: ::std::option::Option<::std::string::String>,
    /**The ID of the request to cancel.

This MUST correspond to the ID of a request previously issued in the same direction.*/
    #[serde(rename = "requestId")]
    pub request_id: RequestId,
}
impl ::std::convert::From<&CancelledNotificationParams> for CancelledNotificationParams {
    fn from(value: &CancelledNotificationParams) -> Self {
        value.clone()
    }
}
///Capabilities a client may support. Known capabilities are defined here, in this schema, but this is not a closed set: any client can define its own, additional capabilities.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Capabilities a client may support. Known capabilities are defined here, in this schema, but this is not a closed set: any client can define its own, additional capabilities.",
///  "type": "object",
///  "properties": {
///    "experimental": {
///      "description": "Experimental, non-standard capabilities that the client supports.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "object",
///        "additionalProperties": true
///      }
///    },
///    "roots": {
///      "description": "Present if the client supports listing roots.",
///      "type": "object",
///      "properties": {
///        "listChanged": {
///          "description": "Whether the client supports notifications for changes to the roots list.",
///          "type": "boolean"
///        }
///      }
///    },
///    "sampling": {
///      "description": "Present if the client supports sampling from an LLM.",
///      "type": "object",
///      "additionalProperties": true
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ClientCapabilities {
    ///Experimental, non-standard capabilities that the client supports.
    #[serde(default, skip_serializing_if = ":: std :: collections :: HashMap::is_empty")]
    pub experimental: ::std::collections::HashMap<
        ::std::string::String,
        ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    >,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub roots: ::std::option::Option<ClientCapabilitiesRoots>,
    ///Present if the client supports sampling from an LLM.
    #[serde(default, skip_serializing_if = "::serde_json::Map::is_empty")]
    pub sampling: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    ///Present if the client supports elicitation requests from the server.
    #[serde(default, skip_serializing_if = "::serde_json::Map::is_empty")]
    pub elicitation: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
}
impl ::std::convert::From<&ClientCapabilities> for ClientCapabilities {
    fn from(value: &ClientCapabilities) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ClientCapabilities {
    fn default() -> Self {
        Self {
            experimental: Default::default(),
            roots: Default::default(),
            sampling: Default::default(),
            elicitation: Default::default(),
        }
    }
}
///Present if the client supports listing roots.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Present if the client supports listing roots.",
///  "type": "object",
///  "properties": {
///    "listChanged": {
///      "description": "Whether the client supports notifications for changes to the roots list.",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ClientCapabilitiesRoots {
    ///Whether the client supports notifications for changes to the roots list.
    #[serde(
        rename = "listChanged",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub list_changed: ::std::option::Option<bool>,
}
impl ::std::convert::From<&ClientCapabilitiesRoots> for ClientCapabilitiesRoots {
    fn from(value: &ClientCapabilitiesRoots) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ClientCapabilitiesRoots {
    fn default() -> Self {
        Self {
            list_changed: Default::default(),
        }
    }
}
///ClientNotification
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "anyOf": [
///    {
///      "$ref": "#/definitions/CancelledNotification"
///    },
///    {
///      "$ref": "#/definitions/InitializedNotification"
///    },
///    {
///      "$ref": "#/definitions/ProgressNotification"
///    },
///    {
///      "$ref": "#/definitions/RootsListChangedNotification"
///    }
///  ]
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum ClientNotification {
    CancelledNotification(CancelledNotification),
    InitializedNotification(InitializedNotification),
    ProgressNotification(ProgressNotification),
    RootsListChangedNotification(RootsListChangedNotification),
}
impl ::std::convert::From<&Self> for ClientNotification {
    fn from(value: &ClientNotification) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<CancelledNotification> for ClientNotification {
    fn from(value: CancelledNotification) -> Self {
        Self::CancelledNotification(value)
    }
}
impl ::std::convert::From<InitializedNotification> for ClientNotification {
    fn from(value: InitializedNotification) -> Self {
        Self::InitializedNotification(value)
    }
}
impl ::std::convert::From<ProgressNotification> for ClientNotification {
    fn from(value: ProgressNotification) -> Self {
        Self::ProgressNotification(value)
    }
}
impl ::std::convert::From<RootsListChangedNotification> for ClientNotification {
    fn from(value: RootsListChangedNotification) -> Self {
        Self::RootsListChangedNotification(value)
    }
}
///ClientRequest
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "anyOf": [
///    {
///      "$ref": "#/definitions/InitializeRequest"
///    },
///    {
///      "$ref": "#/definitions/PingRequest"
///    },
///    {
///      "$ref": "#/definitions/ListResourcesRequest"
///    },
///    {
///      "$ref": "#/definitions/ListResourceTemplatesRequest"
///    },
///    {
///      "$ref": "#/definitions/ReadResourceRequest"
///    },
///    {
///      "$ref": "#/definitions/SubscribeRequest"
///    },
///    {
///      "$ref": "#/definitions/UnsubscribeRequest"
///    },
///    {
///      "$ref": "#/definitions/ListPromptsRequest"
///    },
///    {
///      "$ref": "#/definitions/GetPromptRequest"
///    },
///    {
///      "$ref": "#/definitions/ListToolsRequest"
///    },
///    {
///      "$ref": "#/definitions/CallToolRequest"
///    },
///    {
///      "$ref": "#/definitions/SetLevelRequest"
///    },
///    {
///      "$ref": "#/definitions/CompleteRequest"
///    }
///  ]
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum ClientRequest {
    InitializeRequest(InitializeRequest),
    PingRequest(PingRequest),
    ListResourcesRequest(ListResourcesRequest),
    ListResourceTemplatesRequest(ListResourceTemplatesRequest),
    ReadResourceRequest(ReadResourceRequest),
    SubscribeRequest(SubscribeRequest),
    UnsubscribeRequest(UnsubscribeRequest),
    ListPromptsRequest(ListPromptsRequest),
    GetPromptRequest(GetPromptRequest),
    ListToolsRequest(ListToolsRequest),
    CallToolRequest(CallToolRequest),
    SetLevelRequest(SetLevelRequest),
    CompleteRequest(CompleteRequest),
}
impl ::std::convert::From<&Self> for ClientRequest {
    fn from(value: &ClientRequest) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<InitializeRequest> for ClientRequest {
    fn from(value: InitializeRequest) -> Self {
        Self::InitializeRequest(value)
    }
}
impl ::std::convert::From<PingRequest> for ClientRequest {
    fn from(value: PingRequest) -> Self {
        Self::PingRequest(value)
    }
}
impl ::std::convert::From<ListResourcesRequest> for ClientRequest {
    fn from(value: ListResourcesRequest) -> Self {
        Self::ListResourcesRequest(value)
    }
}
impl ::std::convert::From<ListResourceTemplatesRequest> for ClientRequest {
    fn from(value: ListResourceTemplatesRequest) -> Self {
        Self::ListResourceTemplatesRequest(value)
    }
}
impl ::std::convert::From<ReadResourceRequest> for ClientRequest {
    fn from(value: ReadResourceRequest) -> Self {
        Self::ReadResourceRequest(value)
    }
}
impl ::std::convert::From<SubscribeRequest> for ClientRequest {
    fn from(value: SubscribeRequest) -> Self {
        Self::SubscribeRequest(value)
    }
}
impl ::std::convert::From<UnsubscribeRequest> for ClientRequest {
    fn from(value: UnsubscribeRequest) -> Self {
        Self::UnsubscribeRequest(value)
    }
}
impl ::std::convert::From<ListPromptsRequest> for ClientRequest {
    fn from(value: ListPromptsRequest) -> Self {
        Self::ListPromptsRequest(value)
    }
}
impl ::std::convert::From<GetPromptRequest> for ClientRequest {
    fn from(value: GetPromptRequest) -> Self {
        Self::GetPromptRequest(value)
    }
}
impl ::std::convert::From<ListToolsRequest> for ClientRequest {
    fn from(value: ListToolsRequest) -> Self {
        Self::ListToolsRequest(value)
    }
}
impl ::std::convert::From<CallToolRequest> for ClientRequest {
    fn from(value: CallToolRequest) -> Self {
        Self::CallToolRequest(value)
    }
}
impl ::std::convert::From<SetLevelRequest> for ClientRequest {
    fn from(value: SetLevelRequest) -> Self {
        Self::SetLevelRequest(value)
    }
}
impl ::std::convert::From<CompleteRequest> for ClientRequest {
    fn from(value: CompleteRequest) -> Self {
        Self::CompleteRequest(value)
    }
}
///ClientResult
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "anyOf": [
///    {
///      "$ref": "#/definitions/Result"
///    },
///    {
///      "$ref": "#/definitions/CreateMessageResult"
///    },
///    {
///      "$ref": "#/definitions/ListRootsResult"
///    }
///  ]
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum ClientResult {
    Result(Result),
    CreateMessageResult(CreateMessageResult),
    ListRootsResult(ListRootsResult),
}
impl ::std::convert::From<&Self> for ClientResult {
    fn from(value: &ClientResult) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<Result> for ClientResult {
    fn from(value: Result) -> Self {
        Self::Result(value)
    }
}
impl ::std::convert::From<CreateMessageResult> for ClientResult {
    fn from(value: CreateMessageResult) -> Self {
        Self::CreateMessageResult(value)
    }
}
impl ::std::convert::From<ListRootsResult> for ClientResult {
    fn from(value: ListRootsResult) -> Self {
        Self::ListRootsResult(value)
    }
}
///A request from the client to the server, to ask for completion options.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A request from the client to the server, to ask for completion options.",
///  "type": "object",
///  "required": [
///    "method",
///    "params"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "completion/complete"
///    },
///    "params": {
///      "type": "object",
///      "required": [
///        "argument",
///        "ref"
///      ],
///      "properties": {
///        "argument": {
///          "description": "The argument's information",
///          "type": "object",
///          "required": [
///            "name",
///            "value"
///          ],
///          "properties": {
///            "name": {
///              "description": "The name of the argument",
///              "type": "string"
///            },
///            "value": {
///              "description": "The value of the argument to use for completion matching.",
///              "type": "string"
///            }
///          }
///        },
///        "ref": {
///          "anyOf": [
///            {
///              "$ref": "#/definitions/PromptReference"
///            },
///            {
///              "$ref": "#/definitions/ResourceReference"
///            }
///          ]
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct CompleteRequest {
    pub method: ::std::string::String,
    pub params: CompleteRequestParams,
}
impl ::std::convert::From<&CompleteRequest> for CompleteRequest {
    fn from(value: &CompleteRequest) -> Self {
        value.clone()
    }
}
///CompleteRequestParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "argument",
///    "ref"
///  ],
///  "properties": {
///    "argument": {
///      "description": "The argument's information",
///      "type": "object",
///      "required": [
///        "name",
///        "value"
///      ],
///      "properties": {
///        "name": {
///          "description": "The name of the argument",
///          "type": "string"
///        },
///        "value": {
///          "description": "The value of the argument to use for completion matching.",
///          "type": "string"
///        }
///      }
///    },
///    "ref": {
///      "anyOf": [
///        {
///          "$ref": "#/definitions/PromptReference"
///        },
///        {
///          "$ref": "#/definitions/ResourceReference"
///        }
///      ]
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct CompleteRequestParams {
    pub argument: CompleteRequestParamsArgument,
    #[serde(rename = "ref")]
    pub ref_: CompleteRequestParamsRef,
}
impl ::std::convert::From<&CompleteRequestParams> for CompleteRequestParams {
    fn from(value: &CompleteRequestParams) -> Self {
        value.clone()
    }
}
///The argument's information
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The argument's information",
///  "type": "object",
///  "required": [
///    "name",
///    "value"
///  ],
///  "properties": {
///    "name": {
///      "description": "The name of the argument",
///      "type": "string"
///    },
///    "value": {
///      "description": "The value of the argument to use for completion matching.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct CompleteRequestParamsArgument {
    ///The name of the argument
    pub name: ::std::string::String,
    ///The value of the argument to use for completion matching.
    pub value: ::std::string::String,
}
impl ::std::convert::From<&CompleteRequestParamsArgument>
for CompleteRequestParamsArgument {
    fn from(value: &CompleteRequestParamsArgument) -> Self {
        value.clone()
    }
}
///CompleteRequestParamsRef
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "anyOf": [
///    {
///      "$ref": "#/definitions/PromptReference"
///    },
///    {
///      "$ref": "#/definitions/ResourceReference"
///    }
///  ]
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum CompleteRequestParamsRef {
    PromptReference(PromptReference),
    ResourceReference(ResourceReference),
}
impl ::std::convert::From<&Self> for CompleteRequestParamsRef {
    fn from(value: &CompleteRequestParamsRef) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<PromptReference> for CompleteRequestParamsRef {
    fn from(value: PromptReference) -> Self {
        Self::PromptReference(value)
    }
}
impl ::std::convert::From<ResourceReference> for CompleteRequestParamsRef {
    fn from(value: ResourceReference) -> Self {
        Self::ResourceReference(value)
    }
}
///The server's response to a completion/complete request
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The server's response to a completion/complete request",
///  "type": "object",
///  "required": [
///    "completion"
///  ],
///  "properties": {
///    "_meta": {
///      "description": "This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.",
///      "type": "object",
///      "additionalProperties": {}
///    },
///    "completion": {
///      "type": "object",
///      "required": [
///        "values"
///      ],
///      "properties": {
///        "hasMore": {
///          "description": "Indicates whether there are additional completion options beyond those provided in the current response, even if the exact total is unknown.",
///          "type": "boolean"
///        },
///        "total": {
///          "description": "The total number of completion options available. This can exceed the number of values actually sent in the response.",
///          "type": "integer"
///        },
///        "values": {
///          "description": "An array of completion values. Must not exceed 100 items.",
///          "type": "array",
///          "items": {
///            "type": "string"
///          }
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct CompleteResult {
    pub completion: CompleteResultCompletion,
    ///This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
}
impl ::std::convert::From<&CompleteResult> for CompleteResult {
    fn from(value: &CompleteResult) -> Self {
        value.clone()
    }
}
///CompleteResultCompletion
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "values"
///  ],
///  "properties": {
///    "hasMore": {
///      "description": "Indicates whether there are additional completion options beyond those provided in the current response, even if the exact total is unknown.",
///      "type": "boolean"
///    },
///    "total": {
///      "description": "The total number of completion options available. This can exceed the number of values actually sent in the response.",
///      "type": "integer"
///    },
///    "values": {
///      "description": "An array of completion values. Must not exceed 100 items.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct CompleteResultCompletion {
    ///Indicates whether there are additional completion options beyond those provided in the current response, even if the exact total is unknown.
    #[serde(
        rename = "hasMore",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub has_more: ::std::option::Option<bool>,
    ///The total number of completion options available. This can exceed the number of values actually sent in the response.
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub total: ::std::option::Option<i64>,
    ///An array of completion values. Must not exceed 100 items.
    pub values: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&CompleteResultCompletion> for CompleteResultCompletion {
    fn from(value: &CompleteResultCompletion) -> Self {
        value.clone()
    }
}
///A request from the server to sample an LLM via the client. The client has full discretion over which model to select. The client should also inform the user before beginning sampling, to allow them to inspect the request (human in the loop) and decide whether to approve it.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A request from the server to sample an LLM via the client. The client has full discretion over which model to select. The client should also inform the user before beginning sampling, to allow them to inspect the request (human in the loop) and decide whether to approve it.",
///  "type": "object",
///  "required": [
///    "method",
///    "params"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "sampling/createMessage"
///    },
///    "params": {
///      "type": "object",
///      "required": [
///        "maxTokens",
///        "messages"
///      ],
///      "properties": {
///        "includeContext": {
///          "description": "A request to include context from one or more MCP servers (including the caller), to be attached to the prompt. The client MAY ignore this request.",
///          "type": "string",
///          "enum": [
///            "allServers",
///            "none",
///            "thisServer"
///          ]
///        },
///        "maxTokens": {
///          "description": "The maximum number of tokens to sample, as requested by the server. The client MAY choose to sample fewer tokens than requested.",
///          "type": "integer"
///        },
///        "messages": {
///          "type": "array",
///          "items": {
///            "$ref": "#/definitions/SamplingMessage"
///          }
///        },
///        "metadata": {
///          "description": "Optional metadata to pass through to the LLM provider. The format of this metadata is provider-specific.",
///          "type": "object",
///          "additionalProperties": true
///        },
///        "modelPreferences": {
///          "description": "The server's preferences for which model to select. The client MAY ignore these preferences.",
///          "$ref": "#/definitions/ModelPreferences"
///        },
///        "stopSequences": {
///          "type": "array",
///          "items": {
///            "type": "string"
///          }
///        },
///        "systemPrompt": {
///          "description": "An optional system prompt the server wants to use for sampling. The client MAY modify or omit this prompt.",
///          "type": "string"
///        },
///        "temperature": {
///          "type": "number"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct CreateMessageRequest {
    pub method: ::std::string::String,
    pub params: CreateMessageRequestParams,
}
impl ::std::convert::From<&CreateMessageRequest> for CreateMessageRequest {
    fn from(value: &CreateMessageRequest) -> Self {
        value.clone()
    }
}
///CreateMessageRequestParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "maxTokens",
///    "messages"
///  ],
///  "properties": {
///    "includeContext": {
///      "description": "A request to include context from one or more MCP servers (including the caller), to be attached to the prompt. The client MAY ignore this request.",
///      "type": "string",
///      "enum": [
///        "allServers",
///        "none",
///        "thisServer"
///      ]
///    },
///    "maxTokens": {
///      "description": "The maximum number of tokens to sample, as requested by the server. The client MAY choose to sample fewer tokens than requested.",
///      "type": "integer"
///    },
///    "messages": {
///      "type": "array",
///      "items": {
///        "$ref": "#/definitions/SamplingMessage"
///      }
///    },
///    "metadata": {
///      "description": "Optional metadata to pass through to the LLM provider. The format of this metadata is provider-specific.",
///      "type": "object",
///      "additionalProperties": true
///    },
///    "modelPreferences": {
///      "description": "The server's preferences for which model to select. The client MAY ignore these preferences.",
///      "$ref": "#/definitions/ModelPreferences"
///    },
///    "stopSequences": {
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "systemPrompt": {
///      "description": "An optional system prompt the server wants to use for sampling. The client MAY modify or omit this prompt.",
///      "type": "string"
///    },
///    "temperature": {
///      "type": "number"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct CreateMessageRequestParams {
    ///A request to include context from one or more MCP servers (including the caller), to be attached to the prompt. The client MAY ignore this request.
    #[serde(
        rename = "includeContext",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub include_context: ::std::option::Option<CreateMessageRequestParamsIncludeContext>,
    ///The maximum number of tokens to sample, as requested by the server. The client MAY choose to sample fewer tokens than requested.
    #[serde(rename = "maxTokens")]
    pub max_tokens: i64,
    pub messages: ::std::vec::Vec<SamplingMessage>,
    ///Optional metadata to pass through to the LLM provider. The format of this metadata is provider-specific.
    #[serde(default, skip_serializing_if = "::serde_json::Map::is_empty")]
    pub metadata: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    ///The server's preferences for which model to select. The client MAY ignore these preferences.
    #[serde(
        rename = "modelPreferences",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub model_preferences: ::std::option::Option<ModelPreferences>,
    #[serde(
        rename = "stopSequences",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty"
    )]
    pub stop_sequences: ::std::vec::Vec<::std::string::String>,
    ///An optional system prompt the server wants to use for sampling. The client MAY modify or omit this prompt.
    #[serde(
        rename = "systemPrompt",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub system_prompt: ::std::option::Option<::std::string::String>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub temperature: ::std::option::Option<f64>,
}
impl ::std::convert::From<&CreateMessageRequestParams> for CreateMessageRequestParams {
    fn from(value: &CreateMessageRequestParams) -> Self {
        value.clone()
    }
}
///A request to include context from one or more MCP servers (including the caller), to be attached to the prompt. The client MAY ignore this request.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A request to include context from one or more MCP servers (including the caller), to be attached to the prompt. The client MAY ignore this request.",
///  "type": "string",
///  "enum": [
///    "allServers",
///    "none",
///    "thisServer"
///  ]
///}
/// ```
/// </details>
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd
)]
pub enum CreateMessageRequestParamsIncludeContext {
    #[serde(rename = "allServers")]
    AllServers,
    #[serde(rename = "none")]
    None,
    #[serde(rename = "thisServer")]
    ThisServer,
}
impl ::std::convert::From<&Self> for CreateMessageRequestParamsIncludeContext {
    fn from(value: &CreateMessageRequestParamsIncludeContext) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for CreateMessageRequestParamsIncludeContext {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::AllServers => write!(f, "allServers"),
            Self::None => write!(f, "none"),
            Self::ThisServer => write!(f, "thisServer"),
        }
    }
}
impl ::std::str::FromStr for CreateMessageRequestParamsIncludeContext {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "allServers" => Ok(Self::AllServers),
            "none" => Ok(Self::None),
            "thisServer" => Ok(Self::ThisServer),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for CreateMessageRequestParamsIncludeContext {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
for CreateMessageRequestParamsIncludeContext {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
for CreateMessageRequestParamsIncludeContext {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The client's response to a sampling/create_message request from the server. The client should inform the user before returning the sampled message, to allow them to inspect the response (human in the loop) and decide whether to allow the server to see it.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The client's response to a sampling/create_message request from the server. The client should inform the user before returning the sampled message, to allow them to inspect the response (human in the loop) and decide whether to allow the server to see it.",
///  "type": "object",
///  "required": [
///    "content",
///    "model",
///    "role"
///  ],
///  "properties": {
///    "_meta": {
///      "description": "This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.",
///      "type": "object",
///      "additionalProperties": {}
///    },
///    "content": {
///      "anyOf": [
///        {
///          "$ref": "#/definitions/TextContent"
///        },
///        {
///          "$ref": "#/definitions/ImageContent"
///        }
///      ]
///    },
///    "model": {
///      "description": "The name of the model that generated the message.",
///      "type": "string"
///    },
///    "role": {
///      "$ref": "#/definitions/Role"
///    },
///    "stopReason": {
///      "description": "The reason why sampling stopped, if known.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct CreateMessageResult {
    pub content: CreateMessageResultContent,
    ///This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    ///The name of the model that generated the message.
    pub model: ::std::string::String,
    pub role: Role,
    ///The reason why sampling stopped, if known.
    #[serde(
        rename = "stopReason",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub stop_reason: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&CreateMessageResult> for CreateMessageResult {
    fn from(value: &CreateMessageResult) -> Self {
        value.clone()
    }
}
///CreateMessageResultContent
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "anyOf": [
///    {
///      "$ref": "#/definitions/TextContent"
///    },
///    {
///      "$ref": "#/definitions/ImageContent"
///    }
///  ]
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum CreateMessageResultContent {
    TextContent(TextContent),
    ImageContent(ImageContent),
}
impl ::std::convert::From<&Self> for CreateMessageResultContent {
    fn from(value: &CreateMessageResultContent) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<TextContent> for CreateMessageResultContent {
    fn from(value: TextContent) -> Self {
        Self::TextContent(value)
    }
}
impl ::std::convert::From<ImageContent> for CreateMessageResultContent {
    fn from(value: ImageContent) -> Self {
        Self::ImageContent(value)
    }
}
///An opaque token used to represent a cursor for pagination.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An opaque token used to represent a cursor for pagination.",
///  "type": "string"
///}
/// ```
/// </details>
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd
)]
#[serde(transparent)]
pub struct Cursor(pub ::std::string::String);
impl ::std::ops::Deref for Cursor {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<Cursor> for ::std::string::String {
    fn from(value: Cursor) -> Self {
        value.0
    }
}
impl ::std::convert::From<&Cursor> for Cursor {
    fn from(value: &Cursor) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<::std::string::String> for Cursor {
    fn from(value: ::std::string::String) -> Self {
        Self(value)
    }
}
impl ::std::str::FromStr for Cursor {
    type Err = ::std::convert::Infallible;
    fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
        Ok(Self(value.to_string()))
    }
}
impl ::std::fmt::Display for Cursor {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        self.0.fmt(f)
    }
}
/**The contents of a resource, embedded into a prompt or tool call result.

It is up to the client how best to render embedded resources for the benefit
of the LLM and/or the user.*/
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The contents of a resource, embedded into a prompt or tool call result.\n\nIt is up to the client how best to render embedded resources for the benefit\nof the LLM and/or the user.",
///  "type": "object",
///  "required": [
///    "resource",
///    "type"
///  ],
///  "properties": {
///    "annotations": {
///      "type": "object",
///      "properties": {
///        "audience": {
///          "description": "Describes who the intended customer of this object or data is.\n\nIt can include multiple entries to indicate content useful for multiple audiences (e.g., `[\"user\", \"assistant\"]`).",
///          "type": "array",
///          "items": {
///            "$ref": "#/definitions/Role"
///          }
///        },
///        "priority": {
///          "description": "Describes how important this data is for operating the server.\n\nA value of 1 means \"most important,\" and indicates that the data is\neffectively required, while 0 means \"least important,\" and indicates that\nthe data is entirely optional.",
///          "type": "number",
///          "maximum": 1.0,
///          "minimum": 0.0
///        }
///      }
///    },
///    "resource": {
///      "anyOf": [
///        {
///          "$ref": "#/definitions/TextResourceContents"
///        },
///        {
///          "$ref": "#/definitions/BlobResourceContents"
///        }
///      ]
///    },
///    "type": {
///      "type": "string",
///      "const": "resource"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct EmbeddedResource {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub annotations: ::std::option::Option<EmbeddedResourceAnnotations>,
    pub resource: EmbeddedResourceResource,
    #[serde(rename = "type")]
    pub type_: ::std::string::String,
}
impl ::std::convert::From<&EmbeddedResource> for EmbeddedResource {
    fn from(value: &EmbeddedResource) -> Self {
        value.clone()
    }
}
///EmbeddedResourceAnnotations
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "audience": {
///      "description": "Describes who the intended customer of this object or data is.\n\nIt can include multiple entries to indicate content useful for multiple audiences (e.g., `[\"user\", \"assistant\"]`).",
///      "type": "array",
///      "items": {
///        "$ref": "#/definitions/Role"
///      }
///    },
///    "priority": {
///      "description": "Describes how important this data is for operating the server.\n\nA value of 1 means \"most important,\" and indicates that the data is\neffectively required, while 0 means \"least important,\" and indicates that\nthe data is entirely optional.",
///      "type": "number",
///      "maximum": 1.0,
///      "minimum": 0.0
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct EmbeddedResourceAnnotations {
    /**Describes who the intended customer of this object or data is.

It can include multiple entries to indicate content useful for multiple audiences (e.g., `["user", "assistant"]`).*/
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub audience: ::std::vec::Vec<Role>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub priority: ::std::option::Option<f64>,
}
impl ::std::convert::From<&EmbeddedResourceAnnotations> for EmbeddedResourceAnnotations {
    fn from(value: &EmbeddedResourceAnnotations) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for EmbeddedResourceAnnotations {
    fn default() -> Self {
        Self {
            audience: Default::default(),
            priority: Default::default(),
        }
    }
}
///EmbeddedResourceResource
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "anyOf": [
///    {
///      "$ref": "#/definitions/TextResourceContents"
///    },
///    {
///      "$ref": "#/definitions/BlobResourceContents"
///    }
///  ]
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum EmbeddedResourceResource {
    TextResourceContents(TextResourceContents),
    BlobResourceContents(BlobResourceContents),
}
impl ::std::convert::From<&Self> for EmbeddedResourceResource {
    fn from(value: &EmbeddedResourceResource) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<TextResourceContents> for EmbeddedResourceResource {
    fn from(value: TextResourceContents) -> Self {
        Self::TextResourceContents(value)
    }
}
impl ::std::convert::From<BlobResourceContents> for EmbeddedResourceResource {
    fn from(value: BlobResourceContents) -> Self {
        Self::BlobResourceContents(value)
    }
}
///EmptyResult
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "$ref": "#/definitions/Result"
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
#[serde(transparent)]
pub struct EmptyResult(pub Result);
impl ::std::ops::Deref for EmptyResult {
    type Target = Result;
    fn deref(&self) -> &Result {
        &self.0
    }
}
impl ::std::convert::From<EmptyResult> for Result {
    fn from(value: EmptyResult) -> Self {
        value.0
    }
}
impl ::std::convert::From<&EmptyResult> for EmptyResult {
    fn from(value: &EmptyResult) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<Result> for EmptyResult {
    fn from(value: Result) -> Self {
        Self(value)
    }
}
///Used by the client to get a prompt provided by the server.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Used by the client to get a prompt provided by the server.",
///  "type": "object",
///  "required": [
///    "method",
///    "params"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "prompts/get"
///    },
///    "params": {
///      "type": "object",
///      "required": [
///        "name"
///      ],
///      "properties": {
///        "arguments": {
///          "description": "Arguments to use for templating the prompt.",
///          "type": "object",
///          "additionalProperties": {
///            "type": "string"
///          }
///        },
///        "name": {
///          "description": "The name of the prompt or prompt template.",
///          "type": "string"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct GetPromptRequest {
    pub method: ::std::string::String,
    pub params: GetPromptRequestParams,
}
impl ::std::convert::From<&GetPromptRequest> for GetPromptRequest {
    fn from(value: &GetPromptRequest) -> Self {
        value.clone()
    }
}
///GetPromptRequestParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "name"
///  ],
///  "properties": {
///    "arguments": {
///      "description": "Arguments to use for templating the prompt.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "name": {
///      "description": "The name of the prompt or prompt template.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct GetPromptRequestParams {
    ///Arguments to use for templating the prompt.
    #[serde(default, skip_serializing_if = ":: std :: collections :: HashMap::is_empty")]
    pub arguments: ::std::collections::HashMap<
        ::std::string::String,
        ::std::string::String,
    >,
    ///The name of the prompt or prompt template.
    pub name: ::std::string::String,
}
impl ::std::convert::From<&GetPromptRequestParams> for GetPromptRequestParams {
    fn from(value: &GetPromptRequestParams) -> Self {
        value.clone()
    }
}
///The server's response to a prompts/get request from the client.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The server's response to a prompts/get request from the client.",
///  "type": "object",
///  "required": [
///    "messages"
///  ],
///  "properties": {
///    "_meta": {
///      "description": "This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.",
///      "type": "object",
///      "additionalProperties": {}
///    },
///    "description": {
///      "description": "An optional description for the prompt.",
///      "type": "string"
///    },
///    "messages": {
///      "type": "array",
///      "items": {
///        "$ref": "#/definitions/PromptMessage"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct GetPromptResult {
    ///An optional description for the prompt.
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub description: ::std::option::Option<::std::string::String>,
    pub messages: ::std::vec::Vec<PromptMessage>,
    ///This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
}
impl ::std::convert::From<&GetPromptResult> for GetPromptResult {
    fn from(value: &GetPromptResult) -> Self {
        value.clone()
    }
}
///An image provided to or from an LLM.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An image provided to or from an LLM.",
///  "type": "object",
///  "required": [
///    "data",
///    "mimeType",
///    "type"
///  ],
///  "properties": {
///    "annotations": {
///      "type": "object",
///      "properties": {
///        "audience": {
///          "description": "Describes who the intended customer of this object or data is.\n\nIt can include multiple entries to indicate content useful for multiple audiences (e.g., `[\"user\", \"assistant\"]`).",
///          "type": "array",
///          "items": {
///            "$ref": "#/definitions/Role"
///          }
///        },
///        "priority": {
///          "description": "Describes how important this data is for operating the server.\n\nA value of 1 means \"most important,\" and indicates that the data is\neffectively required, while 0 means \"least important,\" and indicates that\nthe data is entirely optional.",
///          "type": "number",
///          "maximum": 1.0,
///          "minimum": 0.0
///        }
///      }
///    },
///    "data": {
///      "description": "The base64-encoded image data.",
///      "type": "string",
///      "format": "byte"
///    },
///    "mimeType": {
///      "description": "The MIME type of the image. Different providers may support different image types.",
///      "type": "string"
///    },
///    "type": {
///      "type": "string",
///      "const": "image"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ImageContent {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub annotations: ::std::option::Option<ImageContentAnnotations>,
    ///The base64-encoded image data.
    pub data: ::std::string::String,
    ///The MIME type of the image. Different providers may support different image types.
    #[serde(rename = "mimeType")]
    pub mime_type: ::std::string::String,
    #[serde(rename = "type")]
    pub type_: ::std::string::String,
}
impl ::std::convert::From<&ImageContent> for ImageContent {
    fn from(value: &ImageContent) -> Self {
        value.clone()
    }
}
///ImageContentAnnotations
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "audience": {
///      "description": "Describes who the intended customer of this object or data is.\n\nIt can include multiple entries to indicate content useful for multiple audiences (e.g., `[\"user\", \"assistant\"]`).",
///      "type": "array",
///      "items": {
///        "$ref": "#/definitions/Role"
///      }
///    },
///    "priority": {
///      "description": "Describes how important this data is for operating the server.\n\nA value of 1 means \"most important,\" and indicates that the data is\neffectively required, while 0 means \"least important,\" and indicates that\nthe data is entirely optional.",
///      "type": "number",
///      "maximum": 1.0,
///      "minimum": 0.0
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ImageContentAnnotations {
    /**Describes who the intended customer of this object or data is.

It can include multiple entries to indicate content useful for multiple audiences (e.g., `["user", "assistant"]`).*/
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub audience: ::std::vec::Vec<Role>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub priority: ::std::option::Option<f64>,
}
impl ::std::convert::From<&ImageContentAnnotations> for ImageContentAnnotations {
    fn from(value: &ImageContentAnnotations) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ImageContentAnnotations {
    fn default() -> Self {
        Self {
            audience: Default::default(),
            priority: Default::default(),
        }
    }
}
///Describes the name and version of an MCP implementation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Describes the name and version of an MCP implementation.",
///  "type": "object",
///  "required": [
///    "name",
///    "version"
///  ],
///  "properties": {
///    "name": {
///      "type": "string"
///    },
///    "version": {
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct Implementation {
    pub name: ::std::string::String,
    pub version: ::std::string::String,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub title: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Implementation> for Implementation {
    fn from(value: &Implementation) -> Self {
        value.clone()
    }
}
///This request is sent from the client to the server when it first connects, asking it to begin initialization.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "This request is sent from the client to the server when it first connects, asking it to begin initialization.",
///  "type": "object",
///  "required": [
///    "method",
///    "params"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "initialize"
///    },
///    "params": {
///      "type": "object",
///      "required": [
///        "capabilities",
///        "clientInfo",
///        "protocolVersion"
///      ],
///      "properties": {
///        "capabilities": {
///          "$ref": "#/definitions/ClientCapabilities"
///        },
///        "clientInfo": {
///          "$ref": "#/definitions/Implementation"
///        },
///        "protocolVersion": {
///          "description": "The latest version of the Model Context Protocol that the client supports. The client MAY decide to support older versions as well.",
///          "type": "string"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct InitializeRequest {
    pub method: ::std::string::String,
    pub params: InitializeRequestParams,
}
impl ::std::convert::From<&InitializeRequest> for InitializeRequest {
    fn from(value: &InitializeRequest) -> Self {
        value.clone()
    }
}
///InitializeRequestParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "capabilities",
///    "clientInfo",
///    "protocolVersion"
///  ],
///  "properties": {
///    "capabilities": {
///      "$ref": "#/definitions/ClientCapabilities"
///    },
///    "clientInfo": {
///      "$ref": "#/definitions/Implementation"
///    },
///    "protocolVersion": {
///      "description": "The latest version of the Model Context Protocol that the client supports. The client MAY decide to support older versions as well.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct InitializeRequestParams {
    pub capabilities: ClientCapabilities,
    #[serde(rename = "clientInfo")]
    pub client_info: Implementation,
    ///The latest version of the Model Context Protocol that the client supports. The client MAY decide to support older versions as well.
    #[serde(rename = "protocolVersion")]
    pub protocol_version: ::std::string::String,
}
impl ::std::convert::From<&InitializeRequestParams> for InitializeRequestParams {
    fn from(value: &InitializeRequestParams) -> Self {
        value.clone()
    }
}
///After receiving an initialize request from the client, the server sends this response.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "After receiving an initialize request from the client, the server sends this response.",
///  "type": "object",
///  "required": [
///    "capabilities",
///    "protocolVersion",
///    "serverInfo"
///  ],
///  "properties": {
///    "_meta": {
///      "description": "This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.",
///      "type": "object",
///      "additionalProperties": {}
///    },
///    "capabilities": {
///      "$ref": "#/definitions/ServerCapabilities"
///    },
///    "instructions": {
///      "description": "Instructions describing how to use the server and its features.\n\nThis can be used by clients to improve the LLM's understanding of available tools, resources, etc. It can be thought of like a \"hint\" to the model. For example, this information MAY be added to the system prompt.",
///      "type": "string"
///    },
///    "protocolVersion": {
///      "description": "The version of the Model Context Protocol that the server wants to use. This may not match the version that the client requested. If the client cannot support this version, it MUST disconnect.",
///      "type": "string"
///    },
///    "serverInfo": {
///      "$ref": "#/definitions/Implementation"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct InitializeResult {
    pub capabilities: ServerCapabilities,
    /**Instructions describing how to use the server and its features.

This can be used by clients to improve the LLM's understanding of available tools, resources, etc. It can be thought of like a "hint" to the model. For example, this information MAY be added to the system prompt.*/
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub instructions: ::std::option::Option<::std::string::String>,
    ///This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    ///The version of the Model Context Protocol that the server wants to use. This may not match the version that the client requested. If the client cannot support this version, it MUST disconnect.
    #[serde(rename = "protocolVersion")]
    pub protocol_version: ::std::string::String,
    #[serde(rename = "serverInfo")]
    pub server_info: Implementation,
}
impl ::std::convert::From<&InitializeResult> for InitializeResult {
    fn from(value: &InitializeResult) -> Self {
        value.clone()
    }
}
///This notification is sent from the client to the server after initialization has finished.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "This notification is sent from the client to the server after initialization has finished.",
///  "type": "object",
///  "required": [
///    "method"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "notifications/initialized"
///    },
///    "params": {
///      "type": "object",
///      "properties": {
///        "_meta": {
///          "description": "This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.",
///          "type": "object",
///          "additionalProperties": {}
///        }
///      },
///      "additionalProperties": {}
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct InitializedNotification {
    pub method: ::std::string::String,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub params: ::std::option::Option<InitializedNotificationParams>,
}
impl ::std::convert::From<&InitializedNotification> for InitializedNotification {
    fn from(value: &InitializedNotification) -> Self {
        value.clone()
    }
}
///InitializedNotificationParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "_meta": {
///      "description": "This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.",
///      "type": "object",
///      "additionalProperties": {}
///    }
///  },
///  "additionalProperties": {}
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct InitializedNotificationParams {
    ///This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
}
impl ::std::convert::From<&InitializedNotificationParams>
for InitializedNotificationParams {
    fn from(value: &InitializedNotificationParams) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for InitializedNotificationParams {
    fn default() -> Self {
        Self { meta: Default::default() }
    }
}
///A response to a request that indicates an error occurred.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A response to a request that indicates an error occurred.",
///  "type": "object",
///  "required": [
///    "error",
///    "id",
///    "jsonrpc"
///  ],
///  "properties": {
///    "error": {
///      "type": "object",
///      "required": [
///        "code",
///        "message"
///      ],
///      "properties": {
///        "code": {
///          "description": "The error type that occurred.",
///          "type": "integer"
///        },
///        "data": {
///          "description": "Additional information about the error. The value of this member is defined by the sender (e.g. detailed error information, nested errors etc.)."
///        },
///        "message": {
///          "description": "A short description of the error. The message SHOULD be limited to a concise single sentence.",
///          "type": "string"
///        }
///      }
///    },
///    "id": {
///      "$ref": "#/definitions/RequestId"
///    },
///    "jsonrpc": {
///      "type": "string",
///      "const": "2.0"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct JsonrpcError {
    pub error: JsonrpcErrorError,
    pub id: RequestId,
    pub jsonrpc: ::std::string::String,
}
impl ::std::convert::From<&JsonrpcError> for JsonrpcError {
    fn from(value: &JsonrpcError) -> Self {
        value.clone()
    }
}
///JsonrpcErrorError
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "code",
///    "message"
///  ],
///  "properties": {
///    "code": {
///      "description": "The error type that occurred.",
///      "type": "integer"
///    },
///    "data": {
///      "description": "Additional information about the error. The value of this member is defined by the sender (e.g. detailed error information, nested errors etc.)."
///    },
///    "message": {
///      "description": "A short description of the error. The message SHOULD be limited to a concise single sentence.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct JsonrpcErrorError {
    ///The error type that occurred.
    pub code: i64,
    ///Additional information about the error. The value of this member is defined by the sender (e.g. detailed error information, nested errors etc.).
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub data: ::std::option::Option<::serde_json::Value>,
    ///A short description of the error. The message SHOULD be limited to a concise single sentence.
    pub message: ::std::string::String,
}
impl ::std::convert::From<&JsonrpcErrorError> for JsonrpcErrorError {
    fn from(value: &JsonrpcErrorError) -> Self {
        value.clone()
    }
}
///JsonrpcMessage
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "anyOf": [
///    {
///      "$ref": "#/definitions/JSONRPCRequest"
///    },
///    {
///      "$ref": "#/definitions/JSONRPCNotification"
///    },
///    {
///      "$ref": "#/definitions/JSONRPCResponse"
///    },
///    {
///      "$ref": "#/definitions/JSONRPCError"
///    }
///  ]
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum JsonrpcMessage {
    Request(JsonrpcRequest),
    Notification(JsonrpcNotification),
    Response(JsonrpcResponse),
    Error(JsonrpcError),
}
impl ::std::convert::From<&Self> for JsonrpcMessage {
    fn from(value: &JsonrpcMessage) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<JsonrpcRequest> for JsonrpcMessage {
    fn from(value: JsonrpcRequest) -> Self {
        Self::Request(value)
    }
}
impl ::std::convert::From<JsonrpcNotification> for JsonrpcMessage {
    fn from(value: JsonrpcNotification) -> Self {
        Self::Notification(value)
    }
}
impl ::std::convert::From<JsonrpcResponse> for JsonrpcMessage {
    fn from(value: JsonrpcResponse) -> Self {
        Self::Response(value)
    }
}
impl ::std::convert::From<JsonrpcError> for JsonrpcMessage {
    fn from(value: JsonrpcError) -> Self {
        Self::Error(value)
    }
}
///A notification which does not expect a response.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A notification which does not expect a response.",
///  "type": "object",
///  "required": [
///    "jsonrpc",
///    "method"
///  ],
///  "properties": {
///    "jsonrpc": {
///      "type": "string",
///      "const": "2.0"
///    },
///    "method": {
///      "type": "string"
///    },
///    "params": {
///      "type": "object",
///      "properties": {
///        "_meta": {
///          "description": "This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.",
///          "type": "object",
///          "additionalProperties": {}
///        }
///      },
///      "additionalProperties": {}
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct JsonrpcNotification {
    pub jsonrpc: ::std::string::String,
    pub method: ::std::string::String,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub params: ::std::option::Option<JsonrpcNotificationParams>,
}
impl ::std::convert::From<&JsonrpcNotification> for JsonrpcNotification {
    fn from(value: &JsonrpcNotification) -> Self {
        value.clone()
    }
}
///JsonrpcNotificationParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "_meta": {
///      "description": "This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.",
///      "type": "object",
///      "additionalProperties": {}
///    }
///  },
///  "additionalProperties": {}
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct JsonrpcNotificationParams {
    ///This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
}
impl ::std::convert::From<&JsonrpcNotificationParams> for JsonrpcNotificationParams {
    fn from(value: &JsonrpcNotificationParams) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for JsonrpcNotificationParams {
    fn default() -> Self {
        Self { meta: Default::default() }
    }
}
///A request that expects a response.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A request that expects a response.",
///  "type": "object",
///  "required": [
///    "id",
///    "jsonrpc",
///    "method"
///  ],
///  "properties": {
///    "id": {
///      "$ref": "#/definitions/RequestId"
///    },
///    "jsonrpc": {
///      "type": "string",
///      "const": "2.0"
///    },
///    "method": {
///      "type": "string"
///    },
///    "params": {
///      "type": "object",
///      "properties": {
///        "_meta": {
///          "type": "object",
///          "properties": {
///            "progressToken": {
///              "description": "If specified, the caller is requesting out-of-band progress notifications for this request (as represented by notifications/progress). The value of this parameter is an opaque token that will be attached to any subsequent notifications. The receiver is not obligated to provide these notifications.",
///              "$ref": "#/definitions/ProgressToken"
///            }
///          }
///        }
///      },
///      "additionalProperties": {}
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct JsonrpcRequest {
    pub id: RequestId,
    pub jsonrpc: ::std::string::String,
    pub method: ::std::string::String,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub params: ::std::option::Option<JsonrpcRequestParams>,
}
impl ::std::convert::From<&JsonrpcRequest> for JsonrpcRequest {
    fn from(value: &JsonrpcRequest) -> Self {
        value.clone()
    }
}
///JsonrpcRequestParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "_meta": {
///      "type": "object",
///      "properties": {
///        "progressToken": {
///          "description": "If specified, the caller is requesting out-of-band progress notifications for this request (as represented by notifications/progress). The value of this parameter is an opaque token that will be attached to any subsequent notifications. The receiver is not obligated to provide these notifications.",
///          "$ref": "#/definitions/ProgressToken"
///        }
///      }
///    }
///  },
///  "additionalProperties": {}
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct JsonrpcRequestParams {
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub meta: ::std::option::Option<JsonrpcRequestParamsMeta>,
}
impl ::std::convert::From<&JsonrpcRequestParams> for JsonrpcRequestParams {
    fn from(value: &JsonrpcRequestParams) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for JsonrpcRequestParams {
    fn default() -> Self {
        Self { meta: Default::default() }
    }
}
///JsonrpcRequestParamsMeta
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "progressToken": {
///      "description": "If specified, the caller is requesting out-of-band progress notifications for this request (as represented by notifications/progress). The value of this parameter is an opaque token that will be attached to any subsequent notifications. The receiver is not obligated to provide these notifications.",
///      "$ref": "#/definitions/ProgressToken"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct JsonrpcRequestParamsMeta {
    ///If specified, the caller is requesting out-of-band progress notifications for this request (as represented by notifications/progress). The value of this parameter is an opaque token that will be attached to any subsequent notifications. The receiver is not obligated to provide these notifications.
    #[serde(
        rename = "progressToken",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub progress_token: ::std::option::Option<ProgressToken>,
}
impl ::std::convert::From<&JsonrpcRequestParamsMeta> for JsonrpcRequestParamsMeta {
    fn from(value: &JsonrpcRequestParamsMeta) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for JsonrpcRequestParamsMeta {
    fn default() -> Self {
        Self {
            progress_token: Default::default(),
        }
    }
}
///A successful (non-error) response to a request.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A successful (non-error) response to a request.",
///  "type": "object",
///  "required": [
///    "id",
///    "jsonrpc",
///    "result"
///  ],
///  "properties": {
///    "id": {
///      "$ref": "#/definitions/RequestId"
///    },
///    "jsonrpc": {
///      "type": "string",
///      "const": "2.0"
///    },
///    "result": {
///      "$ref": "#/definitions/Result"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct JsonrpcResponse {
    pub id: RequestId,
    pub jsonrpc: ::std::string::String,
    pub result: Result,
}
impl ::std::convert::From<&JsonrpcResponse> for JsonrpcResponse {
    fn from(value: &JsonrpcResponse) -> Self {
        value.clone()
    }
}
///Sent from the client to request a list of prompts and prompt templates the server has.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Sent from the client to request a list of prompts and prompt templates the server has.",
///  "type": "object",
///  "required": [
///    "method"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "prompts/list"
///    },
///    "params": {
///      "type": "object",
///      "properties": {
///        "cursor": {
///          "description": "An opaque token representing the current pagination position.\nIf provided, the server should return results starting after this cursor.",
///          "type": "string"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ListPromptsRequest {
    pub method: ::std::string::String,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub params: ::std::option::Option<ListPromptsRequestParams>,
}
impl ::std::convert::From<&ListPromptsRequest> for ListPromptsRequest {
    fn from(value: &ListPromptsRequest) -> Self {
        value.clone()
    }
}
///ListPromptsRequestParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "cursor": {
///      "description": "An opaque token representing the current pagination position.\nIf provided, the server should return results starting after this cursor.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ListPromptsRequestParams {
    /**An opaque token representing the current pagination position.
If provided, the server should return results starting after this cursor.*/
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub cursor: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ListPromptsRequestParams> for ListPromptsRequestParams {
    fn from(value: &ListPromptsRequestParams) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ListPromptsRequestParams {
    fn default() -> Self {
        Self { cursor: Default::default() }
    }
}
///The server's response to a prompts/list request from the client.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The server's response to a prompts/list request from the client.",
///  "type": "object",
///  "required": [
///    "prompts"
///  ],
///  "properties": {
///    "_meta": {
///      "description": "This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.",
///      "type": "object",
///      "additionalProperties": {}
///    },
///    "nextCursor": {
///      "description": "An opaque token representing the pagination position after the last returned result.\nIf present, there may be more results available.",
///      "type": "string"
///    },
///    "prompts": {
///      "type": "array",
///      "items": {
///        "$ref": "#/definitions/Prompt"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ListPromptsResult {
    ///This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    /**An opaque token representing the pagination position after the last returned result.
If present, there may be more results available.*/
    #[serde(
        rename = "nextCursor",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub next_cursor: ::std::option::Option<::std::string::String>,
    pub prompts: ::std::vec::Vec<Prompt>,
}
impl ::std::convert::From<&ListPromptsResult> for ListPromptsResult {
    fn from(value: &ListPromptsResult) -> Self {
        value.clone()
    }
}
///Sent from the client to request a list of resource templates the server has.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Sent from the client to request a list of resource templates the server has.",
///  "type": "object",
///  "required": [
///    "method"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "resources/templates/list"
///    },
///    "params": {
///      "type": "object",
///      "properties": {
///        "cursor": {
///          "description": "An opaque token representing the current pagination position.\nIf provided, the server should return results starting after this cursor.",
///          "type": "string"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ListResourceTemplatesRequest {
    pub method: ::std::string::String,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub params: ::std::option::Option<ListResourceTemplatesRequestParams>,
}
impl ::std::convert::From<&ListResourceTemplatesRequest>
for ListResourceTemplatesRequest {
    fn from(value: &ListResourceTemplatesRequest) -> Self {
        value.clone()
    }
}
///ListResourceTemplatesRequestParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "cursor": {
///      "description": "An opaque token representing the current pagination position.\nIf provided, the server should return results starting after this cursor.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ListResourceTemplatesRequestParams {
    /**An opaque token representing the current pagination position.
If provided, the server should return results starting after this cursor.*/
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub cursor: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ListResourceTemplatesRequestParams>
for ListResourceTemplatesRequestParams {
    fn from(value: &ListResourceTemplatesRequestParams) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ListResourceTemplatesRequestParams {
    fn default() -> Self {
        Self { cursor: Default::default() }
    }
}
///The server's response to a resources/templates/list request from the client.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The server's response to a resources/templates/list request from the client.",
///  "type": "object",
///  "required": [
///    "resourceTemplates"
///  ],
///  "properties": {
///    "_meta": {
///      "description": "This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.",
///      "type": "object",
///      "additionalProperties": {}
///    },
///    "nextCursor": {
///      "description": "An opaque token representing the pagination position after the last returned result.\nIf present, there may be more results available.",
///      "type": "string"
///    },
///    "resourceTemplates": {
///      "type": "array",
///      "items": {
///        "$ref": "#/definitions/ResourceTemplate"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ListResourceTemplatesResult {
    ///This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    /**An opaque token representing the pagination position after the last returned result.
If present, there may be more results available.*/
    #[serde(
        rename = "nextCursor",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub next_cursor: ::std::option::Option<::std::string::String>,
    #[serde(rename = "resourceTemplates")]
    pub resource_templates: ::std::vec::Vec<ResourceTemplate>,
}
impl ::std::convert::From<&ListResourceTemplatesResult> for ListResourceTemplatesResult {
    fn from(value: &ListResourceTemplatesResult) -> Self {
        value.clone()
    }
}
///Sent from the client to request a list of resources the server has.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Sent from the client to request a list of resources the server has.",
///  "type": "object",
///  "required": [
///    "method"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "resources/list"
///    },
///    "params": {
///      "type": "object",
///      "properties": {
///        "cursor": {
///          "description": "An opaque token representing the current pagination position.\nIf provided, the server should return results starting after this cursor.",
///          "type": "string"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ListResourcesRequest {
    pub method: ::std::string::String,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub params: ::std::option::Option<ListResourcesRequestParams>,
}
impl ::std::convert::From<&ListResourcesRequest> for ListResourcesRequest {
    fn from(value: &ListResourcesRequest) -> Self {
        value.clone()
    }
}
///ListResourcesRequestParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "cursor": {
///      "description": "An opaque token representing the current pagination position.\nIf provided, the server should return results starting after this cursor.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ListResourcesRequestParams {
    /**An opaque token representing the current pagination position.
If provided, the server should return results starting after this cursor.*/
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub cursor: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ListResourcesRequestParams> for ListResourcesRequestParams {
    fn from(value: &ListResourcesRequestParams) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ListResourcesRequestParams {
    fn default() -> Self {
        Self { cursor: Default::default() }
    }
}
///The server's response to a resources/list request from the client.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The server's response to a resources/list request from the client.",
///  "type": "object",
///  "required": [
///    "resources"
///  ],
///  "properties": {
///    "_meta": {
///      "description": "This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.",
///      "type": "object",
///      "additionalProperties": {}
///    },
///    "nextCursor": {
///      "description": "An opaque token representing the pagination position after the last returned result.\nIf present, there may be more results available.",
///      "type": "string"
///    },
///    "resources": {
///      "type": "array",
///      "items": {
///        "$ref": "#/definitions/Resource"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ListResourcesResult {
    ///This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    /**An opaque token representing the pagination position after the last returned result.
If present, there may be more results available.*/
    #[serde(
        rename = "nextCursor",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub next_cursor: ::std::option::Option<::std::string::String>,
    pub resources: ::std::vec::Vec<Resource>,
}
impl ::std::convert::From<&ListResourcesResult> for ListResourcesResult {
    fn from(value: &ListResourcesResult) -> Self {
        value.clone()
    }
}
/**Sent from the server to request a list of root URIs from the client. Roots allow
servers to ask for specific directories or files to operate on. A common example
for roots is providing a set of repositories or directories a server should operate
on.

This request is typically used when the server needs to understand the file system
structure or access specific locations that the client has permission to read from.*/
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Sent from the server to request a list of root URIs from the client. Roots allow\nservers to ask for specific directories or files to operate on. A common example\nfor roots is providing a set of repositories or directories a server should operate\non.\n\nThis request is typically used when the server needs to understand the file system\nstructure or access specific locations that the client has permission to read from.",
///  "type": "object",
///  "required": [
///    "method"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "roots/list"
///    },
///    "params": {
///      "type": "object",
///      "properties": {
///        "_meta": {
///          "type": "object",
///          "properties": {
///            "progressToken": {
///              "description": "If specified, the caller is requesting out-of-band progress notifications for this request (as represented by notifications/progress). The value of this parameter is an opaque token that will be attached to any subsequent notifications. The receiver is not obligated to provide these notifications.",
///              "$ref": "#/definitions/ProgressToken"
///            }
///          }
///        }
///      },
///      "additionalProperties": {}
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ListRootsRequest {
    pub method: ::std::string::String,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub params: ::std::option::Option<ListRootsRequestParams>,
}
impl ::std::convert::From<&ListRootsRequest> for ListRootsRequest {
    fn from(value: &ListRootsRequest) -> Self {
        value.clone()
    }
}
///ListRootsRequestParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "_meta": {
///      "type": "object",
///      "properties": {
///        "progressToken": {
///          "description": "If specified, the caller is requesting out-of-band progress notifications for this request (as represented by notifications/progress). The value of this parameter is an opaque token that will be attached to any subsequent notifications. The receiver is not obligated to provide these notifications.",
///          "$ref": "#/definitions/ProgressToken"
///        }
///      }
///    }
///  },
///  "additionalProperties": {}
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ListRootsRequestParams {
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub meta: ::std::option::Option<ListRootsRequestParamsMeta>,
}
impl ::std::convert::From<&ListRootsRequestParams> for ListRootsRequestParams {
    fn from(value: &ListRootsRequestParams) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ListRootsRequestParams {
    fn default() -> Self {
        Self { meta: Default::default() }
    }
}
///ListRootsRequestParamsMeta
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "progressToken": {
///      "description": "If specified, the caller is requesting out-of-band progress notifications for this request (as represented by notifications/progress). The value of this parameter is an opaque token that will be attached to any subsequent notifications. The receiver is not obligated to provide these notifications.",
///      "$ref": "#/definitions/ProgressToken"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ListRootsRequestParamsMeta {
    ///If specified, the caller is requesting out-of-band progress notifications for this request (as represented by notifications/progress). The value of this parameter is an opaque token that will be attached to any subsequent notifications. The receiver is not obligated to provide these notifications.
    #[serde(
        rename = "progressToken",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub progress_token: ::std::option::Option<ProgressToken>,
}
impl ::std::convert::From<&ListRootsRequestParamsMeta> for ListRootsRequestParamsMeta {
    fn from(value: &ListRootsRequestParamsMeta) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ListRootsRequestParamsMeta {
    fn default() -> Self {
        Self {
            progress_token: Default::default(),
        }
    }
}
/**The client's response to a roots/list request from the server.
This result contains an array of Root objects, each representing a root directory
or file that the server can operate on.*/
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The client's response to a roots/list request from the server.\nThis result contains an array of Root objects, each representing a root directory\nor file that the server can operate on.",
///  "type": "object",
///  "required": [
///    "roots"
///  ],
///  "properties": {
///    "_meta": {
///      "description": "This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.",
///      "type": "object",
///      "additionalProperties": {}
///    },
///    "roots": {
///      "type": "array",
///      "items": {
///        "$ref": "#/definitions/Root"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ListRootsResult {
    ///This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    pub roots: ::std::vec::Vec<Root>,
}
impl ::std::convert::From<&ListRootsResult> for ListRootsResult {
    fn from(value: &ListRootsResult) -> Self {
        value.clone()
    }
}
///Sent from the client to request a list of tools the server has.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Sent from the client to request a list of tools the server has.",
///  "type": "object",
///  "required": [
///    "method"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "tools/list"
///    },
///    "params": {
///      "type": "object",
///      "properties": {
///        "cursor": {
///          "description": "An opaque token representing the current pagination position.\nIf provided, the server should return results starting after this cursor.",
///          "type": "string"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ListToolsRequest {
    pub method: ::std::string::String,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub params: ::std::option::Option<ListToolsRequestParams>,
}
impl ::std::convert::From<&ListToolsRequest> for ListToolsRequest {
    fn from(value: &ListToolsRequest) -> Self {
        value.clone()
    }
}
///ListToolsRequestParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "cursor": {
///      "description": "An opaque token representing the current pagination position.\nIf provided, the server should return results starting after this cursor.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ListToolsRequestParams {
    /**An opaque token representing the current pagination position.
If provided, the server should return results starting after this cursor.*/
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub cursor: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ListToolsRequestParams> for ListToolsRequestParams {
    fn from(value: &ListToolsRequestParams) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ListToolsRequestParams {
    fn default() -> Self {
        Self { cursor: Default::default() }
    }
}
///The server's response to a tools/list request from the client.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The server's response to a tools/list request from the client.",
///  "type": "object",
///  "required": [
///    "tools"
///  ],
///  "properties": {
///    "_meta": {
///      "description": "This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.",
///      "type": "object",
///      "additionalProperties": {}
///    },
///    "nextCursor": {
///      "description": "An opaque token representing the pagination position after the last returned result.\nIf present, there may be more results available.",
///      "type": "string"
///    },
///    "tools": {
///      "type": "array",
///      "items": {
///        "$ref": "#/definitions/Tool"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ListToolsResult {
    ///This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    /**An opaque token representing the pagination position after the last returned result.
If present, there may be more results available.*/
    #[serde(
        rename = "nextCursor",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub next_cursor: ::std::option::Option<::std::string::String>,
    pub tools: ::std::vec::Vec<Tool>,
}
impl ::std::convert::From<&ListToolsResult> for ListToolsResult {
    fn from(value: &ListToolsResult) -> Self {
        value.clone()
    }
}
/**The severity of a log message.

These map to syslog message severities, as specified in RFC-5424:
https://datatracker.ietf.org/doc/html/rfc5424#section-6.2.1*/
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The severity of a log message.\n\nThese map to syslog message severities, as specified in RFC-5424:\nhttps://datatracker.ietf.org/doc/html/rfc5424#section-6.2.1",
///  "type": "string",
///  "enum": [
///    "alert",
///    "critical",
///    "debug",
///    "emergency",
///    "error",
///    "info",
///    "notice",
///    "warning"
///  ]
///}
/// ```
/// </details>
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd
)]
pub enum LoggingLevel {
    #[serde(rename = "alert")]
    Alert,
    #[serde(rename = "critical")]
    Critical,
    #[serde(rename = "debug")]
    Debug,
    #[serde(rename = "emergency")]
    Emergency,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "notice")]
    Notice,
    #[serde(rename = "warning")]
    Warning,
}
impl ::std::convert::From<&Self> for LoggingLevel {
    fn from(value: &LoggingLevel) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for LoggingLevel {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Alert => write!(f, "alert"),
            Self::Critical => write!(f, "critical"),
            Self::Debug => write!(f, "debug"),
            Self::Emergency => write!(f, "emergency"),
            Self::Error => write!(f, "error"),
            Self::Info => write!(f, "info"),
            Self::Notice => write!(f, "notice"),
            Self::Warning => write!(f, "warning"),
        }
    }
}
impl ::std::str::FromStr for LoggingLevel {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "alert" => Ok(Self::Alert),
            "critical" => Ok(Self::Critical),
            "debug" => Ok(Self::Debug),
            "emergency" => Ok(Self::Emergency),
            "error" => Ok(Self::Error),
            "info" => Ok(Self::Info),
            "notice" => Ok(Self::Notice),
            "warning" => Ok(Self::Warning),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for LoggingLevel {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for LoggingLevel {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for LoggingLevel {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Notification of a log message passed from server to client. If no logging/setLevel request has been sent from the client, the server MAY decide which messages to send automatically.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Notification of a log message passed from server to client. If no logging/setLevel request has been sent from the client, the server MAY decide which messages to send automatically.",
///  "type": "object",
///  "required": [
///    "method",
///    "params"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "notifications/message"
///    },
///    "params": {
///      "type": "object",
///      "required": [
///        "data",
///        "level"
///      ],
///      "properties": {
///        "data": {
///          "description": "The data to be logged, such as a string message or an object. Any JSON serializable type is allowed here."
///        },
///        "level": {
///          "description": "The severity of this log message.",
///          "$ref": "#/definitions/LoggingLevel"
///        },
///        "logger": {
///          "description": "An optional name of the logger issuing this message.",
///          "type": "string"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct LoggingMessageNotification {
    pub method: ::std::string::String,
    pub params: LoggingMessageNotificationParams,
}
impl ::std::convert::From<&LoggingMessageNotification> for LoggingMessageNotification {
    fn from(value: &LoggingMessageNotification) -> Self {
        value.clone()
    }
}
///LoggingMessageNotificationParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "data",
///    "level"
///  ],
///  "properties": {
///    "data": {
///      "description": "The data to be logged, such as a string message or an object. Any JSON serializable type is allowed here."
///    },
///    "level": {
///      "description": "The severity of this log message.",
///      "$ref": "#/definitions/LoggingLevel"
///    },
///    "logger": {
///      "description": "An optional name of the logger issuing this message.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct LoggingMessageNotificationParams {
    ///The data to be logged, such as a string message or an object. Any JSON serializable type is allowed here.
    pub data: ::serde_json::Value,
    ///The severity of this log message.
    pub level: LoggingLevel,
    ///An optional name of the logger issuing this message.
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub logger: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&LoggingMessageNotificationParams>
for LoggingMessageNotificationParams {
    fn from(value: &LoggingMessageNotificationParams) -> Self {
        value.clone()
    }
}
/**Hints to use for model selection.

Keys not declared here are currently left unspecified by the spec and are up
to the client to interpret.*/
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Hints to use for model selection.\n\nKeys not declared here are currently left unspecified by the spec and are up\nto the client to interpret.",
///  "type": "object",
///  "properties": {
///    "name": {
///      "description": "A hint for a model name.\n\nThe client SHOULD treat this as a substring of a model name; for example:\n - `claude-3-5-sonnet` should match `claude-3-5-sonnet-20241022`\n - `sonnet` should match `claude-3-5-sonnet-20241022`, `claude-3-sonnet-20240229`, etc.\n - `claude` should match any Claude model\n\nThe client MAY also map the string to a different provider's model name or a different model family, as long as it fills a similar niche; for example:\n - `gemini-1.5-flash` could match `claude-3-haiku-20240307`",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ModelHint {
    /**A hint for a model name.

The client SHOULD treat this as a substring of a model name; for example:
 - `claude-3-5-sonnet` should match `claude-3-5-sonnet-20241022`
 - `sonnet` should match `claude-3-5-sonnet-20241022`, `claude-3-sonnet-20240229`, etc.
 - `claude` should match any Claude model

The client MAY also map the string to a different provider's model name or a different model family, as long as it fills a similar niche; for example:
 - `gemini-1.5-flash` could match `claude-3-haiku-20240307`*/
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub name: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ModelHint> for ModelHint {
    fn from(value: &ModelHint) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ModelHint {
    fn default() -> Self {
        Self { name: Default::default() }
    }
}
/**The server's preferences for model selection, requested of the client during sampling.

Because LLMs can vary along multiple dimensions, choosing the "best" model is
rarely straightforward.  Different models excel in different areas—some are
faster but less capable, others are more capable but more expensive, and so
on. This interface allows servers to express their priorities across multiple
dimensions to help clients make an appropriate selection for their use case.

These preferences are always advisory. The client MAY ignore them. It is also
up to the client to decide how to interpret these preferences and how to
balance them against other considerations.*/
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The server's preferences for model selection, requested of the client during sampling.\n\nBecause LLMs can vary along multiple dimensions, choosing the \"best\" model is\nrarely straightforward.  Different models excel in different areas—some are\nfaster but less capable, others are more capable but more expensive, and so\non. This interface allows servers to express their priorities across multiple\ndimensions to help clients make an appropriate selection for their use case.\n\nThese preferences are always advisory. The client MAY ignore them. It is also\nup to the client to decide how to interpret these preferences and how to\nbalance them against other considerations.",
///  "type": "object",
///  "properties": {
///    "costPriority": {
///      "description": "How much to prioritize cost when selecting a model. A value of 0 means cost\nis not important, while a value of 1 means cost is the most important\nfactor.",
///      "type": "number",
///      "maximum": 1.0,
///      "minimum": 0.0
///    },
///    "hints": {
///      "description": "Optional hints to use for model selection.\n\nIf multiple hints are specified, the client MUST evaluate them in order\n(such that the first match is taken).\n\nThe client SHOULD prioritize these hints over the numeric priorities, but\nMAY still use the priorities to select from ambiguous matches.",
///      "type": "array",
///      "items": {
///        "$ref": "#/definitions/ModelHint"
///      }
///    },
///    "intelligencePriority": {
///      "description": "How much to prioritize intelligence and capabilities when selecting a\nmodel. A value of 0 means intelligence is not important, while a value of 1\nmeans intelligence is the most important factor.",
///      "type": "number",
///      "maximum": 1.0,
///      "minimum": 0.0
///    },
///    "speedPriority": {
///      "description": "How much to prioritize sampling speed (latency) when selecting a model. A\nvalue of 0 means speed is not important, while a value of 1 means speed is\nthe most important factor.",
///      "type": "number",
///      "maximum": 1.0,
///      "minimum": 0.0
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ModelPreferences {
    #[serde(
        rename = "costPriority",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub cost_priority: ::std::option::Option<f64>,
    /**Optional hints to use for model selection.

If multiple hints are specified, the client MUST evaluate them in order
(such that the first match is taken).

The client SHOULD prioritize these hints over the numeric priorities, but
MAY still use the priorities to select from ambiguous matches.*/
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub hints: ::std::vec::Vec<ModelHint>,
    #[serde(
        rename = "intelligencePriority",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub intelligence_priority: ::std::option::Option<f64>,
    #[serde(
        rename = "speedPriority",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub speed_priority: ::std::option::Option<f64>,
}
impl ::std::convert::From<&ModelPreferences> for ModelPreferences {
    fn from(value: &ModelPreferences) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ModelPreferences {
    fn default() -> Self {
        Self {
            cost_priority: Default::default(),
            hints: Default::default(),
            intelligence_priority: Default::default(),
            speed_priority: Default::default(),
        }
    }
}
///Notification
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "method"
///  ],
///  "properties": {
///    "method": {
///      "type": "string"
///    },
///    "params": {
///      "type": "object",
///      "properties": {
///        "_meta": {
///          "description": "This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.",
///          "type": "object",
///          "additionalProperties": {}
///        }
///      },
///      "additionalProperties": {}
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct Notification {
    pub method: ::std::string::String,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub params: ::std::option::Option<NotificationParams>,
}
impl ::std::convert::From<&Notification> for Notification {
    fn from(value: &Notification) -> Self {
        value.clone()
    }
}
///NotificationParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "_meta": {
///      "description": "This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.",
///      "type": "object",
///      "additionalProperties": {}
///    }
///  },
///  "additionalProperties": {}
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct NotificationParams {
    ///This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
}
impl ::std::convert::From<&NotificationParams> for NotificationParams {
    fn from(value: &NotificationParams) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for NotificationParams {
    fn default() -> Self {
        Self { meta: Default::default() }
    }
}
///PaginatedRequest
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "method"
///  ],
///  "properties": {
///    "method": {
///      "type": "string"
///    },
///    "params": {
///      "type": "object",
///      "properties": {
///        "cursor": {
///          "description": "An opaque token representing the current pagination position.\nIf provided, the server should return results starting after this cursor.",
///          "type": "string"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct PaginatedRequest {
    pub method: ::std::string::String,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub params: ::std::option::Option<PaginatedRequestParams>,
}
impl ::std::convert::From<&PaginatedRequest> for PaginatedRequest {
    fn from(value: &PaginatedRequest) -> Self {
        value.clone()
    }
}
///PaginatedRequestParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "cursor": {
///      "description": "An opaque token representing the current pagination position.\nIf provided, the server should return results starting after this cursor.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct PaginatedRequestParams {
    /**An opaque token representing the current pagination position.
If provided, the server should return results starting after this cursor.*/
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub cursor: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&PaginatedRequestParams> for PaginatedRequestParams {
    fn from(value: &PaginatedRequestParams) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PaginatedRequestParams {
    fn default() -> Self {
        Self { cursor: Default::default() }
    }
}
///PaginatedResult
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "_meta": {
///      "description": "This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.",
///      "type": "object",
///      "additionalProperties": {}
///    },
///    "nextCursor": {
///      "description": "An opaque token representing the pagination position after the last returned result.\nIf present, there may be more results available.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct PaginatedResult {
    ///This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    /**An opaque token representing the pagination position after the last returned result.
If present, there may be more results available.*/
    #[serde(
        rename = "nextCursor",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub next_cursor: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&PaginatedResult> for PaginatedResult {
    fn from(value: &PaginatedResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PaginatedResult {
    fn default() -> Self {
        Self {
            meta: Default::default(),
            next_cursor: Default::default(),
        }
    }
}
///A ping, issued by either the server or the client, to check that the other party is still alive. The receiver must promptly respond, or else may be disconnected.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A ping, issued by either the server or the client, to check that the other party is still alive. The receiver must promptly respond, or else may be disconnected.",
///  "type": "object",
///  "required": [
///    "method"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "ping"
///    },
///    "params": {
///      "type": "object",
///      "properties": {
///        "_meta": {
///          "type": "object",
///          "properties": {
///            "progressToken": {
///              "description": "If specified, the caller is requesting out-of-band progress notifications for this request (as represented by notifications/progress). The value of this parameter is an opaque token that will be attached to any subsequent notifications. The receiver is not obligated to provide these notifications.",
///              "$ref": "#/definitions/ProgressToken"
///            }
///          }
///        }
///      },
///      "additionalProperties": {}
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct PingRequest {
    pub method: ::std::string::String,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub params: ::std::option::Option<PingRequestParams>,
}
impl ::std::convert::From<&PingRequest> for PingRequest {
    fn from(value: &PingRequest) -> Self {
        value.clone()
    }
}
///PingRequestParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "_meta": {
///      "type": "object",
///      "properties": {
///        "progressToken": {
///          "description": "If specified, the caller is requesting out-of-band progress notifications for this request (as represented by notifications/progress). The value of this parameter is an opaque token that will be attached to any subsequent notifications. The receiver is not obligated to provide these notifications.",
///          "$ref": "#/definitions/ProgressToken"
///        }
///      }
///    }
///  },
///  "additionalProperties": {}
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct PingRequestParams {
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub meta: ::std::option::Option<PingRequestParamsMeta>,
}
impl ::std::convert::From<&PingRequestParams> for PingRequestParams {
    fn from(value: &PingRequestParams) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PingRequestParams {
    fn default() -> Self {
        Self { meta: Default::default() }
    }
}
///PingRequestParamsMeta
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "progressToken": {
///      "description": "If specified, the caller is requesting out-of-band progress notifications for this request (as represented by notifications/progress). The value of this parameter is an opaque token that will be attached to any subsequent notifications. The receiver is not obligated to provide these notifications.",
///      "$ref": "#/definitions/ProgressToken"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct PingRequestParamsMeta {
    ///If specified, the caller is requesting out-of-band progress notifications for this request (as represented by notifications/progress). The value of this parameter is an opaque token that will be attached to any subsequent notifications. The receiver is not obligated to provide these notifications.
    #[serde(
        rename = "progressToken",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub progress_token: ::std::option::Option<ProgressToken>,
}
impl ::std::convert::From<&PingRequestParamsMeta> for PingRequestParamsMeta {
    fn from(value: &PingRequestParamsMeta) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PingRequestParamsMeta {
    fn default() -> Self {
        Self {
            progress_token: Default::default(),
        }
    }
}
///An out-of-band notification used to inform the receiver of a progress update for a long-running request.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An out-of-band notification used to inform the receiver of a progress update for a long-running request.",
///  "type": "object",
///  "required": [
///    "method",
///    "params"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "notifications/progress"
///    },
///    "params": {
///      "type": "object",
///      "required": [
///        "progress",
///        "progressToken"
///      ],
///      "properties": {
///        "progress": {
///          "description": "The progress thus far. This should increase every time progress is made, even if the total is unknown.",
///          "type": "number"
///        },
///        "progressToken": {
///          "description": "The progress token which was given in the initial request, used to associate this notification with the request that is proceeding.",
///          "$ref": "#/definitions/ProgressToken"
///        },
///        "total": {
///          "description": "Total number of items to process (or total progress required), if known.",
///          "type": "number"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ProgressNotification {
    pub method: ::std::string::String,
    pub params: ProgressNotificationParams,
}
impl ::std::convert::From<&ProgressNotification> for ProgressNotification {
    fn from(value: &ProgressNotification) -> Self {
        value.clone()
    }
}
///ProgressNotificationParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "progress",
///    "progressToken"
///  ],
///  "properties": {
///    "progress": {
///      "description": "The progress thus far. This should increase every time progress is made, even if the total is unknown.",
///      "type": "number"
///    },
///    "progressToken": {
///      "description": "The progress token which was given in the initial request, used to associate this notification with the request that is proceeding.",
///      "$ref": "#/definitions/ProgressToken"
///    },
///    "total": {
///      "description": "Total number of items to process (or total progress required), if known.",
///      "type": "number"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ProgressNotificationParams {
    pub progress: f64,
    ///The progress token which was given in the initial request, used to associate this notification with the request that is proceeding.
    #[serde(rename = "progressToken")]
    pub progress_token: ProgressToken,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub total: ::std::option::Option<f64>,
}
impl ::std::convert::From<&ProgressNotificationParams> for ProgressNotificationParams {
    fn from(value: &ProgressNotificationParams) -> Self {
        value.clone()
    }
}
///A progress token, used to associate progress notifications with the original request.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A progress token, used to associate progress notifications with the original request.",
///  "type": [
///    "string",
///    "integer"
///  ]
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum ProgressToken {
    String(::std::string::String),
    Integer(i64),
}
impl ::std::convert::From<&Self> for ProgressToken {
    fn from(value: &ProgressToken) -> Self {
        value.clone()
    }
}
impl ::std::str::FromStr for ProgressToken {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        if let Ok(v) = value.parse() {
            Ok(Self::String(v))
        } else if let Ok(v) = value.parse() {
            Ok(Self::Integer(v))
        } else {
            Err("string conversion failed for all variants".into())
        }
    }
}
impl ::std::convert::TryFrom<&str> for ProgressToken {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ProgressToken {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ProgressToken {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::fmt::Display for ProgressToken {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match self {
            Self::String(x) => x.fmt(f),
            Self::Integer(x) => x.fmt(f),
        }
    }
}
impl ::std::convert::From<i64> for ProgressToken {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}
///A prompt or prompt template that the server offers.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A prompt or prompt template that the server offers.",
///  "type": "object",
///  "required": [
///    "name"
///  ],
///  "properties": {
///    "arguments": {
///      "description": "A list of arguments to use for templating the prompt.",
///      "type": "array",
///      "items": {
///        "$ref": "#/definitions/PromptArgument"
///      }
///    },
///    "description": {
///      "description": "An optional description of what this prompt provides",
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the prompt or prompt template.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct Prompt {
    ///A list of arguments to use for templating the prompt.
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub arguments: ::std::vec::Vec<PromptArgument>,
    ///An optional description of what this prompt provides
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub description: ::std::option::Option<::std::string::String>,
    ///The name of the prompt or prompt template.
    pub name: ::std::string::String,
}
impl ::std::convert::From<&Prompt> for Prompt {
    fn from(value: &Prompt) -> Self {
        value.clone()
    }
}
///Describes an argument that a prompt can accept.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Describes an argument that a prompt can accept.",
///  "type": "object",
///  "required": [
///    "name"
///  ],
///  "properties": {
///    "description": {
///      "description": "A human-readable description of the argument.",
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the argument.",
///      "type": "string"
///    },
///    "required": {
///      "description": "Whether this argument must be provided.",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct PromptArgument {
    ///A human-readable description of the argument.
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub description: ::std::option::Option<::std::string::String>,
    ///The name of the argument.
    pub name: ::std::string::String,
    ///Whether this argument must be provided.
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub required: ::std::option::Option<bool>,
}
impl ::std::convert::From<&PromptArgument> for PromptArgument {
    fn from(value: &PromptArgument) -> Self {
        value.clone()
    }
}
///An optional notification from the server to the client, informing it that the list of prompts it offers has changed. This may be issued by servers without any previous subscription from the client.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An optional notification from the server to the client, informing it that the list of prompts it offers has changed. This may be issued by servers without any previous subscription from the client.",
///  "type": "object",
///  "required": [
///    "method"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "notifications/prompts/list_changed"
///    },
///    "params": {
///      "type": "object",
///      "properties": {
///        "_meta": {
///          "description": "This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.",
///          "type": "object",
///          "additionalProperties": {}
///        }
///      },
///      "additionalProperties": {}
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct PromptListChangedNotification {
    pub method: ::std::string::String,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub params: ::std::option::Option<PromptListChangedNotificationParams>,
}
impl ::std::convert::From<&PromptListChangedNotification>
for PromptListChangedNotification {
    fn from(value: &PromptListChangedNotification) -> Self {
        value.clone()
    }
}
///PromptListChangedNotificationParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "_meta": {
///      "description": "This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.",
///      "type": "object",
///      "additionalProperties": {}
///    }
///  },
///  "additionalProperties": {}
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct PromptListChangedNotificationParams {
    ///This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
}
impl ::std::convert::From<&PromptListChangedNotificationParams>
for PromptListChangedNotificationParams {
    fn from(value: &PromptListChangedNotificationParams) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PromptListChangedNotificationParams {
    fn default() -> Self {
        Self { meta: Default::default() }
    }
}
/**Describes a message returned as part of a prompt.

This is similar to `SamplingMessage`, but also supports the embedding of
resources from the MCP server.*/
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Describes a message returned as part of a prompt.\n\nThis is similar to `SamplingMessage`, but also supports the embedding of\nresources from the MCP server.",
///  "type": "object",
///  "required": [
///    "content",
///    "role"
///  ],
///  "properties": {
///    "content": {
///      "anyOf": [
///        {
///          "$ref": "#/definitions/TextContent"
///        },
///        {
///          "$ref": "#/definitions/ImageContent"
///        },
///        {
///          "$ref": "#/definitions/EmbeddedResource"
///        }
///      ]
///    },
///    "role": {
///      "$ref": "#/definitions/Role"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct PromptMessage {
    pub content: PromptMessageContent,
    pub role: Role,
}
impl ::std::convert::From<&PromptMessage> for PromptMessage {
    fn from(value: &PromptMessage) -> Self {
        value.clone()
    }
}
///PromptMessageContent
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "anyOf": [
///    {
///      "$ref": "#/definitions/TextContent"
///    },
///    {
///      "$ref": "#/definitions/ImageContent"
///    },
///    {
///      "$ref": "#/definitions/EmbeddedResource"
///    }
///  ]
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum PromptMessageContent {
    TextContent(TextContent),
    ImageContent(ImageContent),
    EmbeddedResource(EmbeddedResource),
}
impl ::std::convert::From<&Self> for PromptMessageContent {
    fn from(value: &PromptMessageContent) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<TextContent> for PromptMessageContent {
    fn from(value: TextContent) -> Self {
        Self::TextContent(value)
    }
}
impl ::std::convert::From<ImageContent> for PromptMessageContent {
    fn from(value: ImageContent) -> Self {
        Self::ImageContent(value)
    }
}
impl ::std::convert::From<EmbeddedResource> for PromptMessageContent {
    fn from(value: EmbeddedResource) -> Self {
        Self::EmbeddedResource(value)
    }
}
///Identifies a prompt.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Identifies a prompt.",
///  "type": "object",
///  "required": [
///    "name",
///    "type"
///  ],
///  "properties": {
///    "name": {
///      "description": "The name of the prompt or prompt template",
///      "type": "string"
///    },
///    "type": {
///      "type": "string",
///      "const": "ref/prompt"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct PromptReference {
    ///The name of the prompt or prompt template
    pub name: ::std::string::String,
    #[serde(rename = "type")]
    pub type_: ::std::string::String,
}
impl ::std::convert::From<&PromptReference> for PromptReference {
    fn from(value: &PromptReference) -> Self {
        value.clone()
    }
}
///Sent from the client to the server, to read a specific resource URI.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Sent from the client to the server, to read a specific resource URI.",
///  "type": "object",
///  "required": [
///    "method",
///    "params"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "resources/read"
///    },
///    "params": {
///      "type": "object",
///      "required": [
///        "uri"
///      ],
///      "properties": {
///        "uri": {
///          "description": "The URI of the resource to read. The URI can use any protocol; it is up to the server how to interpret it.",
///          "type": "string",
///          "format": "uri"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ReadResourceRequest {
    pub method: ::std::string::String,
    pub params: ReadResourceRequestParams,
}
impl ::std::convert::From<&ReadResourceRequest> for ReadResourceRequest {
    fn from(value: &ReadResourceRequest) -> Self {
        value.clone()
    }
}
///ReadResourceRequestParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "uri"
///  ],
///  "properties": {
///    "uri": {
///      "description": "The URI of the resource to read. The URI can use any protocol; it is up to the server how to interpret it.",
///      "type": "string",
///      "format": "uri"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ReadResourceRequestParams {
    ///The URI of the resource to read. The URI can use any protocol; it is up to the server how to interpret it.
    pub uri: ::std::string::String,
}
impl ::std::convert::From<&ReadResourceRequestParams> for ReadResourceRequestParams {
    fn from(value: &ReadResourceRequestParams) -> Self {
        value.clone()
    }
}
///The server's response to a resources/read request from the client.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The server's response to a resources/read request from the client.",
///  "type": "object",
///  "required": [
///    "contents"
///  ],
///  "properties": {
///    "_meta": {
///      "description": "This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.",
///      "type": "object",
///      "additionalProperties": {}
///    },
///    "contents": {
///      "type": "array",
///      "items": {
///        "anyOf": [
///          {
///            "$ref": "#/definitions/TextResourceContents"
///          },
///          {
///            "$ref": "#/definitions/BlobResourceContents"
///          }
///        ]
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ReadResourceResult {
    pub contents: ::std::vec::Vec<ReadResourceResultContentsItem>,
    ///This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
}
impl ::std::convert::From<&ReadResourceResult> for ReadResourceResult {
    fn from(value: &ReadResourceResult) -> Self {
        value.clone()
    }
}
///ReadResourceResultContentsItem
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "anyOf": [
///    {
///      "$ref": "#/definitions/TextResourceContents"
///    },
///    {
///      "$ref": "#/definitions/BlobResourceContents"
///    }
///  ]
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum ReadResourceResultContentsItem {
    TextResourceContents(TextResourceContents),
    BlobResourceContents(BlobResourceContents),
}
impl ::std::convert::From<&Self> for ReadResourceResultContentsItem {
    fn from(value: &ReadResourceResultContentsItem) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<TextResourceContents> for ReadResourceResultContentsItem {
    fn from(value: TextResourceContents) -> Self {
        Self::TextResourceContents(value)
    }
}
impl ::std::convert::From<BlobResourceContents> for ReadResourceResultContentsItem {
    fn from(value: BlobResourceContents) -> Self {
        Self::BlobResourceContents(value)
    }
}
///Request
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "method"
///  ],
///  "properties": {
///    "method": {
///      "type": "string"
///    },
///    "params": {
///      "type": "object",
///      "properties": {
///        "_meta": {
///          "type": "object",
///          "properties": {
///            "progressToken": {
///              "description": "If specified, the caller is requesting out-of-band progress notifications for this request (as represented by notifications/progress). The value of this parameter is an opaque token that will be attached to any subsequent notifications. The receiver is not obligated to provide these notifications.",
///              "$ref": "#/definitions/ProgressToken"
///            }
///          }
///        }
///      },
///      "additionalProperties": {}
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct Request {
    pub method: ::std::string::String,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub params: ::std::option::Option<RequestParams>,
}
impl ::std::convert::From<&Request> for Request {
    fn from(value: &Request) -> Self {
        value.clone()
    }
}
///A uniquely identifying ID for a request in JSON-RPC.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A uniquely identifying ID for a request in JSON-RPC.",
///  "type": [
///    "string",
///    "integer"
///  ]
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum RequestId {
    String(::std::string::String),
    Integer(i64),
}
impl ::std::convert::From<&Self> for RequestId {
    fn from(value: &RequestId) -> Self {
        value.clone()
    }
}
impl ::std::str::FromStr for RequestId {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        if let Ok(v) = value.parse() {
            Ok(Self::String(v))
        } else if let Ok(v) = value.parse() {
            Ok(Self::Integer(v))
        } else {
            Err("string conversion failed for all variants".into())
        }
    }
}
impl ::std::convert::TryFrom<&str> for RequestId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for RequestId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for RequestId {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match self {
            Self::String(x) => x.fmt(f),
            Self::Integer(x) => x.fmt(f),
        }
    }
}
impl ::std::convert::From<i64> for RequestId {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}
///RequestParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "_meta": {
///      "type": "object",
///      "properties": {
///        "progressToken": {
///          "description": "If specified, the caller is requesting out-of-band progress notifications for this request (as represented by notifications/progress). The value of this parameter is an opaque token that will be attached to any subsequent notifications. The receiver is not obligated to provide these notifications.",
///          "$ref": "#/definitions/ProgressToken"
///        }
///      }
///    }
///  },
///  "additionalProperties": {}
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct RequestParams {
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub meta: ::std::option::Option<RequestParamsMeta>,
}
impl ::std::convert::From<&RequestParams> for RequestParams {
    fn from(value: &RequestParams) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RequestParams {
    fn default() -> Self {
        Self { meta: Default::default() }
    }
}
///RequestParamsMeta
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "progressToken": {
///      "description": "If specified, the caller is requesting out-of-band progress notifications for this request (as represented by notifications/progress). The value of this parameter is an opaque token that will be attached to any subsequent notifications. The receiver is not obligated to provide these notifications.",
///      "$ref": "#/definitions/ProgressToken"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct RequestParamsMeta {
    ///If specified, the caller is requesting out-of-band progress notifications for this request (as represented by notifications/progress). The value of this parameter is an opaque token that will be attached to any subsequent notifications. The receiver is not obligated to provide these notifications.
    #[serde(
        rename = "progressToken",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub progress_token: ::std::option::Option<ProgressToken>,
}
impl ::std::convert::From<&RequestParamsMeta> for RequestParamsMeta {
    fn from(value: &RequestParamsMeta) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RequestParamsMeta {
    fn default() -> Self {
        Self {
            progress_token: Default::default(),
        }
    }
}
///A known resource that the server is capable of reading.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A known resource that the server is capable of reading.",
///  "type": "object",
///  "required": [
///    "name",
///    "uri"
///  ],
///  "properties": {
///    "annotations": {
///      "type": "object",
///      "properties": {
///        "audience": {
///          "description": "Describes who the intended customer of this object or data is.\n\nIt can include multiple entries to indicate content useful for multiple audiences (e.g., `[\"user\", \"assistant\"]`).",
///          "type": "array",
///          "items": {
///            "$ref": "#/definitions/Role"
///          }
///        },
///        "priority": {
///          "description": "Describes how important this data is for operating the server.\n\nA value of 1 means \"most important,\" and indicates that the data is\neffectively required, while 0 means \"least important,\" and indicates that\nthe data is entirely optional.",
///          "type": "number",
///          "maximum": 1.0,
///          "minimum": 0.0
///        }
///      }
///    },
///    "description": {
///      "description": "A description of what this resource represents.\n\nThis can be used by clients to improve the LLM's understanding of available resources. It can be thought of like a \"hint\" to the model.",
///      "type": "string"
///    },
///    "mimeType": {
///      "description": "The MIME type of this resource, if known.",
///      "type": "string"
///    },
///    "name": {
///      "description": "A human-readable name for this resource.\n\nThis can be used by clients to populate UI elements.",
///      "type": "string"
///    },
///    "size": {
///      "description": "The size of the raw resource content, in bytes (i.e., before base64 encoding or any tokenization), if known.\n\nThis can be used by Hosts to display file sizes and estimate context window usage.",
///      "type": "integer"
///    },
///    "uri": {
///      "description": "The URI of this resource.",
///      "type": "string",
///      "format": "uri"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct Resource {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub annotations: ::std::option::Option<ResourceAnnotations>,
    /**A description of what this resource represents.

This can be used by clients to improve the LLM's understanding of available resources. It can be thought of like a "hint" to the model.*/
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub description: ::std::option::Option<::std::string::String>,
    ///The MIME type of this resource, if known.
    #[serde(
        rename = "mimeType",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub mime_type: ::std::option::Option<::std::string::String>,
    /**A human-readable name for this resource.

This can be used by clients to populate UI elements.*/
    pub name: ::std::string::String,
    /**The size of the raw resource content, in bytes (i.e., before base64 encoding or any tokenization), if known.

This can be used by Hosts to display file sizes and estimate context window usage.*/
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub size: ::std::option::Option<i64>,
    ///The URI of this resource.
    pub uri: ::std::string::String,
}
impl ::std::convert::From<&Resource> for Resource {
    fn from(value: &Resource) -> Self {
        value.clone()
    }
}
///ResourceAnnotations
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "audience": {
///      "description": "Describes who the intended customer of this object or data is.\n\nIt can include multiple entries to indicate content useful for multiple audiences (e.g., `[\"user\", \"assistant\"]`).",
///      "type": "array",
///      "items": {
///        "$ref": "#/definitions/Role"
///      }
///    },
///    "priority": {
///      "description": "Describes how important this data is for operating the server.\n\nA value of 1 means \"most important,\" and indicates that the data is\neffectively required, while 0 means \"least important,\" and indicates that\nthe data is entirely optional.",
///      "type": "number",
///      "maximum": 1.0,
///      "minimum": 0.0
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ResourceAnnotations {
    /**Describes who the intended customer of this object or data is.

It can include multiple entries to indicate content useful for multiple audiences (e.g., `["user", "assistant"]`).*/
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub audience: ::std::vec::Vec<Role>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub priority: ::std::option::Option<f64>,
}
impl ::std::convert::From<&ResourceAnnotations> for ResourceAnnotations {
    fn from(value: &ResourceAnnotations) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ResourceAnnotations {
    fn default() -> Self {
        Self {
            audience: Default::default(),
            priority: Default::default(),
        }
    }
}
///The contents of a specific resource or sub-resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The contents of a specific resource or sub-resource.",
///  "type": "object",
///  "required": [
///    "uri"
///  ],
///  "properties": {
///    "mimeType": {
///      "description": "The MIME type of this resource, if known.",
///      "type": "string"
///    },
///    "uri": {
///      "description": "The URI of this resource.",
///      "type": "string",
///      "format": "uri"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ResourceContents {
    ///The MIME type of this resource, if known.
    #[serde(
        rename = "mimeType",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub mime_type: ::std::option::Option<::std::string::String>,
    ///The URI of this resource.
    pub uri: ::std::string::String,
}
impl ::std::convert::From<&ResourceContents> for ResourceContents {
    fn from(value: &ResourceContents) -> Self {
        value.clone()
    }
}
///An optional notification from the server to the client, informing it that the list of resources it can read from has changed. This may be issued by servers without any previous subscription from the client.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An optional notification from the server to the client, informing it that the list of resources it can read from has changed. This may be issued by servers without any previous subscription from the client.",
///  "type": "object",
///  "required": [
///    "method"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "notifications/resources/list_changed"
///    },
///    "params": {
///      "type": "object",
///      "properties": {
///        "_meta": {
///          "description": "This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.",
///          "type": "object",
///          "additionalProperties": {}
///        }
///      },
///      "additionalProperties": {}
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ResourceListChangedNotification {
    pub method: ::std::string::String,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub params: ::std::option::Option<ResourceListChangedNotificationParams>,
}
impl ::std::convert::From<&ResourceListChangedNotification>
for ResourceListChangedNotification {
    fn from(value: &ResourceListChangedNotification) -> Self {
        value.clone()
    }
}
///ResourceListChangedNotificationParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "_meta": {
///      "description": "This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.",
///      "type": "object",
///      "additionalProperties": {}
///    }
///  },
///  "additionalProperties": {}
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ResourceListChangedNotificationParams {
    ///This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
}
impl ::std::convert::From<&ResourceListChangedNotificationParams>
for ResourceListChangedNotificationParams {
    fn from(value: &ResourceListChangedNotificationParams) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ResourceListChangedNotificationParams {
    fn default() -> Self {
        Self { meta: Default::default() }
    }
}
///A reference to a resource or resource template definition.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A reference to a resource or resource template definition.",
///  "type": "object",
///  "required": [
///    "type",
///    "uri"
///  ],
///  "properties": {
///    "type": {
///      "type": "string",
///      "const": "ref/resource"
///    },
///    "uri": {
///      "description": "The URI or URI template of the resource.",
///      "type": "string",
///      "format": "uri-template"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ResourceReference {
    #[serde(rename = "type")]
    pub type_: ::std::string::String,
    ///The URI or URI template of the resource.
    pub uri: ::std::string::String,
}
impl ::std::convert::From<&ResourceReference> for ResourceReference {
    fn from(value: &ResourceReference) -> Self {
        value.clone()
    }
}
///A template description for resources available on the server.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A template description for resources available on the server.",
///  "type": "object",
///  "required": [
///    "name",
///    "uriTemplate"
///  ],
///  "properties": {
///    "annotations": {
///      "type": "object",
///      "properties": {
///        "audience": {
///          "description": "Describes who the intended customer of this object or data is.\n\nIt can include multiple entries to indicate content useful for multiple audiences (e.g., `[\"user\", \"assistant\"]`).",
///          "type": "array",
///          "items": {
///            "$ref": "#/definitions/Role"
///          }
///        },
///        "priority": {
///          "description": "Describes how important this data is for operating the server.\n\nA value of 1 means \"most important,\" and indicates that the data is\neffectively required, while 0 means \"least important,\" and indicates that\nthe data is entirely optional.",
///          "type": "number",
///          "maximum": 1.0,
///          "minimum": 0.0
///        }
///      }
///    },
///    "description": {
///      "description": "A description of what this template is for.\n\nThis can be used by clients to improve the LLM's understanding of available resources. It can be thought of like a \"hint\" to the model.",
///      "type": "string"
///    },
///    "mimeType": {
///      "description": "The MIME type for all resources that match this template. This should only be included if all resources matching this template have the same type.",
///      "type": "string"
///    },
///    "name": {
///      "description": "A human-readable name for the type of resource this template refers to.\n\nThis can be used by clients to populate UI elements.",
///      "type": "string"
///    },
///    "uriTemplate": {
///      "description": "A URI template (according to RFC 6570) that can be used to construct resource URIs.",
///      "type": "string",
///      "format": "uri-template"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ResourceTemplate {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub annotations: ::std::option::Option<ResourceTemplateAnnotations>,
    /**A description of what this template is for.

This can be used by clients to improve the LLM's understanding of available resources. It can be thought of like a "hint" to the model.*/
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub description: ::std::option::Option<::std::string::String>,
    ///The MIME type for all resources that match this template. This should only be included if all resources matching this template have the same type.
    #[serde(
        rename = "mimeType",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub mime_type: ::std::option::Option<::std::string::String>,
    /**A human-readable name for the type of resource this template refers to.

This can be used by clients to populate UI elements.*/
    pub name: ::std::string::String,
    ///A URI template (according to RFC 6570) that can be used to construct resource URIs.
    #[serde(rename = "uriTemplate")]
    pub uri_template: ::std::string::String,
}
impl ::std::convert::From<&ResourceTemplate> for ResourceTemplate {
    fn from(value: &ResourceTemplate) -> Self {
        value.clone()
    }
}
///ResourceTemplateAnnotations
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "audience": {
///      "description": "Describes who the intended customer of this object or data is.\n\nIt can include multiple entries to indicate content useful for multiple audiences (e.g., `[\"user\", \"assistant\"]`).",
///      "type": "array",
///      "items": {
///        "$ref": "#/definitions/Role"
///      }
///    },
///    "priority": {
///      "description": "Describes how important this data is for operating the server.\n\nA value of 1 means \"most important,\" and indicates that the data is\neffectively required, while 0 means \"least important,\" and indicates that\nthe data is entirely optional.",
///      "type": "number",
///      "maximum": 1.0,
///      "minimum": 0.0
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ResourceTemplateAnnotations {
    /**Describes who the intended customer of this object or data is.

It can include multiple entries to indicate content useful for multiple audiences (e.g., `["user", "assistant"]`).*/
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub audience: ::std::vec::Vec<Role>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub priority: ::std::option::Option<f64>,
}
impl ::std::convert::From<&ResourceTemplateAnnotations> for ResourceTemplateAnnotations {
    fn from(value: &ResourceTemplateAnnotations) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ResourceTemplateAnnotations {
    fn default() -> Self {
        Self {
            audience: Default::default(),
            priority: Default::default(),
        }
    }
}
///A notification from the server to the client, informing it that a resource has changed and may need to be read again. This should only be sent if the client previously sent a resources/subscribe request.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A notification from the server to the client, informing it that a resource has changed and may need to be read again. This should only be sent if the client previously sent a resources/subscribe request.",
///  "type": "object",
///  "required": [
///    "method",
///    "params"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "notifications/resources/updated"
///    },
///    "params": {
///      "type": "object",
///      "required": [
///        "uri"
///      ],
///      "properties": {
///        "uri": {
///          "description": "The URI of the resource that has been updated. This might be a sub-resource of the one that the client actually subscribed to.",
///          "type": "string",
///          "format": "uri"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ResourceUpdatedNotification {
    pub method: ::std::string::String,
    pub params: ResourceUpdatedNotificationParams,
}
impl ::std::convert::From<&ResourceUpdatedNotification> for ResourceUpdatedNotification {
    fn from(value: &ResourceUpdatedNotification) -> Self {
        value.clone()
    }
}
///ResourceUpdatedNotificationParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "uri"
///  ],
///  "properties": {
///    "uri": {
///      "description": "The URI of the resource that has been updated. This might be a sub-resource of the one that the client actually subscribed to.",
///      "type": "string",
///      "format": "uri"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ResourceUpdatedNotificationParams {
    ///The URI of the resource that has been updated. This might be a sub-resource of the one that the client actually subscribed to.
    pub uri: ::std::string::String,
}
impl ::std::convert::From<&ResourceUpdatedNotificationParams>
for ResourceUpdatedNotificationParams {
    fn from(value: &ResourceUpdatedNotificationParams) -> Self {
        value.clone()
    }
}
///Result
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "_meta": {
///      "description": "This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.",
///      "type": "object",
///      "additionalProperties": {}
///    }
///  },
///  "additionalProperties": {}
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct Result {
    ///This result property is reserved by the protocol to allow clients and servers to attach additional metadata to their responses.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
}
impl ::std::convert::From<&Result> for Result {
    fn from(value: &Result) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Result {
    fn default() -> Self {
        Self { meta: Default::default() }
    }
}
///The sender or recipient of messages and data in a conversation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The sender or recipient of messages and data in a conversation.",
///  "type": "string",
///  "enum": [
///    "assistant",
///    "user"
///  ]
///}
/// ```
/// </details>
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd
)]
pub enum Role {
    #[serde(rename = "assistant")]
    Assistant,
    #[serde(rename = "user")]
    User,
}
impl ::std::convert::From<&Self> for Role {
    fn from(value: &Role) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for Role {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Assistant => write!(f, "assistant"),
            Self::User => write!(f, "user"),
        }
    }
}
impl ::std::str::FromStr for Role {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "assistant" => Ok(Self::Assistant),
            "user" => Ok(Self::User),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for Role {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for Role {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for Role {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Represents a root directory or file that the server can operate on.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Represents a root directory or file that the server can operate on.",
///  "type": "object",
///  "required": [
///    "uri"
///  ],
///  "properties": {
///    "name": {
///      "description": "An optional name for the root. This can be used to provide a human-readable\nidentifier for the root, which may be useful for display purposes or for\nreferencing the root in other parts of the application.",
///      "type": "string"
///    },
///    "uri": {
///      "description": "The URI identifying the root. This *must* start with file:// for now.\nThis restriction may be relaxed in future versions of the protocol to allow\nother URI schemes.",
///      "type": "string",
///      "format": "uri"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct Root {
    /**An optional name for the root. This can be used to provide a human-readable
identifier for the root, which may be useful for display purposes or for
referencing the root in other parts of the application.*/
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub name: ::std::option::Option<::std::string::String>,
    /**The URI identifying the root. This *must* start with file:// for now.
This restriction may be relaxed in future versions of the protocol to allow
other URI schemes.*/
    pub uri: ::std::string::String,
}
impl ::std::convert::From<&Root> for Root {
    fn from(value: &Root) -> Self {
        value.clone()
    }
}
/**A notification from the client to the server, informing it that the list of roots has changed.
This notification should be sent whenever the client adds, removes, or modifies any root.
The server should then request an updated list of roots using the ListRootsRequest.*/
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A notification from the client to the server, informing it that the list of roots has changed.\nThis notification should be sent whenever the client adds, removes, or modifies any root.\nThe server should then request an updated list of roots using the ListRootsRequest.",
///  "type": "object",
///  "required": [
///    "method"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "notifications/roots/list_changed"
///    },
///    "params": {
///      "type": "object",
///      "properties": {
///        "_meta": {
///          "description": "This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.",
///          "type": "object",
///          "additionalProperties": {}
///        }
///      },
///      "additionalProperties": {}
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct RootsListChangedNotification {
    pub method: ::std::string::String,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub params: ::std::option::Option<RootsListChangedNotificationParams>,
}
impl ::std::convert::From<&RootsListChangedNotification>
for RootsListChangedNotification {
    fn from(value: &RootsListChangedNotification) -> Self {
        value.clone()
    }
}
///RootsListChangedNotificationParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "_meta": {
///      "description": "This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.",
///      "type": "object",
///      "additionalProperties": {}
///    }
///  },
///  "additionalProperties": {}
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct RootsListChangedNotificationParams {
    ///This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
}
impl ::std::convert::From<&RootsListChangedNotificationParams>
for RootsListChangedNotificationParams {
    fn from(value: &RootsListChangedNotificationParams) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RootsListChangedNotificationParams {
    fn default() -> Self {
        Self { meta: Default::default() }
    }
}
///Describes a message issued to or received from an LLM API.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Describes a message issued to or received from an LLM API.",
///  "type": "object",
///  "required": [
///    "content",
///    "role"
///  ],
///  "properties": {
///    "content": {
///      "anyOf": [
///        {
///          "$ref": "#/definitions/TextContent"
///        },
///        {
///          "$ref": "#/definitions/ImageContent"
///        }
///      ]
///    },
///    "role": {
///      "$ref": "#/definitions/Role"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct SamplingMessage {
    pub content: SamplingMessageContent,
    pub role: Role,
}
impl ::std::convert::From<&SamplingMessage> for SamplingMessage {
    fn from(value: &SamplingMessage) -> Self {
        value.clone()
    }
}
///SamplingMessageContent
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "anyOf": [
///    {
///      "$ref": "#/definitions/TextContent"
///    },
///    {
///      "$ref": "#/definitions/ImageContent"
///    }
///  ]
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum SamplingMessageContent {
    TextContent(TextContent),
    ImageContent(ImageContent),
}
impl ::std::convert::From<&Self> for SamplingMessageContent {
    fn from(value: &SamplingMessageContent) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<TextContent> for SamplingMessageContent {
    fn from(value: TextContent) -> Self {
        Self::TextContent(value)
    }
}
impl ::std::convert::From<ImageContent> for SamplingMessageContent {
    fn from(value: ImageContent) -> Self {
        Self::ImageContent(value)
    }
}
///Capabilities that a server may support. Known capabilities are defined here, in this schema, but this is not a closed set: any server can define its own, additional capabilities.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Capabilities that a server may support. Known capabilities are defined here, in this schema, but this is not a closed set: any server can define its own, additional capabilities.",
///  "type": "object",
///  "properties": {
///    "experimental": {
///      "description": "Experimental, non-standard capabilities that the server supports.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "object",
///        "additionalProperties": true
///      }
///    },
///    "logging": {
///      "description": "Present if the server supports sending log messages to the client.",
///      "type": "object",
///      "additionalProperties": true
///    },
///    "prompts": {
///      "description": "Present if the server offers any prompt templates.",
///      "type": "object",
///      "properties": {
///        "listChanged": {
///          "description": "Whether this server supports notifications for changes to the prompt list.",
///          "type": "boolean"
///        }
///      }
///    },
///    "resources": {
///      "description": "Present if the server offers any resources to read.",
///      "type": "object",
///      "properties": {
///        "listChanged": {
///          "description": "Whether this server supports notifications for changes to the resource list.",
///          "type": "boolean"
///        },
///        "subscribe": {
///          "description": "Whether this server supports subscribing to resource updates.",
///          "type": "boolean"
///        }
///      }
///    },
///    "tools": {
///      "description": "Present if the server offers any tools to call.",
///      "type": "object",
///      "properties": {
///        "listChanged": {
///          "description": "Whether this server supports notifications for changes to the tool list.",
///          "type": "boolean"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ServerCapabilities {
    ///Experimental, non-standard capabilities that the server supports.
    #[serde(default, skip_serializing_if = ":: std :: collections :: HashMap::is_empty")]
    pub experimental: ::std::collections::HashMap<
        ::std::string::String,
        ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    >,
    ///Present if the server supports sending log messages to the client.
    #[serde(default, skip_serializing_if = "::serde_json::Map::is_empty")]
    pub logging: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub prompts: ::std::option::Option<ServerCapabilitiesPrompts>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub resources: ::std::option::Option<ServerCapabilitiesResources>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub tools: ::std::option::Option<ServerCapabilitiesTools>,
}
impl ::std::convert::From<&ServerCapabilities> for ServerCapabilities {
    fn from(value: &ServerCapabilities) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ServerCapabilities {
    fn default() -> Self {
        Self {
            experimental: Default::default(),
            logging: Default::default(),
            prompts: Default::default(),
            resources: Default::default(),
            tools: Default::default(),
        }
    }
}
///Present if the server offers any prompt templates.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Present if the server offers any prompt templates.",
///  "type": "object",
///  "properties": {
///    "listChanged": {
///      "description": "Whether this server supports notifications for changes to the prompt list.",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ServerCapabilitiesPrompts {
    ///Whether this server supports notifications for changes to the prompt list.
    #[serde(
        rename = "listChanged",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub list_changed: ::std::option::Option<bool>,
}
impl ::std::convert::From<&ServerCapabilitiesPrompts> for ServerCapabilitiesPrompts {
    fn from(value: &ServerCapabilitiesPrompts) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ServerCapabilitiesPrompts {
    fn default() -> Self {
        Self {
            list_changed: Default::default(),
        }
    }
}
///Present if the server offers any resources to read.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Present if the server offers any resources to read.",
///  "type": "object",
///  "properties": {
///    "listChanged": {
///      "description": "Whether this server supports notifications for changes to the resource list.",
///      "type": "boolean"
///    },
///    "subscribe": {
///      "description": "Whether this server supports subscribing to resource updates.",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ServerCapabilitiesResources {
    ///Whether this server supports notifications for changes to the resource list.
    #[serde(
        rename = "listChanged",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub list_changed: ::std::option::Option<bool>,
    ///Whether this server supports subscribing to resource updates.
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub subscribe: ::std::option::Option<bool>,
}
impl ::std::convert::From<&ServerCapabilitiesResources> for ServerCapabilitiesResources {
    fn from(value: &ServerCapabilitiesResources) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ServerCapabilitiesResources {
    fn default() -> Self {
        Self {
            list_changed: Default::default(),
            subscribe: Default::default(),
        }
    }
}
///Present if the server offers any tools to call.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Present if the server offers any tools to call.",
///  "type": "object",
///  "properties": {
///    "listChanged": {
///      "description": "Whether this server supports notifications for changes to the tool list.",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ServerCapabilitiesTools {
    ///Whether this server supports notifications for changes to the tool list.
    #[serde(
        rename = "listChanged",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub list_changed: ::std::option::Option<bool>,
}
impl ::std::convert::From<&ServerCapabilitiesTools> for ServerCapabilitiesTools {
    fn from(value: &ServerCapabilitiesTools) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ServerCapabilitiesTools {
    fn default() -> Self {
        Self {
            list_changed: Default::default(),
        }
    }
}
///ServerNotification
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "anyOf": [
///    {
///      "$ref": "#/definitions/CancelledNotification"
///    },
///    {
///      "$ref": "#/definitions/ProgressNotification"
///    },
///    {
///      "$ref": "#/definitions/ResourceListChangedNotification"
///    },
///    {
///      "$ref": "#/definitions/ResourceUpdatedNotification"
///    },
///    {
///      "$ref": "#/definitions/PromptListChangedNotification"
///    },
///    {
///      "$ref": "#/definitions/ToolListChangedNotification"
///    },
///    {
///      "$ref": "#/definitions/LoggingMessageNotification"
///    }
///  ]
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum ServerNotification {
    CancelledNotification(CancelledNotification),
    ProgressNotification(ProgressNotification),
    ResourceListChangedNotification(ResourceListChangedNotification),
    ResourceUpdatedNotification(ResourceUpdatedNotification),
    PromptListChangedNotification(PromptListChangedNotification),
    ToolListChangedNotification(ToolListChangedNotification),
    LoggingMessageNotification(LoggingMessageNotification),
}
impl ::std::convert::From<&Self> for ServerNotification {
    fn from(value: &ServerNotification) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<CancelledNotification> for ServerNotification {
    fn from(value: CancelledNotification) -> Self {
        Self::CancelledNotification(value)
    }
}
impl ::std::convert::From<ProgressNotification> for ServerNotification {
    fn from(value: ProgressNotification) -> Self {
        Self::ProgressNotification(value)
    }
}
impl ::std::convert::From<ResourceListChangedNotification> for ServerNotification {
    fn from(value: ResourceListChangedNotification) -> Self {
        Self::ResourceListChangedNotification(value)
    }
}
impl ::std::convert::From<ResourceUpdatedNotification> for ServerNotification {
    fn from(value: ResourceUpdatedNotification) -> Self {
        Self::ResourceUpdatedNotification(value)
    }
}
impl ::std::convert::From<PromptListChangedNotification> for ServerNotification {
    fn from(value: PromptListChangedNotification) -> Self {
        Self::PromptListChangedNotification(value)
    }
}
impl ::std::convert::From<ToolListChangedNotification> for ServerNotification {
    fn from(value: ToolListChangedNotification) -> Self {
        Self::ToolListChangedNotification(value)
    }
}
impl ::std::convert::From<LoggingMessageNotification> for ServerNotification {
    fn from(value: LoggingMessageNotification) -> Self {
        Self::LoggingMessageNotification(value)
    }
}
///ServerRequest
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "anyOf": [
///    {
///      "$ref": "#/definitions/PingRequest"
///    },
///    {
///      "$ref": "#/definitions/CreateMessageRequest"
///    },
///    {
///      "$ref": "#/definitions/ListRootsRequest"
///    }
///  ]
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum ServerRequest {
    PingRequest(PingRequest),
    CreateMessageRequest(CreateMessageRequest),
    ListRootsRequest(ListRootsRequest),
}
impl ::std::convert::From<&Self> for ServerRequest {
    fn from(value: &ServerRequest) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<PingRequest> for ServerRequest {
    fn from(value: PingRequest) -> Self {
        Self::PingRequest(value)
    }
}
impl ::std::convert::From<CreateMessageRequest> for ServerRequest {
    fn from(value: CreateMessageRequest) -> Self {
        Self::CreateMessageRequest(value)
    }
}
impl ::std::convert::From<ListRootsRequest> for ServerRequest {
    fn from(value: ListRootsRequest) -> Self {
        Self::ListRootsRequest(value)
    }
}
///ServerResult
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "anyOf": [
///    {
///      "$ref": "#/definitions/Result"
///    },
///    {
///      "$ref": "#/definitions/InitializeResult"
///    },
///    {
///      "$ref": "#/definitions/ListResourcesResult"
///    },
///    {
///      "$ref": "#/definitions/ListResourceTemplatesResult"
///    },
///    {
///      "$ref": "#/definitions/ReadResourceResult"
///    },
///    {
///      "$ref": "#/definitions/ListPromptsResult"
///    },
///    {
///      "$ref": "#/definitions/GetPromptResult"
///    },
///    {
///      "$ref": "#/definitions/ListToolsResult"
///    },
///    {
///      "$ref": "#/definitions/CallToolResult"
///    },
///    {
///      "$ref": "#/definitions/CompleteResult"
///    }
///  ]
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum ServerResult {
    Result(Result),
    InitializeResult(InitializeResult),
    ListResourcesResult(ListResourcesResult),
    ListResourceTemplatesResult(ListResourceTemplatesResult),
    ReadResourceResult(ReadResourceResult),
    ListPromptsResult(ListPromptsResult),
    GetPromptResult(GetPromptResult),
    ListToolsResult(ListToolsResult),
    CallToolResult(CallToolResult),
    CompleteResult(CompleteResult),
}
impl ::std::convert::From<&Self> for ServerResult {
    fn from(value: &ServerResult) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<Result> for ServerResult {
    fn from(value: Result) -> Self {
        Self::Result(value)
    }
}
impl ::std::convert::From<InitializeResult> for ServerResult {
    fn from(value: InitializeResult) -> Self {
        Self::InitializeResult(value)
    }
}
impl ::std::convert::From<ListResourcesResult> for ServerResult {
    fn from(value: ListResourcesResult) -> Self {
        Self::ListResourcesResult(value)
    }
}
impl ::std::convert::From<ListResourceTemplatesResult> for ServerResult {
    fn from(value: ListResourceTemplatesResult) -> Self {
        Self::ListResourceTemplatesResult(value)
    }
}
impl ::std::convert::From<ReadResourceResult> for ServerResult {
    fn from(value: ReadResourceResult) -> Self {
        Self::ReadResourceResult(value)
    }
}
impl ::std::convert::From<ListPromptsResult> for ServerResult {
    fn from(value: ListPromptsResult) -> Self {
        Self::ListPromptsResult(value)
    }
}
impl ::std::convert::From<GetPromptResult> for ServerResult {
    fn from(value: GetPromptResult) -> Self {
        Self::GetPromptResult(value)
    }
}
impl ::std::convert::From<ListToolsResult> for ServerResult {
    fn from(value: ListToolsResult) -> Self {
        Self::ListToolsResult(value)
    }
}
impl ::std::convert::From<CallToolResult> for ServerResult {
    fn from(value: CallToolResult) -> Self {
        Self::CallToolResult(value)
    }
}
impl ::std::convert::From<CompleteResult> for ServerResult {
    fn from(value: CompleteResult) -> Self {
        Self::CompleteResult(value)
    }
}
///A request from the client to the server, to enable or adjust logging.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A request from the client to the server, to enable or adjust logging.",
///  "type": "object",
///  "required": [
///    "method",
///    "params"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "logging/setLevel"
///    },
///    "params": {
///      "type": "object",
///      "required": [
///        "level"
///      ],
///      "properties": {
///        "level": {
///          "description": "The level of logging that the client wants to receive from the server. The server should send all logs at this level and higher (i.e., more severe) to the client as notifications/logging/message.",
///          "$ref": "#/definitions/LoggingLevel"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct SetLevelRequest {
    pub method: ::std::string::String,
    pub params: SetLevelRequestParams,
}
impl ::std::convert::From<&SetLevelRequest> for SetLevelRequest {
    fn from(value: &SetLevelRequest) -> Self {
        value.clone()
    }
}
///SetLevelRequestParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "level"
///  ],
///  "properties": {
///    "level": {
///      "description": "The level of logging that the client wants to receive from the server. The server should send all logs at this level and higher (i.e., more severe) to the client as notifications/logging/message.",
///      "$ref": "#/definitions/LoggingLevel"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct SetLevelRequestParams {
    ///The level of logging that the client wants to receive from the server. The server should send all logs at this level and higher (i.e., more severe) to the client as notifications/logging/message.
    pub level: LoggingLevel,
}
impl ::std::convert::From<&SetLevelRequestParams> for SetLevelRequestParams {
    fn from(value: &SetLevelRequestParams) -> Self {
        value.clone()
    }
}
///Sent from the client to request resources/updated notifications from the server whenever a particular resource changes.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Sent from the client to request resources/updated notifications from the server whenever a particular resource changes.",
///  "type": "object",
///  "required": [
///    "method",
///    "params"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "resources/subscribe"
///    },
///    "params": {
///      "type": "object",
///      "required": [
///        "uri"
///      ],
///      "properties": {
///        "uri": {
///          "description": "The URI of the resource to subscribe to. The URI can use any protocol; it is up to the server how to interpret it.",
///          "type": "string",
///          "format": "uri"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct SubscribeRequest {
    pub method: ::std::string::String,
    pub params: SubscribeRequestParams,
}
impl ::std::convert::From<&SubscribeRequest> for SubscribeRequest {
    fn from(value: &SubscribeRequest) -> Self {
        value.clone()
    }
}
///SubscribeRequestParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "uri"
///  ],
///  "properties": {
///    "uri": {
///      "description": "The URI of the resource to subscribe to. The URI can use any protocol; it is up to the server how to interpret it.",
///      "type": "string",
///      "format": "uri"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct SubscribeRequestParams {
    ///The URI of the resource to subscribe to. The URI can use any protocol; it is up to the server how to interpret it.
    pub uri: ::std::string::String,
}
impl ::std::convert::From<&SubscribeRequestParams> for SubscribeRequestParams {
    fn from(value: &SubscribeRequestParams) -> Self {
        value.clone()
    }
}
///Text provided to or from an LLM.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Text provided to or from an LLM.",
///  "type": "object",
///  "required": [
///    "text",
///    "type"
///  ],
///  "properties": {
///    "annotations": {
///      "type": "object",
///      "properties": {
///        "audience": {
///          "description": "Describes who the intended customer of this object or data is.\n\nIt can include multiple entries to indicate content useful for multiple audiences (e.g., `[\"user\", \"assistant\"]`).",
///          "type": "array",
///          "items": {
///            "$ref": "#/definitions/Role"
///          }
///        },
///        "priority": {
///          "description": "Describes how important this data is for operating the server.\n\nA value of 1 means \"most important,\" and indicates that the data is\neffectively required, while 0 means \"least important,\" and indicates that\nthe data is entirely optional.",
///          "type": "number",
///          "maximum": 1.0,
///          "minimum": 0.0
///        }
///      }
///    },
///    "text": {
///      "description": "The text content of the message.",
///      "type": "string"
///    },
///    "type": {
///      "type": "string",
///      "const": "text"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct TextContent {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub annotations: ::std::option::Option<TextContentAnnotations>,
    ///The text content of the message.
    pub text: ::std::string::String,
    #[serde(rename = "type")]
    pub type_: ::std::string::String,
}
impl ::std::convert::From<&TextContent> for TextContent {
    fn from(value: &TextContent) -> Self {
        value.clone()
    }
}
///TextContentAnnotations
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "audience": {
///      "description": "Describes who the intended customer of this object or data is.\n\nIt can include multiple entries to indicate content useful for multiple audiences (e.g., `[\"user\", \"assistant\"]`).",
///      "type": "array",
///      "items": {
///        "$ref": "#/definitions/Role"
///      }
///    },
///    "priority": {
///      "description": "Describes how important this data is for operating the server.\n\nA value of 1 means \"most important,\" and indicates that the data is\neffectively required, while 0 means \"least important,\" and indicates that\nthe data is entirely optional.",
///      "type": "number",
///      "maximum": 1.0,
///      "minimum": 0.0
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct TextContentAnnotations {
    /**Describes who the intended customer of this object or data is.

It can include multiple entries to indicate content useful for multiple audiences (e.g., `["user", "assistant"]`).*/
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub audience: ::std::vec::Vec<Role>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub priority: ::std::option::Option<f64>,
}
impl ::std::convert::From<&TextContentAnnotations> for TextContentAnnotations {
    fn from(value: &TextContentAnnotations) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TextContentAnnotations {
    fn default() -> Self {
        Self {
            audience: Default::default(),
            priority: Default::default(),
        }
    }
}
///TextResourceContents
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "text",
///    "uri"
///  ],
///  "properties": {
///    "mimeType": {
///      "description": "The MIME type of this resource, if known.",
///      "type": "string"
///    },
///    "text": {
///      "description": "The text of the item. This must only be set if the item can actually be represented as text (not binary data).",
///      "type": "string"
///    },
///    "uri": {
///      "description": "The URI of this resource.",
///      "type": "string",
///      "format": "uri"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct TextResourceContents {
    ///The MIME type of this resource, if known.
    #[serde(
        rename = "mimeType",
        default,
        skip_serializing_if = "::std::option::Option::is_none"
    )]
    pub mime_type: ::std::option::Option<::std::string::String>,
    ///The text of the item. This must only be set if the item can actually be represented as text (not binary data).
    pub text: ::std::string::String,
    ///The URI of this resource.
    pub uri: ::std::string::String,
}
impl ::std::convert::From<&TextResourceContents> for TextResourceContents {
    fn from(value: &TextResourceContents) -> Self {
        value.clone()
    }
}
///Definition for a tool the client can call.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Definition for a tool the client can call.",
///  "type": "object",
///  "required": [
///    "inputSchema",
///    "name"
///  ],
///  "properties": {
///    "description": {
///      "description": "A human-readable description of the tool.",
///      "type": "string"
///    },
///    "inputSchema": {
///      "description": "A JSON Schema object defining the expected parameters for the tool.",
///      "type": "object",
///      "required": [
///        "type"
///      ],
///      "properties": {
///        "properties": {
///          "type": "object",
///          "additionalProperties": {
///            "type": "object",
///            "additionalProperties": true
///          }
///        },
///        "required": {
///          "type": "array",
///          "items": {
///            "type": "string"
///          }
///        },
///        "type": {
///          "type": "string",
///          "const": "object"
///        }
///      }
///    },
///    "name": {
///      "description": "The name of the tool.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct Tool {
    ///A human-readable description of the tool.
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub description: ::std::option::Option<::std::string::String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: ToolInputSchema,
    ///The name of the tool.
    pub name: ::std::string::String,
}
impl ::std::convert::From<&Tool> for Tool {
    fn from(value: &Tool) -> Self {
        value.clone()
    }
}
///A JSON Schema object defining the expected parameters for the tool.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A JSON Schema object defining the expected parameters for the tool.",
///  "type": "object",
///  "required": [
///    "type"
///  ],
///  "properties": {
///    "properties": {
///      "type": "object",
///      "additionalProperties": {
///        "type": "object",
///        "additionalProperties": true
///      }
///    },
///    "required": {
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "type": {
///      "type": "string",
///      "const": "object"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ToolInputSchema {
    #[serde(default, skip_serializing_if = ":: std :: collections :: HashMap::is_empty")]
    pub properties: ::std::collections::HashMap<
        ::std::string::String,
        ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    >,
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub required: ::std::vec::Vec<::std::string::String>,
    #[serde(rename = "type")]
    pub type_: ::std::string::String,
}
impl ::std::convert::From<&ToolInputSchema> for ToolInputSchema {
    fn from(value: &ToolInputSchema) -> Self {
        value.clone()
    }
}
///An optional notification from the server to the client, informing it that the list of tools it offers has changed. This may be issued by servers without any previous subscription from the client.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An optional notification from the server to the client, informing it that the list of tools it offers has changed. This may be issued by servers without any previous subscription from the client.",
///  "type": "object",
///  "required": [
///    "method"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "notifications/tools/list_changed"
///    },
///    "params": {
///      "type": "object",
///      "properties": {
///        "_meta": {
///          "description": "This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.",
///          "type": "object",
///          "additionalProperties": {}
///        }
///      },
///      "additionalProperties": {}
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ToolListChangedNotification {
    pub method: ::std::string::String,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub params: ::std::option::Option<ToolListChangedNotificationParams>,
}
impl ::std::convert::From<&ToolListChangedNotification> for ToolListChangedNotification {
    fn from(value: &ToolListChangedNotification) -> Self {
        value.clone()
    }
}
///ToolListChangedNotificationParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "_meta": {
///      "description": "This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.",
///      "type": "object",
///      "additionalProperties": {}
///    }
///  },
///  "additionalProperties": {}
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct ToolListChangedNotificationParams {
    ///This parameter name is reserved by MCP to allow clients and servers to attach additional metadata to their notifications.
    #[serde(
        rename = "_meta",
        default,
        skip_serializing_if = "::serde_json::Map::is_empty"
    )]
    pub meta: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
}
impl ::std::convert::From<&ToolListChangedNotificationParams>
for ToolListChangedNotificationParams {
    fn from(value: &ToolListChangedNotificationParams) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ToolListChangedNotificationParams {
    fn default() -> Self {
        Self { meta: Default::default() }
    }
}
///Sent from the client to request cancellation of resources/updated notifications from the server. This should follow a previous resources/subscribe request.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Sent from the client to request cancellation of resources/updated notifications from the server. This should follow a previous resources/subscribe request.",
///  "type": "object",
///  "required": [
///    "method",
///    "params"
///  ],
///  "properties": {
///    "method": {
///      "type": "string",
///      "const": "resources/unsubscribe"
///    },
///    "params": {
///      "type": "object",
///      "required": [
///        "uri"
///      ],
///      "properties": {
///        "uri": {
///          "description": "The URI of the resource to unsubscribe from.",
///          "type": "string",
///          "format": "uri"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct UnsubscribeRequest {
    pub method: ::std::string::String,
    pub params: UnsubscribeRequestParams,
}
impl ::std::convert::From<&UnsubscribeRequest> for UnsubscribeRequest {
    fn from(value: &UnsubscribeRequest) -> Self {
        value.clone()
    }
}
///UnsubscribeRequestParams
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "uri"
///  ],
///  "properties": {
///    "uri": {
///      "description": "The URI of the resource to unsubscribe from.",
///      "type": "string",
///      "format": "uri"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug, PartialEq)]
pub struct UnsubscribeRequestParams {
    ///The URI of the resource to unsubscribe from.
    pub uri: ::std::string::String,
}
impl ::std::convert::From<&UnsubscribeRequestParams> for UnsubscribeRequestParams {
    fn from(value: &UnsubscribeRequestParams) -> Self {
        value.clone()
    }
}
