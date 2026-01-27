there seems to be duplication for agent' `reasoning` and `content` output.

eg:

"
The load_skill_resource returned JSON with content that appears to be truncated. Let me read the full con
tent using read_file with start_line and end_line to get more context.

The load_skill_resource returned JSON with content that appears to be truncated. Let me read the full co
ntent using read_file with start_line and end_line to get more context."

the first line is the reasoning and the second line is the content.

-> check to make sure that when sending non duplication messages to the llm to preserve context and not lose information. and to save tokens.

full log: 

---

improve UX and responsive when agent about to edit/write file, currently the TUI is not responsive and not immediate feedback to the user when agent is about to edit/write file.
