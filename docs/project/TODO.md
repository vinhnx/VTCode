implement planning mode and TODO list (research)

--

https://agentclientprotocol.com/overview/introduction

---

reference this https://github.com/openai/codex/tree/main/codex-rs/file-search to update file search tool for vtcode.

check

Uses https://crates.io/crates/ignore under the hood (which is what ripgrep uses) to traverse a directory (while honoring .gitignore, etc.) to produce the list of files to search and then uses https://crates.io/crates/nucleo-matcher to fuzzy-match the user supplied PATTERN against the corpus. write tests to verify it works as expected. update docs and readme accordingly. update system prompt for vtcode to reflect the changes.

---

https://crates.io/crates/ignore
ignore

The ignore crate provides a fast recursive directory iterator that respects various filters such as globs, file types and .gitignore files. This crate also provides lower level direct access to gitignore and file type matchers.

---

https://crates.io/crates/nucleo-matcher

nucleo is a highly performant fuzzy matcher written in rust. It aims to fill the same use case as fzf and skim. Compared to fzf nucleo has a significantly faster matching algorithm. This mainly makes a difference when matching patterns with low selectivity on many items. An (unscientific) comparison is shown in the benchmark section below.

    Note: If you are looking for a replacement of the fuzzy-matcher crate and not a fully managed fuzzy picker, you should use the nucleo-matcher crate.

nucleo uses the exact same scoring system as fzf. That means you should get the same ranking quality (or better) as you are used to from fzf. However, nucleo has a more faithful implementation of the Smith-Waterman algorithm which is normally used in DNA sequence alignment (see https://www.cs.cmu.edu/~ckingsf/bioinfo-lectures/gaps.pdf) with two separate matrices (instead of one like fzf). This means that nucleo finds the optimal match more often. For example if you match foo in xf foo nucleo will match x\_\_foo but fzf will match xf_oo (you can increase the word length the result will stay the same). The former is the more intuitive match and has a higher score according to the ranking system that both nucleo and fzf.

Compared to skim (and the fuzzy-matcher crate) nucleo has an even larger performance advantage and is often around six times faster (see benchmarks below). Furthermore, the bonus system used by nucleo and fzf is (in my opinion) more consistent/superior. nucleo also handles non-ascii text much better. (skims bonus system and even case insensitivity only work for ASCII).

Nucleo also handles Unicode graphemes more correctly. Fzf and skim both operate on Unicode code points (chars). That means that multi codepoint graphemes can have weird effects (match multiple times, weirdly change the score, ...). nucleo will always use the first codepoint of the grapheme for matching instead (and reports grapheme indices, so they can be highlighted correctly).

--

https://github.com/Stebalien/term

A Rust library for terminfo parsing and terminal colors.

---

https://github.com/openai/codex/blob/main/codex-rs/core/src/plan_tool.rs

---

https://github.com/openai/codex/blob/main/codex-rs/core/src/project_doc.rs

---

https://github.com/openai/codex/blob/main/codex-rs/core/src/terminal.rs

--

mcp integration
https://modelcontextprotocol.io/

---

find a way to render agent output full syntax highlighting and markdown rendering in terminal. check for existing crates or libraries that can help with this. integrate the chosen solution into the vtcode application, ensuring that agent output is displayed in a clear and visually appealing manner. test the implementation to ensure that syntax highlighting and markdown rendering work correctly across different types of agent output. make sure streamed output also supports syntax highlighting and markdown rendering.
