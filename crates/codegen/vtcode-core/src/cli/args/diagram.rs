use clap::{Args, Subcommand, ValueHint};
use std::path::PathBuf;

/// Render Mermaid diagrams to standalone SVG or PNG files, fully offline.
#[derive(Debug, Clone, Subcommand)]
pub enum DiagramCommand {
    /// Render one diagram to SVG or PNG through the vendored Pele bundle.
    Render(DiagramRenderArgs),
}

/// Render one Mermaid source to a standalone SVG or PNG file.
#[derive(Debug, Clone, Args)]
pub struct DiagramRenderArgs {
    /// Mermaid source (`.mmd`/`.mermaid`) or Markdown (first ```mermaid fence).
    #[arg(value_name = "INPUT", value_hint = ValueHint::FilePath)]
    pub input: PathBuf,
    /// Output path; defaults to the input path with an `.svg` extension. A
    /// `.png` extension selects 2x raster output instead.
    #[arg(long, short, value_name = "PATH", value_hint = ValueHint::FilePath)]
    pub output: Option<PathBuf>,
}
