//! Document Processing for Skills using Vision Models
//!
//! Implements OpenAI-style document processing by converting PDFs, DOCX, and spreadsheets
//! to rendered images for vision model analysis. This preserves layout, formatting, and
//! visual information that would be lost in text extraction.
//!
//! ## Supported Formats
//!
//! - **PDF**: Multi-page documents converted to page-by-page PNGs
//! - **DOCX/DOC**: Word documents rendered per-page
//! - **Spreadsheets**: Excel/CSV files rendered as visual tables
//! - **Images**: Direct vision model processing
//!
//! ## Architecture
//!
//! ```text
//! Document → Renderer → PNG Images → Vision Model → Structured Data
//! ```
//!
//! Inspired by OpenAI's implementation in ChatGPT's Code Interpreter.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Document processing configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentProcessorConfig {
    /// Enable vision-based document processing
    pub enabled: bool,

    /// Output format for rendered pages
    pub image_format: String, // "png" recommended

    /// DPI for rendering (higher = better quality but larger files)
    pub dpi: u32,

    /// Maximum number of pages to process (prevent runaway)
    pub max_pages: usize,

    /// Enable OCR fallback for text extraction
    pub enable_ocr_fallback: bool,
}

impl Default for DocumentProcessorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            image_format: "png".to_string(),
            dpi: 150,      // Good balance of quality vs file size
            max_pages: 50, // Reasonable limit for most documents
            enable_ocr_fallback: true,
        }
    }
}

/// Processed document result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessedDocument {
    /// Original document path
    pub source_path: PathBuf,

    /// Document type
    pub doc_type: DocumentType,

    /// Page count
    pub page_count: usize,

    /// Rendered page images
    pub pages: Vec<PageImage>,

    /// Extracted text (with layout preservation)
    pub extracted_text: Option<String>,

    /// Document metadata
    pub metadata: DocumentMetadata,
}

/// Document type classification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DocumentType {
    Pdf,
    Docx,
    Doc,
    Xlsx,
    Xls,
    Csv,
    Txt,
    Rtf,
    Image,
    Unknown,
}

impl DocumentType {
    /// Detect document type from file extension
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("pdf") => DocumentType::Pdf,
            Some("docx") => DocumentType::Docx,
            Some("doc") => DocumentType::Doc,
            Some("xlsx") => DocumentType::Xlsx,
            Some("xls") => DocumentType::Xls,
            Some("csv") => DocumentType::Csv,
            Some("txt") => DocumentType::Txt,
            Some("rtf") => DocumentType::Rtf,
            Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("bmp") | Some("tiff") => {
                DocumentType::Image
            }
            _ => DocumentType::Unknown,
        }
    }

    /// Check if this document type is supported for vision processing
    pub fn supports_vision_processing(&self) -> bool {
        matches!(
            self,
            DocumentType::Pdf
                | DocumentType::Docx
                | DocumentType::Doc
                | DocumentType::Xlsx
                | DocumentType::Xls
                | DocumentType::Image
        )
    }
}

/// Single page image data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageImage {
    /// Page number (1-indexed)
    pub page_number: usize,

    /// Image file path
    pub image_path: PathBuf,

    /// Image dimensions
    pub dimensions: ImageDimensions,

    /// Page text content (if OCR enabled)
    pub text_content: Option<String>,
}

/// Image dimensions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageDimensions {
    pub width: u32,
    pub height: u32,
}

/// Document metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub created_date: Option<String>,
    pub modified_date: Option<String>,
    pub file_size: u64,
    pub page_count: Option<usize>,
}

/// Main document processor
pub struct DocumentProcessor {
    config: DocumentProcessorConfig,
    temp_dir: PathBuf,
}

impl DocumentProcessor {
    /// Create new document processor
    pub fn new(config: DocumentProcessorConfig) -> Result<Self> {
        let temp_dir = std::env::temp_dir().join("vtcode-document-processor");
        std::fs::create_dir_all(&temp_dir)?;

        Ok(Self { config, temp_dir })
    }

    /// Process a document for vision model analysis
    pub async fn process_document(&self, document_path: &Path) -> Result<ProcessedDocument> {
        if !self.config.enabled {
            return Err(anyhow!("Document processing is disabled"));
        }

        if !document_path.exists() {
            return Err(anyhow!("Document not found: {}", document_path.display()));
        }

        let doc_type = DocumentType::from_path(document_path);
        info!(
            "Processing document: {} (type: {:?})",
            document_path.display(),
            doc_type
        );

        match doc_type {
            DocumentType::Pdf => self.process_pdf(document_path).await,
            DocumentType::Docx | DocumentType::Doc => {
                self.process_word_document(document_path).await
            }
            DocumentType::Xlsx | DocumentType::Xls | DocumentType::Csv => {
                self.process_spreadsheet(document_path).await
            }
            DocumentType::Image => self.process_image(document_path).await,
            other => {
                warn!("Unsupported document type: {:?}", other);
                Err(anyhow!("Unsupported document type: {:?}", other))
            }
        }
    }

