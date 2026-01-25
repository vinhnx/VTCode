use reqwest::RequestBuilder;

pub(crate) fn apply_json_content_type(builder: RequestBuilder) -> RequestBuilder {
    builder.header("Content-Type", "application/json")
}

pub(crate) fn apply_responses_beta(builder: RequestBuilder) -> RequestBuilder {
    builder.header("OpenAI-Beta", "responses=v1")
}
