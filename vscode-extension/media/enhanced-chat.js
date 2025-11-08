/**
 * Enhanced Chat View Client-Side Script
 *
 * Handles UI interactions, real-time updates, markdown rendering,
 * and message management for the VTCode chat interface.
 */

(function () {
	const vscode = acquireVsCodeApi();

	// State management
	let state = vscode.getState() || {
		messages: [],
		filter: null,
		searchQuery: "",
	};

	// DOM elements
	const transcriptContainer = document.getElementById("transcript-container");
	const messageInput = document.getElementById("message-input");
	const sendBtn = document.getElementById("send-btn");
	const clearInputBtn = document.getElementById("clear-input-btn");
	const searchBtn = document.getElementById("search-btn");
	const filterBtn = document.getElementById("filter-btn");
	const exportBtn = document.getElementById("export-btn");
	const archiveBtn = document.getElementById("archive-btn");
	const clearBtn = document.getElementById("clear-btn");
	const statsBtn = document.getElementById("stats-btn");
	const searchPanel = document.getElementById("search-panel");
	const searchInput = document.getElementById("search-input");
	const searchExecute = document.getElementById("search-execute");
	const searchClose = document.getElementById("search-close");
	const thinkingIndicator = document.getElementById("thinking-indicator");
	const charCount = document.getElementById("char-count");

	// Initialize
	init();

	function init() {
		setupEventListeners();
		restoreMessages();
		messageInput.focus();
		vscode.postMessage({ type: "ready" });
	}

	function setupEventListeners() {
		// Send message
		sendBtn.addEventListener("click", sendMessage);
		messageInput.addEventListener("keydown", (e) => {
			if (e.key === "Enter" && !e.shiftKey) {
				e.preventDefault();
				sendMessage();
			} else if (e.key === "k" && (e.ctrlKey || e.metaKey)) {
				e.preventDefault();
				clearInput();
			} else if (e.key === "l" && (e.ctrlKey || e.metaKey)) {
				e.preventDefault();
				clearTranscript();
			}
		});

		// Character count
		messageInput.addEventListener("input", () => {
			charCount.textContent = messageInput.value.length;
		});

		// Toolbar buttons
		clearInputBtn.addEventListener("click", clearInput);
		searchBtn.addEventListener("click", toggleSearchPanel);
		filterBtn.addEventListener("click", showFilterDialog);
		exportBtn.addEventListener("click", showExportDialog);
		archiveBtn.addEventListener("click", archiveTranscript);
		clearBtn.addEventListener("click", clearTranscript);
		statsBtn.addEventListener("click", showStats);

		// Search panel
		searchExecute.addEventListener("click", executeSearch);
		searchClose.addEventListener("click", () => {
			searchPanel.style.display = "none";
			clearFilter();
		});
		searchInput.addEventListener("keydown", (e) => {
			if (e.key === "Enter") {
				executeSearch();
			}
		});

		// Handle messages from extension
		window.addEventListener("message", handleMessage);
	}

	function handleMessage(event) {
		const message = event.data;

		switch (message.type) {
			case "addMessage":
				addMessage(message.message);
				break;

			case "restoreTranscript":
				state.messages = message.messages;
				saveState();
				renderAllMessages();
				break;

			case "clearTranscript":
				state.messages = [];
				state.filter = null;
				state.searchQuery = "";
				saveState();
				transcriptContainer.innerHTML = "";
				break;

			case "searchResults":
				displaySearchResults(message.results, message.query);
				break;

			case "filterResults":
				displayFilteredResults(message.results, message.filter);
				break;

			case "clearFilter":
				state.filter = null;
				state.searchQuery = "";
				saveState();
				renderAllMessages();
				break;

			case "thinking":
				thinkingIndicator.style.display = message.thinking ? "flex" : "none";
				break;

			case "deleteMessage":
				deleteMessageFromUI(message.messageId);
				break;

			case "updateMessage":
				updateMessageInUI(message.message);
				break;
		}
	}

	function sendMessage() {
		const text = messageInput.value.trim();
		if (!text) {
			return;
		}

		vscode.postMessage({
			type: "userMessage",
			text,
		});

		clearInput();
	}

	function clearInput() {
		messageInput.value = "";
		charCount.textContent = "0";
		messageInput.focus();
	}

	function addMessage(msg) {
		state.messages.push(msg);
		saveState();
		renderMessage(msg);
		scrollToBottom();
	}

	function renderMessage(msg) {
		const messageDiv = document.createElement("div");
		messageDiv.className = `message message-${msg.role}`;
		messageDiv.id = `msg-${msg.id}`;

		// Timestamp
		const timestamp = document.createElement("div");
		timestamp.className = "message-timestamp";
		timestamp.textContent = new Date(msg.timestamp).toLocaleString();

		// Role badge
		const role = document.createElement("span");
		role.className = "message-role";
		role.textContent = msg.role.toUpperCase();

		// Content (with markdown rendering)
		const content = document.createElement("div");
		content.className = "message-content";
		content.innerHTML = renderMarkdown(msg.content);

		// Metadata
		if (msg.metadata) {
			const metadata = document.createElement("div");
			metadata.className = "message-metadata";

			if (msg.metadata.model) {
				const model = document.createElement("span");
				model.className = "metadata-item";
				model.textContent = `Model: ${msg.metadata.model}`;
				metadata.appendChild(model);
			}

			if (msg.metadata.tokens) {
				const tokens = document.createElement("span");
				tokens.className = "metadata-item";
				tokens.textContent = `Tokens: ${msg.metadata.tokens.total}`;
				metadata.appendChild(tokens);
			}

			if (metadata.children.length > 0) {
				messageDiv.appendChild(metadata);
			}
		}

		// Actions
		const actions = document.createElement("div");
		actions.className = "message-actions";

		const copyBtn = createActionButton("📋", "Copy", () => {
			vscode.postMessage({ type: "copyMessage", messageId: msg.id });
		});

		const editBtn = createActionButton("✏️", "Edit", () => {
			editMessage(msg.id, msg.content);
		});

		const deleteBtn = createActionButton("🗑️", "Delete", () => {
			vscode.postMessage({ type: "deleteMessage", messageId: msg.id });
		});

		actions.appendChild(copyBtn);
		if (msg.role === "user" || msg.role === "assistant") {
			actions.appendChild(editBtn);
		}
		actions.appendChild(deleteBtn);

		if (msg.role === "assistant") {
			const regenerateBtn = createActionButton("🔄", "Regenerate", () => {
				vscode.postMessage({ type: "regenerateResponse", messageId: msg.id });
			});
			actions.appendChild(regenerateBtn);
		}

		messageDiv.appendChild(timestamp);
		messageDiv.appendChild(role);
		messageDiv.appendChild(content);
		messageDiv.appendChild(actions);

		transcriptContainer.appendChild(messageDiv);
	}

	function createActionButton(icon, title, onClick) {
		const btn = document.createElement("button");
		btn.className = "action-btn";
		btn.textContent = icon;
		btn.title = title;
		btn.addEventListener("click", onClick);
		return btn;
	}

	function renderAllMessages() {
		transcriptContainer.innerHTML = "";
		state.messages.forEach((msg) => renderMessage(msg));
		scrollToBottom();
	}

	function deleteMessageFromUI(messageId) {
		const msgElement = document.getElementById(`msg-${messageId}`);
		if (msgElement) {
			msgElement.remove();
		}
		state.messages = state.messages.filter((m) => m.id !== messageId);
		saveState();
	}

	function updateMessageInUI(msg) {
		const msgElement = document.getElementById(`msg-${msg.id}`);
		if (msgElement) {
			msgElement.remove();
		}
		const index = state.messages.findIndex((m) => m.id === msg.id);
		if (index !== -1) {
			state.messages[index] = msg;
		}
		renderMessage(msg);
		saveState();
	}

	function editMessage(messageId, currentContent) {
		const newContent = prompt("Edit message:", currentContent);
		if (newContent && newContent !== currentContent) {
			vscode.postMessage({
				type: "editMessage",
				messageId,
				newContent,
			});
		}
	}

	function toggleSearchPanel() {
		const isVisible = searchPanel.style.display !== "none";
		searchPanel.style.display = isVisible ? "none" : "flex";
		if (!isVisible) {
			searchInput.focus();
		}
	}

	function executeSearch() {
		const query = searchInput.value.trim();
		if (query) {
			state.searchQuery = query;
			saveState();
			vscode.postMessage({
				type: "searchTranscript",
				query,
			});
		}
	}

	function clearFilter() {
		vscode.postMessage({ type: "clearFilter" });
	}

	function displaySearchResults(results, query) {
		transcriptContainer.innerHTML = "";
		const header = document.createElement("div");
		header.className = "search-header";
		header.innerHTML = `<strong>Search Results</strong> for "${query}" (${results.length} found) <button id="clear-search">Clear</button>`;
		transcriptContainer.appendChild(header);

		document.getElementById("clear-search").addEventListener("click", clearFilter);

		results.forEach((msg) => renderMessage(msg));
		scrollToBottom();
	}

	function displayFilteredResults(results, _filter) {
		transcriptContainer.innerHTML = "";
		const header = document.createElement("div");
		header.className = "filter-header";
		header.innerHTML = `<strong>Filtered Results</strong> (${results.length} messages) <button id="clear-filter">Clear Filter</button>`;
		transcriptContainer.appendChild(header);

		document.getElementById("clear-filter").addEventListener("click", clearFilter);

		results.forEach((msg) => renderMessage(msg));
		scrollToBottom();
	}

	function showFilterDialog() {
		const role = prompt(
			"Filter by role (comma-separated):\nuser, assistant, system, tool\n\nLeave empty for all roles:"
		);

		if (role !== null) {
			const roles = role
				.split(",")
				.map((r) => r.trim())
				.filter((r) => r);

			const filter = {
				role: roles.length > 0 ? roles : undefined,
			};

			vscode.postMessage({
				type: "filterTranscript",
				filter,
			});
		}
	}

	function showExportDialog() {
		const format = prompt(
			"Export format:\njson, markdown, text, html\n\nEnter format:",
			"markdown"
		);

		if (format && ["json", "markdown", "text", "html"].includes(format.toLowerCase())) {
			vscode.postMessage({
				type: "exportTranscript",
				options: {
					format: format.toLowerCase(),
					includeMetadata: true,
					includeTimestamps: true,
				},
			});
		}
	}

	function archiveTranscript() {
		if (
			confirm(
				"Archive current transcript?\n\nThis will save the current session and clear the view."
			)
		) {
			vscode.postMessage({
				type: "clearTranscript",
				archive: true,
			});
		}
	}

	function clearTranscript() {
		if (
			confirm(
				"Clear transcript?\n\nThis will permanently delete all messages in the current session."
			)
		) {
			vscode.postMessage({
				type: "clearTranscript",
				archive: false,
			});
		}
	}

	function showStats() {
		vscode.postMessage({ type: "userMessage", text: "/stats" });
	}

	function restoreMessages() {
		if (state.messages && state.messages.length > 0) {
			renderAllMessages();
		}
	}

	function scrollToBottom() {
		setTimeout(() => {
			transcriptContainer.scrollTop = transcriptContainer.scrollHeight;
		}, 100);
	}

	function saveState() {
		vscode.setState(state);
	}

	/**
	 * Simple markdown renderer
	 * Supports: **bold**, *italic*, `code`, ```code blocks```, [links](url), # headers
	 */
	function renderMarkdown(text) {
		let html = text;

		// Escape HTML
		html = html
			.replace(/&/g, "&amp;")
			.replace(/</g, "&lt;")
			.replace(/>/g, "&gt;");

		// Code blocks
		html = html.replace(
			/```(\w+)?\n([\s\S]*?)```/g,
			(_, lang, code) =>
				`<pre><code class="language-${lang || "plaintext"}">${code.trim()}</code></pre>`
		);

		// Inline code
		html = html.replace(/`([^`]+)`/g, "<code>$1</code>");

		// Bold
		html = html.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");

		// Italic
		html = html.replace(/\*([^*]+)\*/g, "<em>$1</em>");

		// Links
		html = html.replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2">$1</a>');

		// Headers
		html = html.replace(/^### (.+)$/gm, "<h3>$1</h3>");
		html = html.replace(/^## (.+)$/gm, "<h2>$1</h2>");
		html = html.replace(/^# (.+)$/gm, "<h1>$1</h1>");

		// Line breaks
		html = html.replace(/\n/g, "<br>");

		return html;
	}
})();
