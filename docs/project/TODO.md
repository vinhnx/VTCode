NOTE: use deepwiki mcp to reference from codex https://deepwiki.com/openai/codex

---

extract and open source more components from vtcode-core

---

CODEX plus

main account
kiweuro
writedownapp
humidapp
vtchat.io

--

build subagent

https://developers.openai.com/codex/subagents

https://deepwiki.com/search/subagents_6ec3f589-2e20-406a-a2df-03cb44654c9c

https://deepwiki.com/search/tell-me-more-about-subagent-ar_30cc4805-dbe7-492c-b834-5608cd1031ee

--> Improve and build and handle background subagent

https://deepwiki.com/search/how-do-manage-background-subag_75be134b-97cf-4175-a9c0-0db620bb50e6?mode=fast

Develop a feature that binds Ctrl+B (Control+B) to toggle a background subagent on and off and expose a Subprocesses panel in the left sidebar that lists and manages all background subprocesses. When enabled, spawn the subagent as a persistent background subprocess and ensure it remains running until toggled off or explicitly terminated; on disable, shut it down gracefully and perform necessary cleanup. The Subprocesses panel should display active background tasks with details such as Name, PID, Status, Start Time, and Uptime, and provide per-item actions to Kill (graceful termination) or Cancel (forceful termination) with user confirmations, error handling, and unobtrusive notifications. The panel must refresh automatically (or at a configurable interval) and support selecting a subprocess to inspect or act upon, including focus and quick-reload capabilities. Persist the on/off state and the current subprocesses across sessions via user preferences or a config file, so the state is restored on app restart. Use clear visual cues and accessible labels for statuses (running, stopped, error) and ensure color/iconography communicates state for quick scanning. Ensure cross-platform compatibility (Windows/macOS/Linux) and proper integration with the app’s architecture (e.g., main/renderer separation in Electron or equivalent), with robust error handling for spawn failures, unresponsive processes, and permission issues. Log events to a central app log and surface non-blocking user notifications on start/stop or errors. Include keyboard shortcuts for focusing the Subprocesses pane, refreshing the list, and sending termination signals. Provide thorough tests: unit tests for the toggle logic and lifecycle management, UI tests for the Subprocesses panel, and end-to-end tests validating the user flow from toggling the subagent to managing subprocesses. Update the project README with usage instructions, troubleshooting, and configuration guidance. Provide a minimal, reusable demonstration subagent script and a sample JSON/YAML config snippet illustrating feature enablement and the Ctrl+B binding.

ref: https://code.claude.com/docs/en/sub-agents#run-subagents-in-foreground-or-background

---

https://code.claude.com/docs/en/headless

--

fix theme switching now doesn't work, regression. when selecting a theme from the theme picker, presss enter. the theme is not applied.

---

improve /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/logs and /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/trajectory with ttl and cleanup old logs/trajectories

====

hooks

https://developers.openai.com/codex/hooks

https://deepwiki.com/search/how-does-hooks-works-in-codex_68383f0e-ec03-44eb-be92-69a26aa3d1e1?mode=fast

https://code.claude.com/docs/en/hooks

--

refactor /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-tui/src/ui/theme.rs
