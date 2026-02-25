#!/usr/bin/env python3
import os
import re
from pathlib import Path

# Mapping of directory names to category titles
CATEGORY_MAPPING = {
    "user-guide": "Getting Started & Overview",
    "security": "Security & Safety",
    "providers": "LLM Providers & Models",
    "config": "Configuration & Customization",
    "performance": "Performance & Optimization",
    "benchmarks": "Performance & Optimization",
    "research": "Performance & Optimization",
    "development": "Development & Testing",
    "guides": "Integrations & Tooling",
    "mcp": "Integrations & Tooling",
    "ide": "Editor Integrations",
    "subagents": "Advanced Features & Research",
    "tools": "Tools & Functionality",
    "modules": "Modules & Implementation",
}

# Explicit file to category mapping for files in root docs/ or misfits
FILE_CATEGORY_OVERRIDE = {
    "docs/ARCHITECTURE.md": "Getting Started & Overview",
    "docs/models.json": "LLM Providers & Models",
    "docs/user-guide/commands.md": "User Workflows & Commands",
    "docs/user-guide/interactive-mode.md": "User Workflows & Commands",
}

# Files to exclude from the map
EXCLUDE_FILES = [
    "docs/modules/vtcode_docs_map.md",
    "docs/README.md",
    "docs/INDEX.md",
    "docs/CONTRIBUTING.md",
    "docs/CODE_OF_CONDUCT.md",
    "docs/FAQ.md",
]

# Directories to exclude from the map
EXCLUDE_DIRS = [
    "docs/mcp/archive",
    "docs/async",
    "docs/vscode-extension-improve-docs",
    "docs/bugs",
    "docs/experimental",
    "docs/research", # Some research is ok but some might be too much, I'll keep it for now but let's see
]

# Patterns to exclude from topics
EXCLUDE_TOPIC_PATTERNS = [
    r"Common Issues",
    r"Getting Help",
    r"Next Steps",
    r"Learn More",
    r"Prerequisites",
    r"Installation",
    r"Quick Start",
]

def extract_metadata(file_path):
    """Extract title and topics from a markdown file."""
    content = ""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
    except Exception:
        return None, None

    # Try to find the H1 title
    title_match = re.search(r'^#\s+(.+)$', content, re.MULTILINE)
    title = title_match.group(1).strip() if title_match else file_path.name

    # Try to find H2 headers for topics
    topics = []
    for match in re.finditer(r'^##\s+(.+)$', content, re.MULTILINE):
        topic = match.group(1).strip()
        if not any(re.search(pattern, topic, re.IGNORECASE) for pattern in EXCLUDE_TOPIC_PATTERNS):
            topics.append(topic)

    # If it's a JSON file, handle differently
    if file_path.suffix == '.json':
        title = f"{file_path.name} Metadata"
        topics = ["Model Specifications", "Capabilities", "Context Limits"]

    return title, topics

def generate_questions(title, topics):
    """Generate sample user questions based on title and topics."""
    questions = []
    # Escape quotes for the markdown list
    safe_title = title.replace('"', '\\"')
    
    # Add title-based question
    questions.append(f'"What can you tell me about {safe_title}?"')
    
    # Add topic-based questions
    for topic in topics[:2]:
        safe_topic = topic.replace('"', '\\"')
        questions.append(f'"How does {safe_topic} work?"')
    
    return questions

