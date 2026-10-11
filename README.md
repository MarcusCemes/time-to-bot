# time-to-bot

A Discord bot that sleeps until mentioned, then joins the conversation until it
decides it is no longer part of it.

- **Asleep:** no model calls. Messages are only buffered in memory.
- **Awake:** each message goes to a decision model (Jev, via OpenRouter) which
  answers typed questions: is this for me, is it over, joke?, which tone?
- **Replies** come from `LLM_MODEL`: any OpenRouter model slug, or an `@preset/<slug>` to change the model from the dashboard.
- **Personality and knowledge** live in `config/persona.md`, re-read on every wake.

## Layout

| File | Role |
| --- | --- |
| `src/main.rs` | startup and wiring |
| `src/config.rs` | environment variables |
| `src/bot.rs` | Discord event handler, the per-message flow |
| `src/state.rs` | per-channel history and awake/asleep session |
| `src/gate.rs` | the questions asked of the decision model (extend here) |
| `src/decision.rs` | decision-model wire format (OpenRouter alpha endpoint) |
| `src/llm.rs` | reply generation through OpenRouter chat completions |
| `src/persona.rs` | loads the persona markdown |

## Run

Enable the **Message Content** privileged intent for the bot in the Discord
developer portal, then:

    cp .env.example .env   # fill in
    cargo run
    # or: docker compose up -d --build

Set `RUST_LOG=info,time_to_bot=debug` to see each gate verdict.
