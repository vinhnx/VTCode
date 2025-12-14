# Web Search Skill

A VTCode skill that integrates web search capabilities using the `curl` command to query search APIs.

## Overview

This skill provides web search functionality by making HTTP requests to search APIs. It's designed to be simple and flexible, working with various search providers.

## Features

- Web search via HTTP APIs
- JSON input/output support
- Configurable search parameters
- Error handling and validation
- Streaming results support

## Installation

1. Ensure `curl` is installed on your system
2. Place this directory in your VTCode skills directory
3. Configure your preferred search API in the tool configuration

## Configuration

Edit `tool.json` to configure:
- Search API endpoint
- API keys (if required)
- Default search parameters
- Timeout settings

## Usage

Once installed, you can use this skill through VTCode's interface:

```
Search the web for "rust programming language"
```

## API Support

By default, this skill is configured to work with public search APIs. You can customize it for:
- DuckDuckGo API
- Bing Search API
- Custom search endpoints

## Requirements

- `curl` command available in PATH
- Internet connectivity
- Valid search API access (if using private APIs)