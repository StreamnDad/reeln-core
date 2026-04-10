# Security Policy

## Supported Versions

reeln-core is pre-1.0 software. Security fixes are published against the
latest release only. We recommend always running the most recent version
from the [Releases page](https://github.com/StreamnDad/reeln-core/releases).

| Version | Supported          |
| ------- | ------------------ |
| latest release | :white_check_mark: |
| older   | :x:                |

## Scope

reeln-core is a Rust workspace providing the native backend for the reeln
ecosystem — media processing, overlay rendering, and FFmpeg orchestration.
It is consumed as a library by other reeln components and does not expose
any network listeners of its own.

In-scope concerns include, but are not limited to:
- Memory safety issues (`unsafe` blocks, FFI boundaries, lifetime misuse)
- Command injection or argument smuggling when constructing `ffmpeg`,
  `ffprobe`, or other subprocess invocations
- Filter-graph injection via user-controlled overlay, subtitle, or text
  parameters passed to FFmpeg filter strings
- Path traversal or unsafe file handling in render pipelines, cache
  directories, or intermediate artifact paths
- Unsafe deserialization of game state, render manifests, or config files
  (JSON / TOML)
- Integer overflow, panic-on-untrusted-input, or denial-of-service in
  media parsing and overlay rendering code

Out of scope:
- Vulnerabilities in FFmpeg itself, or in third-party crates — report
  those to the respective upstream project
- Vulnerabilities in consumers of reeln-core (`reeln-cli`, `reeln-dock`,
  individual plugins) — report those to the respective repository
- Issues that require an attacker to already have local code execution
  on the user's machine

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub
issues, discussions, or pull requests.**

Report vulnerabilities using GitHub's private vulnerability reporting:

1. Go to the [Security tab](https://github.com/StreamnDad/reeln-core/security)
   of this repository
2. Click **"Report a vulnerability"**
3. Fill in as much detail as you can: affected version, reproduction steps,
   impact, and any suggested mitigation

If you cannot use GitHub's reporting, email **git-security@email.remitz.us**
instead.

### What to include

A good report contains:
- The version of reeln-core and Rust toolchain you tested against
- Your operating system and architecture (macOS / Windows / Linux, arch)
- Steps to reproduce the issue
- What you expected to happen vs. what actually happened
- The potential impact (memory corruption, code execution, command
  injection, denial of service, etc.)
- Any proof-of-concept code, if applicable

### What to expect

reeln-core is maintained by a small team, so all timelines below are
best-effort rather than hard guarantees:

- **Acknowledgement:** typically within a week of your report
- **Initial assessment:** usually within two to three weeks, including
  whether we consider the report in scope and our planned next steps
- **Status updates:** roughly every few weeks until the issue is resolved
- **Fix & disclosure:** coordinated with you. We aim to ship a patch
  release reasonably quickly for high-severity issues, with lower-severity
  issues addressed in a future release. Credit will be given in the
  release notes and CHANGELOG unless you prefer to remain anonymous.

If a report is declined, we will explain why. You are welcome to disagree
and provide additional context.
