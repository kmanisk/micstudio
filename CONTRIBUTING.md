# Contributing

Contributions are welcome. Audio-path changes should be small, measurable, and explicit about their effect on routing, latency, and signal processing.

## Workflow

1. Branch from `main`.
2. Follow the setup and run instructions in `README.md`.
3. Keep each pull request limited to one logical feature, fix, or documentation change.
4. Run the relevant formatter, linter, build, and tests available in the repository.
5. Describe the affected audio path, expected behavior, and verification performed.

For DSP or routing changes, include sample rate/channel assumptions and note any latency or CPU impact when relevant. Never commit recordings containing private material, credentials, machine-specific state, generated build output, or local caches.
