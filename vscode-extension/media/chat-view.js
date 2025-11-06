// Modern, optimized chat view with performance improvements
(function () {
	'use strict';

	const vscode = acquireVsCodeApi();

	// Cache DOM references
	const elements = {
		transcript: document.getElementById('transcript'),
		status: document.getElementById('status'),
		form: document.getElementById('composer'),
		input: document.getElementById('message'),
		sendBtn: document.getElementById('send'),
		cancelBtn: document.getElementById('cancel'),
		clearBtn: document.getElementById('clear')
	};
	if (elements.status) {
		elements.status.textContent = 'Ready';
	}

	// State
	let state = {
		messages: [],
		isStreaming: false,
		streamingBubble: null,
		reasoningBubble: null,
		toolStreamingBubble: null,
		rafId: null,
		pendingUpdate: null
	};

	// Cache icon HTML to avoid repeated string creation
	const ICONS = {
		copy: '<span class="codicon codicon-copy"></span>',
		check: '<span class="codicon codicon-check"></span>',
		refresh: '<span class="codicon codicon-refresh"></span>',
		terminal: '<span class="codicon codicon-terminal"></span>',
		shield: '<span class="codicon codicon-shield"></span>'
	};

	// Use Intl.DateTimeFormat for better date formatting (cached)
	const dateFormatter = new Intl.DateTimeFormat(undefined, {
		dateStyle: 'medium',
		timeStyle: 'short'
	});

	// Performance: Use DocumentFragment for batch DOM operations
	const createMessageElement = (entry) => {
		const fragment = document.createDocumentFragment();
		const wrapper = document.createElement('div');
		wrapper.className = 'chat-message-wrapper';

		const bubble = document.createElement('div');
		bubble.className = `chat-message chat-message--${entry.role}`;
		bubble.textContent = entry.content;

		if (entry.timestamp) {
			bubble.title = dateFormatter.format(new Date(entry.timestamp));
		}

		wrapper.appendChild(bubble);

		// Add metadata row with actions
		if (entry.role === 'assistant' || entry.role === 'tool' || entry.role === 'error') {
			const meta = document.createElement('div');
			meta.className = 'chat-message-meta';

			// Tool badge
			if (entry.role === 'tool') {
				const badge = document.createElement('span');
				badge.className = 'chat-tool-badge';
				const toolType = entry.metadata?.toolType === 'command' ? 'Terminal' : 'Tool';
				const toolName = entry.metadata?.tool ? String(entry.metadata.tool) : '';
				const label = toolName ? `${toolType}: ${toolName}` : toolType;
				badge.innerHTML = `${ICONS.terminal} ${label}`;
				meta.appendChild(badge);
			}

			// HITL badge
			if (entry.metadata?.humanApproved) {
				const badge = document.createElement('span');
				badge.className = 'chat-hitl-badge';
				badge.innerHTML = `${ICONS.shield} HITL`;
				meta.appendChild(badge);
			}

			// Actions
			const actions = document.createElement('div');
			actions.className = 'chat-message-actions';
			actions.style.marginLeft = 'auto';

			// Copy button
			if (entry.role === 'assistant' || entry.role === 'tool') {
				const copyBtn = createActionButton(ICONS.copy, 'Copy', () => {
					navigator.clipboard.writeText(entry.content).then(() => {
						copyBtn.innerHTML = ICONS.check;
						setTimeout(() => {
							copyBtn.innerHTML = ICONS.copy;
						}, 2000);
					}).catch(err => console.error('Copy failed:', err));
				});
				actions.appendChild(copyBtn);
			}

			// Retry button
			if (entry.role === 'error') {
				const retryBtn = createActionButton(ICONS.refresh, 'Retry', () => {
					vscode.postMessage({ type: 'retry' });
				});
				actions.appendChild(retryBtn);
			}

			meta.appendChild(actions);
			wrapper.appendChild(meta);
		}

		fragment.appendChild(wrapper);
		return fragment;
	};

	const createActionButton = (iconHTML, title, onClick) => {
		const btn = document.createElement('button');
		btn.className = 'chat-action-button';
		btn.innerHTML = iconHTML;
		btn.title = title;
		btn.addEventListener('click', onClick, { passive: true });
		return btn;
	};

	// Performance: Batch render with DocumentFragment
	const renderTranscript = () => {
		// Use replaceChildren for better performance than innerHTML
		elements.transcript.replaceChildren();

		if (state.messages.length === 0) {
			const empty = document.createElement('div');
			empty.className = 'chat-empty-state';
			empty.textContent = 'No messages yet';
			elements.transcript.appendChild(empty);
			return;
		}

		// Batch create all elements
		const fragment = document.createDocumentFragment();
		for (const entry of state.messages) {
			fragment.appendChild(createMessageElement(entry));
		}

		elements.transcript.appendChild(fragment);

		// Smooth scroll to bottom
		requestAnimationFrame(() => {
			elements.transcript.scrollTo({
				top: elements.transcript.scrollHeight,
				behavior: 'smooth'
			});
		});
	};

	const setThinking = (active) => {
		elements.status.textContent = active ? 'Synthesizing...' : 'Ready';
		state.isStreaming = active;
		updateButtonStates();
	};

	const updateButtonStates = () => {
		const { sendBtn, cancelBtn, input } = elements;

		if (state.isStreaming) {
			sendBtn.style.display = 'none';
			cancelBtn.style.display = 'inline-block';
			input.disabled = true;
			input.placeholder = 'Processing...';
		} else {
			sendBtn.style.display = 'inline-block';
			cancelBtn.style.display = 'none';
			input.disabled = false;
			input.placeholder = 'Ask VTCode...';
		}
	};

	// Optimized stream content update with RAF
	const updateStreamContent = (content) => {
		state.pendingUpdate = content;

		if (state.rafId) return;

		state.rafId = requestAnimationFrame(() => {
			if (state.streamingBubble && state.pendingUpdate !== null) {
				state.streamingBubble.textContent = state.pendingUpdate;
				elements.transcript.scrollTop = elements.transcript.scrollHeight;
			}
			state.rafId = null;
			state.pendingUpdate = null;
		});
	};

	const cleanupStreamingElements = () => {
		[state.streamingBubble, state.reasoningBubble, state.toolStreamingBubble].forEach(el => {
			el?.remove();
		});
		state.streamingBubble = null;
		state.reasoningBubble = null;
		state.toolStreamingBubble = null;

		const skeleton = elements.transcript.querySelector('.chat-skeleton');
		skeleton?.remove();
	};

	// Event delegation for better performance
	elements.transcript.addEventListener('click', (e) => {
		const button = e.target.closest('.chat-action-button');
		if (!button) return;

		// Button click handler is already attached
	}, { passive: true });

	// Form submission
	elements.form.addEventListener('submit', (e) => {
		e.preventDefault();
		const text = elements.input.value.trim();
		if (!text || state.isStreaming) return;

		vscode.postMessage({ type: 'sendMessage', text });
		elements.input.value = '';
		elements.input.focus();
	});

	// Keyboard shortcut
	elements.input.addEventListener('keydown', (e) => {
		if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
			e.preventDefault();
			elements.form.dispatchEvent(new Event('submit'));
		}
	});

	// Cancel button
	elements.cancelBtn.addEventListener('click', () => {
		vscode.postMessage({ type: 'cancel' });
		cleanupStreamingElements();
		state.isStreaming = false;
		updateButtonStates();
	});

	// Clear button
	elements.clearBtn.addEventListener('click', () => {
		vscode.postMessage({ type: 'clear' });
	});

	// Message handler
	window.addEventListener('message', (event) => {
		const { type } = event.data;
		const message = event.data;

		switch (type || message.type) {
			case 'transcript':
				state.messages = message.messages ?? [];
				state.streamingBubble = null;
				state.toolStreamingBubble = null;
				state.reasoningBubble = null;
				renderTranscript();
				break;

			case 'thinking':
				setThinking(Boolean(message.active));

				if (message.active && !state.streamingBubble) {
					const skeleton = document.createElement('div');
					skeleton.className = 'chat-skeleton';
					skeleton.innerHTML = `
						<div class="chat-skeleton-line"></div>
						<div class="chat-skeleton-line"></div>
						<div class="chat-skeleton-line"></div>
					`;
					elements.transcript.appendChild(skeleton);
					requestAnimationFrame(() => {
						elements.transcript.scrollTop = elements.transcript.scrollHeight;
					});
				} else if (!message.active) {
					elements.transcript.querySelector('.chat-skeleton')?.remove();
				}
				break;

			case 'stream':
				elements.transcript.querySelector('.chat-skeleton')?.remove();

				if (!state.streamingBubble) {
					state.streamingBubble = document.createElement('div');
					state.streamingBubble.className = 'chat-message chat-message--assistant';
					elements.transcript.appendChild(state.streamingBubble);
				}
				updateStreamContent(message.content);
				break;

			case 'reasoning':
				if (!state.reasoningBubble) {
					state.reasoningBubble = document.createElement('div');
					state.reasoningBubble.className = 'chat-reasoning';
					elements.transcript.appendChild(state.reasoningBubble);
				}
				state.reasoningBubble.textContent = message.content;
				requestAnimationFrame(() => {
					elements.transcript.scrollTop = elements.transcript.scrollHeight;
				});
				break;

			case 'toolStream':
				if (!state.toolStreamingBubble) {
					state.toolStreamingBubble = document.createElement('div');
					state.toolStreamingBubble.className = 'chat-message chat-message--tool';
					elements.transcript.appendChild(state.toolStreamingBubble);
				}
				state.toolStreamingBubble.textContent += (message.chunk?.replace(/\r/g, '') ?? '');
				requestAnimationFrame(() => {
					elements.transcript.scrollTop = elements.transcript.scrollHeight;
				});
				break;
		}
	});

	// Cleanup on unload
	window.addEventListener('beforeunload', () => {
		if (state.rafId) {
			cancelAnimationFrame(state.rafId);
		}
	});

	// Initial message
	vscode.postMessage({ type: 'ready' });
})();
