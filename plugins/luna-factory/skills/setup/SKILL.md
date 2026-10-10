---
name: setup
description: Guide a user through safe, operator-approved Luna Factory plugin setup.
---

# Set up Luna Factory

Use this skill after installing the Luna Factory plugin. It guides the user; it does not change local services, tunnel profiles, credentials, or account registrations.

1. Check whether the user wants to use the skill directly, the local Codex MCP service, or a private ChatGPT plugin connection. The skill works without either runtime connection.
2. For local Codex use, ask the operator to verify the existing service endpoint and plugin MCP entry. Do not restart or rewrite an existing service to make setup pass.
3. For a private ChatGPT connection, explain that the operator must first identify and authorize an independent Secure MCP Tunnel profile and the intended private plugin registration. Do not reuse or repoint an Executor route.
4. Ask the operator to follow the official ChatGPT plugin and Secure MCP Tunnel setup flow using the released Luna Factory package. Keep account registration, browser approval, credentials, and access grants in the operator's hands.
5. Do not print or paste credentials. Do not add an API key, copy native Codex credentials, create a tunnel, or change a protected profile. This setup does not change protected services or profiles.
6. Verify only the connection path the operator explicitly approved. MCP tool discovery is not proof that a ChatGPT App UI rendered. Mark any unavailable authenticated host test as unverified.

The local operator can stop here if no independent route or private registration is approved. Luna Factory's canonical orchestration skill remains usable directly.
