"""Unit tests for new_workspace_e2e.py pure helpers (no subprocesses)."""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import new_workspace_e2e as e2e


def test_infra_classifier_matches_auth_and_network_failures():
    assert e2e.INFRA.search("Error 401: invalid_api_key")
    assert e2e.INFRA.search("API key not found for provider 'xai'")
    assert e2e.INFRA.search("rate limit exceeded, retry later")
    assert e2e.INFRA.search("error sending request: connection refused")


def test_infra_classifier_ignores_task_outcomes():
    assert not e2e.INFRA.search("Hello Luna")
    assert not e2e.INFRA.search("test result: ok. 0 passed; 0 failed")
    assert not e2e.INFRA.search("Turn blocked: verification pending")


def test_prompt_markers_catch_denials_but_not_prose():
    assert e2e.PROMPT_MARKERS.search("Tool preflight validation failed: needsApproval")
    assert e2e.PROMPT_MARKERS.search("permission denied by policy")
    assert not e2e.PROMPT_MARKERS.search("Permissions: Auto-approve")
    assert not e2e.PROMPT_MARKERS.search("Hello Luna")
