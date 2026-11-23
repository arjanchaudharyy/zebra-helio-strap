import type { McpClientId, McpSidecar } from './bridge/types';

export const MCP_CLIENT_NAMES: Record<McpClientId, string> = {
  claude_desktop: 'Claude Desktop',
  claude_code: 'Claude Code',
  codex: 'Codex',
  opencode: 'OpenCode',
  grok: 'Grok Build',
  qwen: 'Qwen Code',
  gemini: 'Gemini CLI',
  factory: 'Factory',
};

const isStale = (sidecar: McpSidecar, client: McpClientId) =>
  !!sidecar.stale_clients?.includes(client) || !!sidecar.scope_stale_clients?.includes(client);

export const PLUGIN_MARKETPLACE_REF = 'main';
const PLUGIN_REPO = 'lingcang728/ZeppBridge';
const PLUGIN_ID = 'zepp-coach@zeppbridge';

export const serverArgs = (sidecar: McpSidecar | null): string[] =>
  sidecar?.task_only ? ['--scope', 'task'] : [];

const quote = (value: string) => `"${value}"`;

const envFlag = (flag: string, sidecar: McpSidecar) =>
  sidecar.data_dir_env ? [flag, quote(`ZEPPBRIDGE_DATA_DIR=${sidecar.data_dir_env}`)] : [];

const serverLaunch = (sidecar: McpSidecar, path: string) => [quote('--'), quote(path), ...serverArgs(sidecar)];

export const claudeCodeCommands = (sidecar: McpSidecar): string | null => {
  if (!sidecar.path) return null;
  return [
    ...(isStale(sidecar, 'claude_code') ? ['claude mcp remove zeppbridge --scope user'] : []),
    ['claude', 'mcp', 'add', ...envFlag('-e', sidecar), '--scope', 'user', 'zeppbridge', ...serverLaunch(sidecar, sidecar.path)].join(' '),
    `claude plugin marketplace add ${PLUGIN_REPO}#${PLUGIN_MARKETPLACE_REF}`,
    `claude plugin install ${PLUGIN_ID}`,
  ].join('\n');
};

export const codexCommands = (sidecar: McpSidecar): string | null => {
  if (!sidecar.path) return null;
  return [
    ...(isStale(sidecar, 'codex') ? ['codex mcp remove zeppbridge'] : []),
    ['codex', 'mcp', 'add', ...envFlag('--env', sidecar), 'zeppbridge', ...serverLaunch(sidecar, sidecar.path)].join(' '),
    `codex plugin marketplace add ${PLUGIN_REPO} --ref ${PLUGIN_MARKETPLACE_REF}`,
    `codex plugin add ${PLUGIN_ID}`,
  ].join('\n');
};

export const opencodeCommands = (sidecar: McpSidecar): string | null => {
  if (!sidecar.path) return null;
  return ['opencode', 'mcp', 'add', '--global', ...envFlag('--env', sidecar), 'zeppbridge', ...serverLaunch(sidecar, sidecar.path)].join(' ');
};

export const grokCommands = (sidecar: McpSidecar): string | null => {
  if (!sidecar.path) return null;
  const args = serverArgs(sidecar);
  return ['grok', 'mcp', 'add', '-s', 'user', ...envFlag('-e', sidecar), 'zeppbridge', quote(sidecar.path), ...(args.length ? [quote('--'), ...args] : [])].join(' ');
};

export const qwenCommands = (sidecar: McpSidecar): string | null => {
  if (!sidecar.path) return null;
  const args = serverArgs(sidecar);
  return ['qwen', 'mcp', 'add', '-s', 'user', ...envFlag('-e', sidecar), 'zeppbridge', quote(sidecar.path), ...(args.length ? [quote('--'), ...args] : [])].join(' ');
};

/** Gemini CLI: `gemini mcp add -s user [-e K=V] <name> <command> [args...]`, written to ~/.gemini/settings.json. */
export const geminiCommands = (sidecar: McpSidecar): string | null => {
  if (!sidecar.path) return null;
  const args = serverArgs(sidecar);
  return ['gemini', 'mcp', 'add', '-s', 'user', ...envFlag('-e', sidecar), 'zeppbridge', quote(sidecar.path), ...args].join(' ');
};

const serverEntry = (sidecar: McpSidecar | null, placeholder: string, extra: Record<string, unknown> = {}) => {
  const server: Record<string, unknown> = { ...extra, command: sidecar?.path ?? placeholder, args: serverArgs(sidecar) };
  if (sidecar?.data_dir_env) server.env = { ZEPPBRIDGE_DATA_DIR: sidecar.data_dir_env };
  return server;
};

export const factoryConfig = (sidecar: McpSidecar | null, placeholder: string): string =>
  JSON.stringify({ mcpServers: { zeppbridge: serverEntry(sidecar, placeholder, { type: 'stdio' }) } }, null, 2);

export const genericMcpConfig = (sidecar: McpSidecar | null, placeholder: string): string =>
  JSON.stringify({ mcpServers: { zeppbridge: serverEntry(sidecar, placeholder) } }, null, 2);

export const COMMAND_PRESETS: { id: McpClientId; build: (sidecar: McpSidecar) => string | null }[] = [
  { id: 'claude_code', build: claudeCodeCommands },
  { id: 'codex', build: codexCommands },
  { id: 'opencode', build: opencodeCommands },
  { id: 'grok', build: grokCommands },
  { id: 'qwen', build: qwenCommands },
  { id: 'gemini', build: geminiCommands },
];
