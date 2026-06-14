# Security Policy

Bruno runs locally and can hold API keys (in `~/.config/bruno/config.toml`),
browse the web, and store data. If you find a vulnerability, please report it
privately rather than opening a public issue.

- Open a [private security advisory](https://github.com/rithulkamesh/bruno/security/advisories/new), or
- Email **hi@rithul.dev**.

Please include steps to reproduce and the affected component. You'll get an
acknowledgement as soon as possible.

## Notes

- `~/.config/bruno/config.toml` contains secrets — it is not tracked by git and
  must never be committed.
- Optional Piper TTS links GPL-3.0 native libraries (`libpiper`, espeak-ng).