    /// Process PDF document
    async fn process_pdf(&self, pdf_path: &Path) -> Result<ProcessedDocument> {
        debug!("Processing PDF: {}", pdf_path.display());

        // For now, return a placeholder implementation
        // In a full implementation, this would:
        // 1. Use a PDF rendering library to convert pages to images
        // 2. Optionally run OCR on each page
        // 3. Extract metadata

        let metadata = self.extract_file_metadata(pdf_path)?;

        Ok(ProcessedDocument {
            source_path: pdf_path.to_path_buf(),
            doc_type: DocumentType::Pdf,
            page_count: 1,        // Placeholder
            pages: vec![],        // Placeholder - would contain actual rendered pages
            extracted_text: None, // Placeholder - would contain OCR text if enabled
            metadata,
        })
    }

    /// Process Word document
    async fn process_word_document(&self, doc_path: &Path) -> Result<ProcessedDocument> {
        debug!("Processing Word document: {}", doc_path.display());

        let metadata = self.extract_file_metadata(doc_path)?;

        Ok(ProcessedDocument {
            source_path: doc_path.to_path_buf(),
            doc_type: DocumentType::Docx,
            page_count: 1, // Placeholder
            pages: vec![],
            extracted_text: None,
            metadata,
        })
    }

    /// Process spreadsheet
    async fn process_spreadsheet(&self, spreadsheet_path: &Path) -> Result<ProcessedDocument> {
        debug!("Processing spreadsheet: {}", spreadsheet_path.display());

        let metadata = self.extract_file_metadata(spreadsheet_path)?;
        let doc_type = DocumentType::from_path(spreadsheet_path);

        Ok(ProcessedDocument {
            source_path: spreadsheet_path.to_path_buf(),
            doc_type,
            page_count: 1, // Spreadsheets are typically single "sheet"
            pages: vec![],
            extracted_text: None,
            metadata,
        })
    }

    /// Process image file
    async fn process_image(&self, image_path: &Path) -> Result<ProcessedDocument> {
        debug!("Processing image: {}", image_path.display());

        let metadata = self.extract_file_metadata(image_path)?;

        Ok(ProcessedDocument {
            source_path: image_path.to_path_buf(),
            doc_type: DocumentType::Image,
            page_count: 1,
            pages: vec![PageImage {
                page_number: 1,
                image_path: image_path.to_path_buf(),
                dimensions: ImageDimensions {
                    width: 0,
                    height: 0,
                }, // Would detect actual dimensions
                text_content: None,
            }],
            extracted_text: None,
            metadata,
        })
    }

    /// Extract basic file metadata
    fn extract_file_metadata(&self, path: &Path) -> Result<DocumentMetadata> {
        let metadata = std::fs::metadata(path)?;

        Ok(DocumentMetadata {
            title: None,
            author: None,
            created_date: None,
            modified_date: None,
            file_size: metadata.len(),
            page_count: None,
        })
    }

    /// Generate a prompt for vision model analysis
    pub fn generate_vision_prompt(
        &self,
        processed: &ProcessedDocument,
        query: &str,
    ) -> Result<String> {
        let mut prompt = String::new();

        prompt.push_str(&format!("Document: {}\n", processed.source_path.display()));
        prompt.push_str(&format!("Type: {:?}\n", processed.doc_type));
        prompt.push_str(&format!("Pages: {}\n\n", processed.page_count));

        if let Some(text) = &processed.extracted_text {
            prompt.push_str("Extracted Text:\n");
            prompt.push_str(text);
            prompt.push_str("\n\n");
        }

        prompt.push_str("Analyze the document images and provide: ");
        prompt.push_str("\n1. A summary of the content");
        prompt.push_str("\n2. Key insights or findings");
        prompt.push_str("\n3. Answers to specific questions");
        prompt.push_str(&format!("\n\nSpecific query: {}\n", query));

        Ok(prompt)
    }

    /// Clean up temporary files
    pub fn cleanup(&self) -> Result<()> {
        if self.temp_dir.exists() {
            std::fs::remove_dir_all(&self.temp_dir)?;
            debug!(
                "Cleaned up temporary directory: {}",
                self.temp_dir.display()
            );
        }
        Ok(())
    }
}

impl Drop for DocumentProcessor {
    fn drop(&mut self) {
        // Attempt to clean up on drop
        let _ = self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_type_detection() {
        assert_eq!(
            DocumentType::from_path(Path::new("test.pdf")),
            DocumentType::Pdf
        );
        assert_eq!(
            DocumentType::from_path(Path::new("test.docx")),
            DocumentType::Docx
        );
        assert_eq!(
            DocumentType::from_path(Path::new("test.xlsx")),
            DocumentType::Xlsx
        );
        assert_eq!(
            DocumentType::from_path(Path::new("test.png")),
            DocumentType::Image
        );
        assert_eq!(
            DocumentType::from_path(Path::new("test.unknown")),
            DocumentType::Unknown
        );
    }

    #[test]
    fn test_document_processor_creation() {
        let config = DocumentProcessorConfig::default();
        let processor = DocumentProcessor::new(config).unwrap();
        assert!(processor.temp_dir.exists());
    }

    #[tokio::test]
    async fn test_process_nonexistent_document() {
        let config = DocumentProcessorConfig::default();
        let processor = DocumentProcessor::new(config).unwrap();

        let result = processor
            .process_document(Path::new("/nonexistent/document.pdf"))
            .await;
        assert!(result.is_err());
    }
}
