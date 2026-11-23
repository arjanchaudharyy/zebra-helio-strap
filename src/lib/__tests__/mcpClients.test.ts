import { describe, expect, it } from 'vitest';
import {
  claudeCodeCommands, codexCommands, factoryConfig, genericMcpConfig, grokCommands, opencodeCommands, qwenCommands,
} from '../mcpClients';

const path = 'C:\\Program Files\\ZeppBridge\\zeppbridge-mcp.exe';

describe('mcpClients', () => {
  it('builds no command without a sidecar', () => {
    const none = { path: null, data_dir_env: null };
    expect(claudeCodeCommands(none)).toBeNull();
    expect(codexCommands(none)).toBeNull();
    expect(opencodeCommands(none)).toBeNull();
    expect(grokCommands(none)).toBeNull();
    expect(qwenCommands(none)).toBeNull();
  });

  it('since 3.1.2 the default reads the whole library: no --scope, and quotes wrap "--" and paths with spaces', () => {
    const [mcp, market, install] = claudeCodeCommands({ path, data_dir_env: null })!.split('\n');
    expect(mcp).toBe(`claude mcp add --scope user zeppbridge "--" "${path}"`);
    expect(market).toBe('claude plugin marketplace add lingcang728/ZeppBridge#main');
    expect(install).toBe('claude plugin install zepp-coach@zeppbridge');
    expect(opencodeCommands({ path, data_dir_env: null })).toBe(`opencode mcp add --global zeppbridge "--" "${path}"`);
    expect(grokCommands({ path, data_dir_env: null })).toBe(`grok mcp add -s user zeppbridge "${path}"`);
    expect(qwenCommands({ path, data_dir_env: null })).toBe(`qwen mcp add -s user zeppbridge "${path}"`);
    expect(JSON.parse(genericMcpConfig({ path, data_dir_env: null }, '<x>')).mcpServers.zeppbridge.args).toEqual([]);
  });

  it('every client gets --scope task when "shared task only" is on', () => {
    const sidecar = { path, data_dir_env: null, task_only: true };
    expect(claudeCodeCommands(sidecar)!.split('\n')[0]).toBe(`claude mcp add --scope user zeppbridge "--" "${path}" --scope task`);
    expect(codexCommands(sidecar)!.split('\n')[0]).toBe(`codex mcp add zeppbridge "--" "${path}" --scope task`);
    expect(opencodeCommands(sidecar)).toBe(`opencode mcp add --global zeppbridge "--" "${path}" --scope task`);
    expect(grokCommands(sidecar)).toBe(`grok mcp add -s user zeppbridge "${path}" "--" --scope task`);
    expect(qwenCommands(sidecar)).toBe(`qwen mcp add -s user zeppbridge "${path}" "--" --scope task`);
    expect(JSON.parse(factoryConfig(sidecar, '<x>')).mcpServers.zeppbridge).toEqual({
      type: 'stdio', command: path, args: ['--scope', 'task'],
    });
  });

  it('every client gets ZEPPBRIDGE_DATA_DIR when the library is not next to the sidecar', () => {
    const sidecar = { path, data_dir_env: 'D:\\my data' };
    expect(claudeCodeCommands(sidecar)!.split('\n')[0]).toBe(
      `claude mcp add -e "ZEPPBRIDGE_DATA_DIR=D:\\my data" --scope user zeppbridge "--" "${path}"`,
    );
    expect(codexCommands(sidecar)!.split('\n')[0]).toBe(
      `codex mcp add --env "ZEPPBRIDGE_DATA_DIR=D:\\my data" zeppbridge "--" "${path}"`,
    );
    expect(opencodeCommands(sidecar)).toBe(
      `opencode mcp add --global --env "ZEPPBRIDGE_DATA_DIR=D:\\my data" zeppbridge "--" "${path}"`,
    );
    expect(grokCommands(sidecar)).toBe(`grok mcp add -s user -e "ZEPPBRIDGE_DATA_DIR=D:\\my data" zeppbridge "${path}"`);
    expect(qwenCommands(sidecar)).toBe(`qwen mcp add -s user -e "ZEPPBRIDGE_DATA_DIR=D:\\my data" zeppbridge "${path}"`);
    const config = JSON.parse(genericMcpConfig(sidecar, '<x>'));
    expect(config.mcpServers.zeppbridge).toEqual({
      command: path,
      args: [],
      env: { ZEPPBRIDGE_DATA_DIR: 'D:\\my data' },
    });
  });

  it('a client registered with an old version or another scope removes the old entry first, other clients are not affected', () => {
    const sidecar = { path, data_dir_env: null, stale_clients: ['codex' as const] };
    expect(codexCommands(sidecar)!.split('\n').slice(0, 2)).toEqual([
      'codex mcp remove zeppbridge',
      `codex mcp add zeppbridge "--" "${path}"`,
    ]);
    expect(claudeCodeCommands(sidecar)!.split('\n')[0]).toMatch(/^claude mcp add /);
    expect(claudeCodeCommands({ ...sidecar, stale_clients: [], scope_stale_clients: ['claude_code'] })!.split('\n')[0])
      .toBe('claude mcp remove zeppbridge --scope user');
  });

  it('the generic config uses the placeholder path without a sidecar', () => {
    expect(JSON.parse(genericMcpConfig(null, '<x>')).mcpServers.zeppbridge.command).toBe('<x>');
  });
});
