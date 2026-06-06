big plan: extract and unify and build a centralize design-system cratte for all common base components and utilities, and then build the project on top of it. This will allow us to have a consistent design language across all our projects and also make it easier to maintain and update our components in the future.

--

check and fix @ file modal

When selecting a first-level file, the file isn't selected, but treated as a folder (expanded), and the file is selected only when clicking the file name, which is not intuitive. We should select the file when clicking anywhere on the file item, and expand the folder when clicking the expand icon.

When a top-level file is selected, the file should be selected in the TUI and dismiss the file modal, but currently it just expands the folder and doesn't select the file.

--

Try to find a way or use a library to handle markdown table responsiveness and text wrapping

example:

     Code layout — lives in vtcode-core/src/a2a/ with these modules:
     │ File            │ Role                                                   │
     ├─────────────────┼────────────────────────────────────────────────────────┤
     │ types.rs        │ Core structs: Task, Message, Part, Artifact, TaskState │
     │ rpc.rs          │ JSON-RPC 2.0 request/response framing                  │
     │ errors.rs       │ A2A and JSON-RPC error codes                           │
     │ agent_card.rs   │ Discovery metadata                                     │
     │ task_manager.rs │ In-memory task store (RwLock concurrency)              │
     │ server.rs       │ Axum HTTP server (feature-gated: a2a-server)           │
     │ client.rs       │ HTTP client for calling remote agents                  │
     │ webhook.rs      │ Webhook notifier for push events                       │

currently the table is not responsive and the text in the cells can overflow and break the layout. we need to find a way to make the table responsive and also handle text wrapping properly. this will improve the readability and usability of the table on different screen sizes.
