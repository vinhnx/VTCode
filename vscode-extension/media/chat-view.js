// VTCode Chat View Client-Side Script

(function () {
	try {
		const vscode = acquireVsCodeApi();
		let state = vscode.getState() || { messages: [] };

		// DOM elements
		const transcriptContainer = document.getElementById("transcript-container");
		const userInput = document.getElementById("user-input");
		const sendButton = document.getElementById("send-button");
		const clearButton = document.getElementById("clear-button");
		const cancelButton = document.getElementById("cancel-button");
		const thinkingIndicator = document.getElementById("thinking-indicator");
		const approvalPanel = document.getElementById("approval-panel");

		// Check if all elements are found
		if (!transcriptContainer || !userInput || !sendButton || !clearButton || !cancelButton) {
			console.error("Chat view: Required DOM elements not found", {
				transcriptContainer: !!transcriptContainer,
				userInput: !!userInput,
				sendButton: !!sendButton,
				clearButton: !!clearButton,
				cancelButton: !!cancelButton,
			});
			return;
		}

		// Initialize
		init();

		function init() {
			setupEventListeners();
			renderMessages(state.messages);
			showEmptyStateIfNeeded();
			if (userInput) {
				userInput.focus();
			}
		}

	function setupEventListeners() {
		sendButton.addEventListener("click", handleSend);
		clearButton.addEventListener("click", handleClear);
		cancelButton.addEventListener("click", handleCancel);

		userInput.addEventListener("keydown", (e) => {
			if (e.key === "Enter" && !e.shiftKey) {
				e.preventDefault();
				handleSend();
			}
		});

		// Handle messages from extension
		window.addEventListener("message", handleExtensionMessage);
	}

	function handleSend() {
		const text = userInput.value.trim();
		if (!text) {
			return;
		}

		vscode.postMessage({
			type: "userMessage",
			text,
		});

		userInput.value = "";
		userInput.focus();
	}

	function handleClear() {
		if (confirm("Clear all transcript? This cannot be undone.")) {
			vscode.postMessage({
				type: "clearTranscript",
			});
		}
	}

	function handleCancel() {
		vscode.postMessage({
			type: "cancelOperation",
		});
	}

	function handleExtensionMessage(event) {
		const message = event.data;

		switch (message.type) {
			case "addMessage":
				addMessage(message.message);
				break;

			case "clearTranscript":
				clearTranscript();
				break;

			case "thinking":
				setThinking(message.thinking);
				break;

			case "requestToolApproval":
				showToolApproval(message.toolCall);
				break;
		}
	}

	function addMessage(message) {
		state.messages.push(message);
		vscode.setState(state);

		const messageEl = createMessageElement(message);
		transcriptContainer.appendChild(messageEl);
		scrollToBottom();
		hideEmptyState();
	}

	function createMessageElement(message) {
		const messageEl = document.createElement("div");
		messageEl.className = `message message-${message.role}`;
		messageEl.dataset.id = message.id;

		// Add error/warning class for system messages
		if (message.role === "system" && message.content.toLowerCase().includes("error")) {
			messageEl.classList.add("error");
		} else if (message.role === "system" && message.content.toLowerCase().includes("warning")) {
			messageEl.classList.add("warning");
		}

		// Header
		const header = document.createElement("div");
		header.className = "message-header";

		const roleSpan = document.createElement("span");
		roleSpan.className = "message-role";
		roleSpan.textContent = getRoleDisplayName(message.role);

		const timestampSpan = document.createElement("span");
		timestampSpan.className = "message-timestamp";
		timestampSpan.textContent = formatTimestamp(message.timestamp);

		header.appendChild(roleSpan);
		header.appendChild(timestampSpan);
		messageEl.appendChild(header);

		// Content
		const content = document.createElement("div");
		content.className = "message-content";
		content.textContent = message.content;
		messageEl.appendChild(content);

		// Metadata
		if (message.metadata) {
			const metadata = document.createElement("div");
			metadata.className = "message-metadata";

			if (message.metadata.reasoning) {
				const reasoning = document.createElement("div");
				reasoning.innerHTML = `<strong>Reasoning:</strong> ${escapeHtml(message.metadata.reasoning)}`;
				metadata.appendChild(reasoning);
			}

			if (message.metadata.toolCall) {
				const toolCall = document.createElement("div");
				toolCall.className = "tool-call";
				toolCall.innerHTML = `
					<strong>Tool:</strong> ${escapeHtml(message.metadata.toolCall.name)}<br>
					<strong>Arguments:</strong> ${escapeHtml(JSON.stringify(message.metadata.toolCall.arguments, null, 2))}
				`;
				metadata.appendChild(toolCall);
			}

			if (message.metadata.toolResult) {
				const toolResult = document.createElement("div");
				toolResult.className = "tool-result";
				if (message.metadata.toolResult.error) {
					toolResult.classList.add("error");
				}

				let resultHtml = `<strong>Result:</strong><br>`;
				if (message.metadata.toolResult.error) {
					resultHtml += `<span style="color: var(--vscode-errorForeground);">Error: ${escapeHtml(message.metadata.toolResult.error)}</span>`;
				} else {
					resultHtml += `<pre><code>${escapeHtml(JSON.stringify(message.metadata.toolResult.result, null, 2))}</code></pre>`;
				}

				if (message.metadata.toolResult.executionTimeMs) {
					resultHtml += `<br><small>Executed in ${message.metadata.toolResult.executionTimeMs}ms</small>`;
				}

				toolResult.innerHTML = resultHtml;
				metadata.appendChild(toolResult);
			}

			messageEl.appendChild(metadata);
		}

		return messageEl;
	}

	function getRoleDisplayName(role) {
		const names = {
			user: "You",
			assistant: "Agent",
			system: "System",
			tool: "Tool",
		};
		return names[role] || role;
	}

	function formatTimestamp(timestamp) {
		const date = new Date(timestamp);
		return date.toLocaleTimeString();
	}

	function escapeHtml(text) {
		const div = document.createElement("div");
		div.textContent = text;
		return div.innerHTML;
	}

	function clearTranscript() {
		state.messages = [];
		vscode.setState(state);
		transcriptContainer.innerHTML = "";
		showEmptyStateIfNeeded();
	}

	function renderMessages(messages) {
		transcriptContainer.innerHTML = "";
		messages.forEach((message) => {
			const messageEl = createMessageElement(message);
			transcriptContainer.appendChild(messageEl);
		});
		scrollToBottom();
	}

	function scrollToBottom() {
		transcriptContainer.scrollTop = transcriptContainer.scrollHeight;
	}

	function setThinking(thinking) {
		thinkingIndicator.style.display = thinking ? "block" : "none";
		sendButton.disabled = thinking;
		if (thinking) {
			scrollToBottom();
		}
	}

	function showToolApproval(toolCall) {
		approvalPanel.style.display = "block";

		approvalPanel.innerHTML = `
			<div class="approval-header">🔧 Tool Approval Required</div>
			<div class="approval-tool-info">
				<strong>Tool:</strong> ${escapeHtml(toolCall.name)}<br>
				<strong>Arguments:</strong><br>
				<pre><code>${escapeHtml(JSON.stringify(toolCall.arguments, null, 2))}</code></pre>
			</div>
			<div class="approval-buttons">
				<button id="approve-button">✓ Approve</button>
				<button id="reject-button">✗ Reject</button>
			</div>
		`;

		document.getElementById("approve-button").addEventListener("click", () => {
			vscode.postMessage({
				type: "toolApproval",
				toolId: toolCall.id,
				approved: true,
			});
			approvalPanel.style.display = "none";
		});

		document.getElementById("reject-button").addEventListener("click", () => {
			vscode.postMessage({
				type: "toolApproval",
				toolId: toolCall.id,
				approved: false,
			});
			approvalPanel.style.display = "none";
		});
	}

	function showEmptyStateIfNeeded() {
		if (state.messages.length === 0) {
			transcriptContainer.innerHTML = `
				<div class="empty-state">
					<h3>🤖 VTCode Chat</h3>
					<p>Start a conversation with your AI coding assistant!</p>
					<p>
						<span class="command-hint">/</span> System commands<br>
						<span class="command-hint">@</span> Agent commands<br>
						<span class="command-hint">#</span> Tool commands
					</p>
					<p>Type <strong>/help</strong> to see all available commands.</p>
				</div>
			`;
		}
	}

	function hideEmptyState() {
		const emptyState = transcriptContainer.querySelector(".empty-state");
		if (emptyState) {
			emptyState.remove();
		}
	}
	} catch (error) {
		console.error("Chat view initialization error:", error);
		document.body.innerHTML = `
			<div style="padding: 20px; color: var(--vscode-errorForeground);">
				<h3>❌ Chat View Error</h3>
				<p>Failed to initialize chat view: ${error.message}</p>
				<p>Please reload the window or check the developer console.</p>
			</div>
		`;
	}
})();
