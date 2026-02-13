[DON'T DELETE UNTIL FEEL COMPLETE] review duplicated and redundent logic from whole code base and remove and cleanup AND DRY.

continue with your recommendation, proceed with outcome. don't stop. review overall progress and changes again carefully, can you do better? go on don't ask me

--

Execute a comprehensive, line-by-line audit of the entire codebase to systematically identify and resolve optimization opportunities, prioritizing efficiency, scalability, and maintainability. Rigorously enforce the DRY (Don't Repeat Yourself) principle by detecting and eliminating all duplicated or redundant logic, consolidating patterns into reusable, modular components. Validate strict alignment between agent loops, tool calls, and system prompts, ensuring consistency in logic flow, error handling, and state management. Refactor the agent harness and core execution logic to enforce autonomous yet safe tool execution, incorporating robust validation, fallback mechanisms, and rate-limiting. Adhere to best practices regarding modular design, separation of concerns, and minimal dependency overhead. Exclude all non-code deliverables—such as summaries or documentation—and output only the fully optimized, refactored code.

---

implement user's share .json log from current session to debug issues faster.

maybe a quick way to share current session log as .json file for debugging purposes and copy/paste.

--

A few days ago, I became curious about Claude Code's Agent Teams feature: when multiple AI agents collaborate as a team, how do they communicate? WebSocket? gRPC? Some kind of inter-process message queue?

The answer surprised me—they read and write JSON files on disk. That's it.

What's even more interesting is the discovery process itself. I asked Claude Code, "How exactly is your teammate functionality implemented?" Without hesitation, it sent several agents to run the `strings` command on their own binary files, extracting function names, code snippets, and data structures from the 183MB Bundle compilation output. Then I reminded it—you already have this functionality, why not just use it and see? So it created an experimental team, spawned a teammate, and observed the complete communication process on the file system.

In other words: an AI first reverse-engineered its own source code, and then personally verified the reverse-engineered conclusions. This article is a summary of this process.

I. Conclusion

Claude Code's team system is built on three extremely simple primitives:

1. File system message queue – each agent has an inbox JSON file.

2. AsyncLocalStorage – Node.js's native asynchronous context isolation.

3. Shared task list – each task has a JSON file.

There's no message broker, no database, no network communication. Everything is a file.

II. How it was discovered

Claude Code is a single-unit binary compiled from a bundle, approximately 183MB, located here:

~/.local/share/claude/versions/2.1.39

I asked its teammate how it was implemented, and they directly had agents run `strings` on their own binary – extracting readable strings from the compilation output. Although variable names were obfuscated (e.g., N$, i7, c8), function names and error messages remained.

It found these key function names:

`injectUserMessageToTeammate` ← Injects a message as a user message

`readUnreadMessages` ← Reads unread messages

`formatTeammateMessages` ← Formats teammate messages

`waitForTeammatesToBecomeIdle` ← Waits for a teammate to become idle

`isInProcessTeammate` ← Checks if a teammate is in-process

It also uncovered the core code for context management, using Node.js's `AsyncLocalStorage`:

`var T7A = new AsyncLocalStorage();`

`function getTeammateContext() { return T7A.getStore(); }`

`function runWithTeammateContext(ctx, fn) { return T7A.run

(ctx, fn); }`

However, reverse engineering can only tell you what the code looks like, not what traces it leaves on the disk during runtime. So I had it do something more direct—

III. Experiment: Create a team and observe the file system

Next, it directly called `TeamCreate` to create a team called "Exploration Experiment," spawned a teammate, had them send messages to each other, and then checked the disk step by step to see what happened.

【Step 1: Creating a Team】

After calling `TeamCreate`, two directories appear simultaneously:

~/.claude/teams/----/config.json ← Team configuration

~/.claude/tasks/----/ ← Task list

(Chinese team names are sanitized into "----", and all non-alphanumeric characters are hyphenated.)

The content of `config.json` is straightforward:

{
"name": "Exploration Experiment",

"leadAgentId": "team-lead@Exploration Experiment",

"members": [
{
"name": "team-lead",

"model": "claude-opus-4-6",

"backendType": "in-process"

}

]

}

It's just a list of members. The team's "existence" is entirely defined by this file.

【Step Two: Creating Tasks】

Calling `TaskCreate` creates a new file:

~/.claude/tasks/----/1.json

Content:

{
"id": "1",

"subject": "Check if the inbox file appears",

"status": "pending",

"blocks": [],

"blockedBy": []

}

Each task is an incrementing numbered JSON file. The `blockedBy` array is the only dependency management mechanism.

【Step Three: Spawning a Teammate】

It creates a teammate named `observer`, which sends messages to the lead (which is itself).

After spawning, `config.json` is immediately updated with a new member:

{
"name": "observer",

"agentType": "general-purpose",

"model": "haiku",

"color": "blue",

"backendType": "in-process"

}
Simultaneously, a new directory appears:

~/.claude/teams/----/inboxes/

This is the core of the communication.

【Step 4: Check the inbox】

After the observer sends a message, check the inboxes directory:

inboxes/

└── team-lead.json

Open team-lead.json:

[
{
"from": "observer",

"text": "Hello lead, I am the observer, I have started!",

"summary": "Observer reporting in",

"timestamp": "2026-02-12T09:21:46.491Z",

"color": "blue",

"read": true

}

]

It's a JSON array. Each message is appended to the end of the array.

Note that at this point, only team-lead.json is in inboxes/, not observer.json—because no one has sent a message to the observer yet.

The `observer.json` file only appears after the lead sends a message to the observer:

inboxes/

├── team-lead.json

└── observer.json

The inbox files are created on demand, not pre-allocated.

IV. Protocol Messages: JSON within JSON

The `text` field of ordinary conversation messages is plain text. However, system-level protocol messages—such as idle notifications and close requests—serialize JSON into a string and insert it into the `text` field.

For example, an idle notification automatically sent by the observer after completing their work:

{
"from": "observer",

"text": "{\"type\":\"idle_notification\",\"from\":\"observer\",\"idleReason\":\"available\"}",

"timestamp": "...",

"color": "blue",

"read": true

}

The `text` field contains a JSON string. The receiver needs to parse the `text`, check the `type` field, and then distribute and process it.

Complete message timeline (lead's inbox):

Message 1: Regular DM "Hello lead, I'm the observer, I've started!"

Message 2: Regular DM "Task list report: Currently there is 1 task..."

Message 3: Protocol message idle_notification (idleReason: available)

Message 4: Protocol message idle_notification (idle became idle again after receiving a reply from the lead)

Message 5: Protocol message shutdown_approved (shutdown approved)

And in the observer's inbox:

Message 1: Regular DM test message from the lead

Message 2: Protocol message shutdown_request (lead requested shutdown)

The entire lifecycle is contained in these two JSON files.

V. How are messages read by the agent?

This is the most crucial part.

The function name injectedUserMessageToTeammate, extracted from the binary, directly reveals the mechanism: teammate messages are injected as user messages.

In other words, for the receiving agent, a message from a teammate and a message from a human user have the same place in the conversation history. The only difference is that it's wrapped in a certain format (the specific wrapping template wasn't found in the binary; it's likely assembled at runtime).

The timing of delivery is also crucial: only between conversation turns.

One Claude API call equals one turn. The agent receives input → thinks → invokes the tool → returns the result—this is one turn. Only after a turn is fully completed will the system check the inbox for new messages.

This means that if an agent is executing a long turn (e.g., writing a lot of code), messages received during that time won't be processed in real-time. They must wait for the current turn to finish.

This feature even led to a bug (GitHub #24108): in tmux mode, newly spawned teammates would get stuck on the initial welcome screen after startup, never having completed their first turn, so they would never start polling the inbox, causing the entire agent to freeze.

VI. Two Running Modes

Teammate has two backendTypes:

in-process: Uses AsyncLocalStorage to isolate the context within the main process.

tmux: Runs a completely independent process within a separate tmux pane.

The default is in-process. Both share the same inbox file communication mechanism, but the differences are:

- In-process terminates using AbortController.abort()

- tmux terminates using process.exit()

- In-process offers better performance, but a crash may affect the main process.

- tmux provides greater isolation, but has the polling startup bug mentioned above.

VII. Known Issues

Several known issues have been verified through GitHub issues, all of which are still open:

#23620 — Context compaction kills team awareness

Long tasks run, and the lead's context window fills up, triggering automatic compaction.

After compaction, the lead completely forgets about the team's existence.

Messages cannot be sent, tasks cannot be coordinated, it's as if the team has vanished.

The community developed the Cozempic tool to mitigate this: it automatically reads team state from

config.json and re-injects it after compression.

However, the official PostCompact hook is still missing.

#25131 — Catastrophic agent lifecycle failure

Duplicate spawning, wasted mailbox polling, and chaotic lifecycle management.

#24130 — Auto memory files do not support concurrency

Multiple teammate writes to MEMORY.md simultaneously

Overwriting each other.

#24977 —Task completion notifications overwhelm the context

Each TaskUpdate leaves a trace in the lead's context,

accelerating compaction issues.

#23629 — Task state inconsistency

The task state at the team level may differ from the state within each agent session.

VIII. File System as a Message Queue

You might be thinking: Isn't this just a message queue?

Yes. It's a message queue implemented on the file system. And the implementation is surprisingly natural.

Think about the core abstractions of a message queue: producers write, consumers read, messages are persisted, and multiple independent channels are supported.

inboxes/team-lead.json = one channel

inboxes/observer.json = another channel

JSON array append = enqueue

readUnreadMessages() = dequeue

"read": true = ack

You don't need to install RabbitMQ. You don't need to run Redis. You don't need any additional processes. The file system itself is persistent storage, and the operating system's file API is your queue interface.

The appeal of this choice lies in its virtually zero deployment cost. Claude Code is a CLI tool; users can simply install it with `npm install`. If communication relies on a message queue, users would have to install Redis and start a daemon, which is clearly too cumbersome for a command-line tool.

A file system, on the other hand, exists on every operating system, requiring no installation, configuration, ports, or permissions. `mkdir` and `writeFile` are all you need.

This also brings a very useful byproduct: complete observability. You can `cat` an inbox file at any time to see the entire message history. Problems? `ls` the `teams` directory to see the current status. No dedicated monitoring panel or log aggregator is needed; the file system itself is your debugging tool.

Of course, using a file system as a message queue isn't without its costs. It lacks true atomicity—two processes writing to the same inbox simultaneously can cause problems (although in in-process mode, the shared event loop generally prevents this). It lacks real-time push—consumers must actively poll. It lacks backpressure—the inbox file can grow indefinitely.

However, these "drawbacks" are acceptable in this scenario. The message volume between agents is small (typically only a few dozen messages per team), latency requirements are low (one poll between turns is sufficient), and concurrency is limited (a team typically has 2-4 agents).

Under these constraints, a file system is an extremely reasonable choice. It pushes the complexity to the operating system—a more mature and reliable infrastructure than any message broker.

IX. Costs

Simplicity comes with its own costs. This system currently has several structural limitations:

- Lack of real-time performance: Messages can only be delivered between turns. Agents writing code may not receive messages.

- Lack of synchronous waiting: `await teammate.confirm()` is not supported. If you send a question,

the agent won't stop to wait for a reply; it will either continue doing something else or enter an idle state.

- Lack of context reset: The teammate's context window only increases, never decreases. After completing task A,

when starting task B, all residual information from A remains until compression is triggered, and compression is lossy.

- Concurrency safety relies primarily on gentleman's agreements: .lock files exist, but they are not strict mutexes.

This is like multithreaded programming without locks—you have to design task boundaries and dependencies very carefully, otherwise race conditions and inconsistencies will occur.

A statement repeatedly emphasized by the community is very apt:

"You need to manage your agent team like a good tech lead."

Because the system itself won't cover for you.

X. Summary

Claude Code's Agent Teams is a very early but already usable multi-agent collaboration framework.

It made a decision I think is very smart: it didn't invent anything new. The file system is the oldest "database," JSON is the most universal serialization format, and AsyncLocalStorage is a built-in isolation primitive in Node.js. Combining these three things gives you a multi-agent communication system. Nothing requires additional learning or installation.

Its biggest advantage isn't advanced orchestration capabilities, but rather "you can always open ~/.claude/teams/ to see everything." Every message, every task, every member's information is there, plain text, freely viewable. It's fair to say it has taken observability to the extreme—not because of added monitoring, but because nothing is truly hidden.

Its current limitations are also obvious. Issues on GitHub such as context compaction killing team awareness (#23620) and chaotic agent lifecycle management (#25131) are all open. These aren't minor bugs; they represent structural challenges at the architectural level.

However, as a multi-agent system within a CLI tool, I think this starting point is excellent. Start with the simplest method, let real users encounter real problems, and then decide where to increase complexity. Compared to building a sophisticated distributed messaging system from the outset, this "make do with files" approach is far less risky.

After all, file systems have been around for 40 years and haven't broken down yet.

————————————————————————————————————————

Appendix: Quick Verification Method

If you want to see it for yourself:

1. Create a team in Claude Code.

2. Use `ls -laR ~/.claude/teams/` to observe file changes.

3. Spawn a teammate to send you messages.

4. `cat ~/.claude/teams/your_team_name/inboxes/team-lead.json`

5. You will see all messages, including protocol messages.

The entire communication history is contained in those few JSON files.
