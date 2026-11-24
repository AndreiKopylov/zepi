@echo off
start "" /d "%~dp0..\.." /b /wait /belownormal node --import tsx --test --test-concurrency=1 ^
    packages/acp/test/mcp.test.ts packages/acp/test/pi-commands.test.ts ^
    packages/acp/test/pi-network.test.ts packages/acp/test/subagents.test.ts
exit /b %errorlevel%
