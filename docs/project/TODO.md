Suggestion - the harness should compact the conversation at the allowance limit and on resume:

    + ask the user to continue from summary
    + allow the user the continue but they have to be aware of the costs
    + suggest the user to start fresh.

Such continued conversations are a clear candidate for 1 hour cache TTL.

If a user is at limit, it’s very likely they won’t continue their session (not many users will opt for extra usage), so the TTL can be reduced to 10m for the prompts after the limits are hit, unless the system detects that the user refilled the extra usage allowance.

--

https://x.com/trq212/status/2044548257058328723

---


---

https://developers.openai.com/codex/memories
