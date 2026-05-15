# Session working directory commands

Jcode sessions have a session-scoped working directory, used for tool execution, project-local instructions, project commands, and displayed environment context.

## Commands

| Command | Behavior |
|---|---|
| `/pwd` | Show the current session working directory. Does not change anything. |
| `/cwd` | Show the current session working directory. Same display behavior as `/pwd`. |
| `/cwd <path>` | Change the current session working directory to an existing directory. Relative paths are resolved from the current session cwd. `~` and `~/...` are supported. |
| `/cd <path>` | Alias for `/cwd <path>`. |
| `/cd` | Show the current session working directory, matching `/cwd`. |

## `pwd` vs `/pwd`

- `pwd` without a leading slash is a normal user message to the model.
- `/pwd` is handled directly by the TUI/server and does not call the model.

## Local and remote sessions

These commands work in both local and remote TUI sessions. In remote sessions, the client sends a `set_cwd` protocol request to the server, and the server updates and persists the active session cwd.

Changing cwd preserves conversation history and refreshes working-directory dependent context such as project-local instructions and skills.