def main():
    root_dir = Path(__file__).parent.parent
    docs_dir = root_dir / "docs"
    output_file = docs_dir / "modules" / "vtcode_docs_map.md"

    categories = {}

    # Scan for markdown and specific json files
    for ext in ['*.md', 'models.json']:
        for file_path in docs_dir.rglob(ext):
            rel_path = file_path.relative_to(root_dir)
            rel_path_str = str(rel_path)

            if rel_path_str in EXCLUDE_FILES:
                continue
            
            if any(rel_path_str.startswith(d) for d in EXCLUDE_DIRS):
                continue

            # Determine category
            category = "Other"
            if rel_path_str in FILE_CATEGORY_OVERRIDE:
                category = FILE_CATEGORY_OVERRIDE[rel_path_str]
            else:
                parent_name = file_path.parent.name
                if parent_name in CATEGORY_MAPPING:
                    category = CATEGORY_MAPPING[parent_name]
                elif file_path.parent == docs_dir:
                    category = "Getting Started & Overview"

            if category not in categories:
                categories[category] = []

            title, topics = extract_metadata(file_path)
            if title:
                categories[category].append({
                    "path": rel_path_str,
                    "title": title,
                    "topics": topics,
                    "questions": generate_questions(title, topics)
                })

    # Sort categories and files within categories
    sorted_categories = sorted(categories.keys())
    for cat in sorted_categories:
        categories[cat].sort(key=lambda x: x['title'])

    # Build the output content
    content = [
        "# VT Code Documentation Map",
        "",
        "This document serves as an index of all VT Code documentation. When users ask questions about VT Code itself (capabilities, features, configuration, etc.), this file provides the complete catalog of available documentation sources.",
        "",
        "## Quick Reference",
        "",
        "**Core Questions**: Can VT Code do X? | How does VT Code Y work? | What's VT Code's Z feature?",
        "",
        "**Documentation Retrieval**: When users ask about VT Code capabilities, fetch relevant sections from the files listed below based on the topic area.",
        "",
        "## Documentation Categories",
        ""
    ]

    for category in sorted_categories:
        content.append(f"### {category}")
        content.append("")
        for doc in categories[category]:
            content.append(f"- **File**: `{doc['path']}`")
            content.append(f"  - **Content**: {doc['title']}")
            if doc['topics']:
                content.append(f"  - **Topics**: {', '.join(doc['topics'][:5])}")
            if doc['questions']:
                content.append(f"  - **User Questions**: {', '.join(doc['questions'])}")
            content.append("")

    # Add the manual sections back
    content.extend([
        "## Enhanced Trigger Questions",
        "",
        "### Core Capabilities & Features",
        "- \"What can VT Code do?\"",
        "- \"What are VT Code's main features?\"",
        "- \"How does VT Code compare to other AI coding tools?\"",
        "- \"What makes VT Code unique?\"",
        "- \"Can VT Code handle multiple programming languages?\"",
        "- \"Does VT Code support real-time collaboration?\"",
        "- \"How does VT Code handle large codebases efficiently?\"",
        "- \"What are the different system prompt modes (minimal, lightweight, etc.)?\"",
        "- \"How can I reduce token usage with tool documentation modes?\"",
        "",
        "### Workflows & Agent Behavior",
        "- \"What is Plan Mode and how do I use it?\"",
        "- \"How do I use the @ symbol to reference files in my messages?\"",
        "- \"What are agent teams and how do they work?\"",
        "- \"How can I delegate tasks to specialized subagents like the code-reviewer?\"",
        "- \"How do I use the /files slash command to browse my workspace?\"",
        "- \"What is the Decision Ledger and how does it help with coherence?\"",
        "- \"How does the agent handle long-running conversations?\"",
        "",
        "### Security & Reliability",
        "- \"What security layers does VT Code implement?\"",
        "- \"How does VT Code ensure shell command safety?\"",
        "- \"What is the 5-layer security model in VT Code?\"",
        "- \"How do tool policies and human-in-the-loop approvals work?\"",
        "- \"How does the circuit breaker prevent cascading failures?\"",
        "- \"Is my code and data safe with VT Code?\"",
        "",
        "### Integrations & Protocols",
        "- \"How do I use VT Code inside the Zed editor?\"",
        "- \"What is the Agent Client Protocol (ACP) and how is it used?\"",
        "- \"What is the Agent2Agent (A2A) protocol?\"",
        "- \"How does VT Code conform to the Open Responses specification?\"",
        "- \"How do I configure Model Context Protocol (MCP) servers?\"",
        "- \"What are lifecycle hooks and how do I configure them?\"",
        "",
        "### Local Models & Providers",
        "- \"Can I use VT Code with local models via Ollama?\"",
        "- \"How do I integrate VT Code with LM Studio?\"",
        "- \"Which AI providers are supported (OpenAI, Anthropic, Gemini, etc.)?\"",
        "- \"How do I set up OpenRouter with VT Code?\"",
        "- \"How can I use Hugging Face Inference Providers?\"",
        "",
        "### Getting Started & Setup",
        "- \"How do I install VT Code?\"",
        "- \"How do I get started with VT Code?\"",
        "- \"How do I set up VT Code for the first time?\"",
        "- \"What do I need to get started?\"",
        "- \"How do I configure API keys?\"",
        "- \"Which LLM provider should I choose?\"",
        "- \"How do I configure VT Code for my workflow?\"",
        "- \"What are the most common keyboard shortcuts?\"",
        "",
        "### Development & Maintenance",
        "- \"How do I build VT Code from source?\"",
        "- \"How do I run the test suite?\"",
        "- \"How do I add a new tool to VT Code?\"",
        "- \"How do I debug agent behavior or tool execution?\"",
        "- \"How do I run the performance benchmarks?\"",
        "- \"How do I update the self-documentation map?\"",
        "- \"How do I contribute to the VT Code project?\"",
        "- \"What is the release process for VT Code?\"",
        "- \"How do I manage multi-crate dependencies in this workspace?\"",
        "",
        "## VT Code Feature Categories",
        "",
        "### Core Capabilities",
        "- **Multi-LLM Provider Support**: OpenAI, Anthropic, Google, DeepSeek, xAI, OpenRouter, Moonshot AI, Ollama, LM Studio",
        "- **Terminal Interface**: Modern TUI with mouse support, text selection, and streaming output",
        "- **Workspace Management**: Automatic project indexing, fuzzy file discovery, and context curation",
        "- **Tool System**: Modular, extensible tool architecture with 53+ specialized tools",
        "- **Security**: Enterprise-grade safety with tree-sitter-bash validation, sandboxing, and policy controls",
        "- **Agent Protocols**: Support for ACP, A2A, and Open Responses for cross-tool interoperability",
        "",
        "## Additional Resources",
        "",
        "### External Documentation",
        "- **Repository**: https://github.com/vinhnx/vtcode",
        "- **Crate**: https://crates.io/crates/vtcode",
        "- **VS Code Extension**: Open VSX and VS Code Marketplace",
        "",
        "---",
        "",
        "**Note**: This enhanced documentation map is designed for VT Code's self-documentation system. When users ask questions about VT Code itself, the system should fetch this document and use it to provide accurate, up-to-date information about VT Code's capabilities and features."
    ])

    with open(output_file, 'w', encoding='utf-8') as f:
        f.write('\n'.join(content) + '\n')

    print(f"Generated {output_file}")

if __name__ == "__main__":
    main()
