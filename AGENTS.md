This project is the code for Shadow Warrior - a physical art project and game. The point of the game is to help people express anger and frustration in a safe and constructive way. 

# The Game Arena

The game arena is a physical space where people can play the game. It is a space with microphones, speakers, led strips, UV lights and a punching bag with IMU sensors.

# The Game

When there is no one in the arena the arena is in _idle_ mode, and the leds have a slow breathing effect to attract people.

When a player enters the arena, the game starts. The led strips respond to voice and the user fills the led strip based energy bar by shouting. Once the energy bar is full, music starts, led strips show fire and energy pulses effects, uv lights turn on and the user can punch the punching bag. 

The punching bag has IMU sensors that detect punches and send them to the game. The arena gives visual and auditory feedback to punches and shouts. The game ends after several minutes or when the user stops interacting with the game for a few minutes.

# Hardware setup

Brain runs on Raspberry Pi 4, or a Pi zero w.

# The Code

The code is split into several components:

## The Brain

The brain is the central component of the game. It is responsible for managing the game state and coordinating the different components of the game. The brain is written in Rust and uses the Tokio runtime for async operations. The brain is split into several services that communicate with each other using events.

Brain code is in @./brain-rs directory. @./brain is the legacy python code

## The Web Dashboard

The web dashboard is a web interface that allows users to interact with the game. It is written in TypeScript and uses the React framework. The web dashboard communicates with the brain using SSE.
The static html, js and css of the web dashboard are served by the brain-rs server

## The Punching Bag

The punching bag is a physical object that is used to play the game. It is equipped with IMU sensors that detect punches and send them to the brain. The punching bag is connected to the brain via Bluetooth Low Energy.

## The LED Strips

Standard WLED controllers are used for the LED strips. They are connected to the brain via Wi-Fi and are used to display visual feedback to the user. Brain communicates with WLED controllers via HTTP API and UDP realtime API.

## The Smart Plugs

Standard Tasmota smart plugs are used for the smart plugs. They are connected to the brain via Wi-Fi and are used to control the power to the LED strips and UV lights. Brain communicates with smart plugs via HTTP API.

## The Microphone

Standard USB microphone is used for the microphone. It is connected to the brain via USB and is used to detect shouts from the user. Brain communicates with the microphone via ALSA API.

## The Speakers

Standard Bluetooth speakers are used for the speakers. They are connected to the brain via Bluetooth and are used to play music and sound effects to the user. Brain communicates with the speakers via Bluetooth API.

## Landing the Plane (Session Completion)

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd sync
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:7510c1e2 -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md for details and anti-patterns.

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
<!-- END BEADS INTEGRATION -->
