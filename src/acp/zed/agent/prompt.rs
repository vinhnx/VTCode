use super::ZedAgent;
use agent_client_protocol::{self as acp, Client};
use anyhow::Result;
use percent_encoding::percent_decode_str;
use std::path::PathBuf;
use tracing::warn;
use url::Url;
use vtcode_core::config::constants::tools;

use super::super::constants::*;
use crate::acp::tooling::TOOL_READ_FILE_URI_ARG;

impl ZedAgent {
    fn append_segment(target: &mut String, segment: &str) {
        if !target.is_empty() {
            target.push('\n');
        }
        target.push_str(segment);
    }

    fn render_context_block(name: &str, uri: &str, body: Option<&str>) -> String {
        match body {
            Some(content) => {
                let mut rendered = String::new();
                rendered.push_str(RESOURCE_CONTEXT_OPEN);
                rendered.push(' ');
                rendered.push_str(RESOURCE_CONTEXT_URI_ATTR);
                rendered.push_str("=\"");
                rendered.push_str(uri);
                rendered.push_str("\" ");
                rendered.push_str(RESOURCE_CONTEXT_NAME_ATTR);
                rendered.push_str("=\"");
                rendered.push_str(name);
                rendered.push_str("\">\n");
                rendered.push_str(content);
                if !content.ends_with('\n') {
                    rendered.push('\n');
                }
                rendered.push_str(RESOURCE_CONTEXT_CLOSE);
                rendered
            }
            None => format!("{RESOURCE_FALLBACK_LABEL} {name} ({uri})"),
        }
    }

    pub(super) fn parse_resource_path(&self, uri: &str) -> Result<PathBuf, String> {
        if uri.is_empty() {
            return Err(format!(
                "Unable to resolve URI provided to {}",
                tools::READ_FILE
            ));
        }

        if uri.starts_with('/') {
            let candidate = PathBuf::from(uri);
            return self.resolve_workspace_path(candidate, TOOL_READ_FILE_URI_ARG);
        }

        let parsed = Url::parse(uri)
            .map_err(|_| format!("Unable to resolve URI provided to {}", tools::READ_FILE))?;

        let path = match parsed.scheme() {
            "file" => parsed
                .to_file_path()
                .map_err(|_| format!("Unable to resolve URI provided to {}", tools::READ_FILE))?,
            "zed" | "zed-fs" => {
                let raw_path = parsed.path();
                if raw_path.is_empty() {
                    return Err(format!(
                        "Unable to resolve URI provided to {}",
                        tools::READ_FILE
                    ));
                }
                let decoded = percent_decode_str(raw_path).decode_utf8().map_err(|_| {
                    format!("Unable to resolve URI provided to {}", tools::READ_FILE)
                })?;
                PathBuf::from(&*decoded)
            }
            _ => {
                return Err(format!(
                    "Unable to resolve URI provided to {}",
                    tools::READ_FILE
                ));
            }
        };

        self.resolve_workspace_path(path, TOOL_READ_FILE_URI_ARG)
    }

    pub(super) async fn resolve_prompt(
        &self,
        session_id: &acp::SessionId,
        prompt: &[acp::ContentBlock],
    ) -> Result<String, acp::Error> {
        let mut aggregated = String::new();

        for block in prompt {
            match block {
                acp::ContentBlock::Text(text) => Self::append_segment(&mut aggregated, &text.text),
                acp::ContentBlock::ResourceLink(link) => {
                    let rendered = self.render_resource_link(session_id, link).await?;
                    Self::append_segment(&mut aggregated, &rendered);
                }
                acp::ContentBlock::Resource(resource) => match &resource.resource {
                    acp::EmbeddedResourceResource::TextResourceContents(text) => {
                        let rendered =
                            Self::render_context_block(&text.uri, &text.uri, Some(&text.text));
                        Self::append_segment(&mut aggregated, &rendered);
                    }
                    acp::EmbeddedResourceResource::BlobResourceContents(blob) => {
                        warn!(
                            uri = blob.uri,
                            "Ignoring unsupported embedded blob resource"
                        );
                        let rendered = format!(
                            "{RESOURCE_FAILURE_LABEL} {name} ({uri})",
                            name = blob.uri,
                            uri = blob.uri
                        );
                        Self::append_segment(&mut aggregated, &rendered);
                    }
                },
                acp::ContentBlock::Image(image) => {
                    let identifier = image.uri.as_deref().unwrap_or(image.mime_type.as_str());
                    let placeholder = format!(
                        "{RESOURCE_FALLBACK_LABEL} image ({identifier})",
                        identifier = identifier
                    );
                    Self::append_segment(&mut aggregated, &placeholder);
                }
                acp::ContentBlock::Audio(audio) => {
                    let placeholder = format!(
                        "{RESOURCE_FALLBACK_LABEL} audio ({mime})",
                        mime = audio.mime_type
                    );
                    Self::append_segment(&mut aggregated, &placeholder);
                }
            }
        }

        Ok(aggregated)
    }

    async fn render_resource_link(
        &self,
        session_id: &acp::SessionId,
        link: &acp::ResourceLink,
    ) -> Result<String, acp::Error> {
        let Some(client) = self.client() else {
            return Ok(Self::render_context_block(&link.name, &link.uri, None));
        };

        if !self.client_supports_read_text_file() {
            return Ok(Self::render_context_block(&link.name, &link.uri, None));
        }

        let path = match self.parse_resource_path(&link.uri) {
            Ok(path) => path,
            Err(_) => {
                return Ok(Self::render_context_block(&link.name, &link.uri, None));
            }
        };

        let request = acp::ReadTextFileRequest {
            session_id: session_id.clone(),
            path,
            line: None,
            limit: None,
            meta: None,
        };

        match client.read_text_file(request).await {
            Ok(response) => Ok(Self::render_context_block(
                &link.name,
                &link.uri,
                Some(response.content.as_str()),
            )),
            Err(error) => {
                warn!(%error, uri = link.uri, name = link.name, "Failed to read linked resource");
                Ok(format!(
                    "{RESOURCE_FAILURE_LABEL} {name} ({uri})",
                    name = link.name,
                    uri = link.uri
                ))
            }
        }
    }
}
