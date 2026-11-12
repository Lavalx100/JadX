# JadX

JadX is a local-first macOS desktop assistant built with Tauri, Rust, React, TypeScript, and SQLite. It connects to LM Studio by default and offers optional IDE, Markets, and Avatar workspaces.

## Features

- Streaming conversations, persistent local chat history, context management, and usage reporting.
- Workspace-scoped IDE tools with reviewable changes and explicit approvals for sensitive actions.
- Optional market research tools that request provider data at runtime. No personal portfolio, watchlist, saved thesis, or quote-history data is bundled.
- Optional local Avatar workspace. Users import their own face models and configure local speech services; no user face, recording, or voice sample is included in this source tree.
- External providers are opt-in. Provider API keys are stored in the macOS Keychain.

## Requirements

- macOS on Apple Silicon
- Xcode Command Line Tools
- Node.js 20 or newer
- Rust stable
- LM Studio and its lms command line tool

## Setup

Install JavaScript dependencies with npm ci, then run npm run tauri:dev. The app uses the LM Studio API at 127.0.0.1:1234 by default.

Run npm run build for the frontend, npm test for the frontend unit tests, and cargo test --manifest-path src-tauri/Cargo.toml for Rust tests.

## Data and privacy

JadX keeps conversations and settings in local application support storage. Provider API keys are stored in Keychain. Local inference is the default; selecting an external provider requires an explicit data-sharing acknowledgement. Prompts, attachments, or workspace text can be sent to that provider when used, so review provider terms and the selected provider before sharing sensitive material.

Market quotes and research are fetched when requested. No user-specific market snapshots, holdings, or saved portfolio context are included in this repository. Avatar media is supplied by each user and is stored outside the source tree.

## Third-party components

See THIRD_PARTY_NOTICES.md and the licenses directory for notices covering bundled components.

## Licensing status

This private source snapshot does not yet include a project license. Add the chosen license before making the repository public or granting redistribution rights.
