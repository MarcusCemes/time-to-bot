# Time To Bot

[![discord](https://img.shields.io/badge/Discord-%235865F2?logo=discord&logoColor=white&style=for-the-badge)](https://discord.com)

A Discord bot implementation for a private server.

## Setup

- Create a Discord application at the [Developer Portal](https://discord.com/developers/applications).
- Invite the bot to your server with the `applications.commands` scope.
- **Enable the `MESSAGE CONTENT` privileged intent** in the Bot settings — the bot reads message content for its features; without it, message content arrives empty.
- Set environment variables:
  - `DISCORD_TOKEN` — the bot token
  - `APPLICATION_ID` — the Discord application ID (required for slash commands)

## Commands

- `/gather` — A call to gather all server members for some games
- `/ping` — A ping command

<br />
<div align="center">

[![forthebadge](https://forthebadge.com/images/badges/powered-by-electricity.svg)](https://forthebadge.com)

</div>
