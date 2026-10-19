# Rules

All these rules apply, unless explicitly stated otherwise anywhere else in contexts or prompts.

## General

- Prioritize accuracy and quality over speed. Always re-check your work for correctness before finalizing it.
- Always clean up any temporary utility files you create.

## Code

### Any

- Never perform any `git` operations that create, update, delete, or in any form modify state. They must be read-only.
- Keep the code highly modular. One function should do one thing and do it well.
- Use early returns and guard clauses to reduce nesting levels.
- Do not write code comments unless they are absolutely necessary.
- If both double and single quotes offer the exact same functionality all the way down, prefer double quotes.

### Rust

- Never panic. Do not use `unwrap()` or `expect()`.
- Keep the code efficient. Do not use `clone()` unless required.
- Do not perform heavy abstractions just to please the borrow checker. Think and apply a better design instead.
- Use string interpolation as `"{foo}"` instead of `"{}", foo`.
- Always run `cargo clippy` after a change. Resolve any warnings before considering your work complete.

### Yaml, Toml, etc.

- Keep the types explicit, such as strings in double quotes, not implicitly bare.
