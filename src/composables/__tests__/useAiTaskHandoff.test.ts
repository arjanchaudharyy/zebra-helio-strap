import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AiTaskPrepareResult } from '../../lib/bridge/types';
import { newTaskDraft } from '../../lib/aiTask/draft';

const prepareMock = vi.fn();
const copyMock = vi.fn(async (_text: string) => {});
const openMock = vi.fn(async (_provider: unknown): Promise<'opened' | 'skipped'> => 'opened');
const revealMock = vi.fn(async (_path: string) => {});

vi.mock('../../lib/bridge', () => ({
  backend: { aiTaskPrepare: (...args: unknown[]) => prepareMock(...args) },
  toUserMessage: (_error: unknown, fallback: string) => fallback,
}));

vi.mock('../useAiHandoff', () => ({
  copyTextToClipboard: (text: string) => copyMock(text),
  openProviderSite: (provider: unknown) => openMock(provider),
  revealInFolder: (path: string) => revealMock(path),
}));

import { useAiTaskHandoff } from '../useAiTaskHandoff';
import { AI_PROVIDERS } from '../../lib/aiProviders';

const ready = (patch: Partial<AiTaskPrepareResult> = {}): AiTaskPrepareResult => ({
  status: 'ready',
  task_id: 't-1',
  output_dir: 'C:/Users/me/Desktop/ZeppBridge AI/\u6062\u590d\u8dd1_20260924-1530',
  json_path: 'x/health-context.json',
  prompt_path: 'x/prompt.txt',
  prompt_text: 'prompt body',
  byte_len: 1234,
  attachments: [],
  blocked: [],
  ...patch,
});

describe('useAiTaskHandoff', () => {
  beforeEach(() => {
    prepareMock.mockReset();
    copyMock.mockReset();
    copyMock.mockResolvedValue(undefined);
    openMock.mockReset();
    openMock.mockResolvedValue('opened');
    revealMock.mockReset();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('runAll all green: export, copy, open the site, and select the export folder in the file manager', async () => {
    prepareMock.mockResolvedValue(ready());
    const handoff = useAiTaskHandoff();
    await handoff.runAll(newTaskDraft(), AI_PROVIDERS[0], 'Direction');
    expect(prepareMock).toHaveBeenCalledWith(expect.anything(), expect.any(String), 'Direction', undefined);
    expect(copyMock).toHaveBeenCalledWith('prompt body');
    expect(openMock).toHaveBeenCalledWith(AI_PROVIDERS[0]);
    expect(revealMock).toHaveBeenCalledWith(ready().output_dir);
    expect(handoff.steps.value.prepare.state).toBe('done');
    expect(handoff.steps.value.copy.state).toBe('done');
    expect(handoff.steps.value.open.state).toBe('done');
  });

  it('prepare blocked by the backend: the next two steps do not run and the folder does not open', async () => {
    prepareMock.mockResolvedValue(ready({
      status: 'blocked',
      blocked: [{ code: 'ui.ai_task.blocked.attachment_missing', message: 'x' }],
    }));
    const handoff = useAiTaskHandoff();
    await handoff.runAll(newTaskDraft(), AI_PROVIDERS[0]);
    expect(handoff.steps.value.prepare.state).toBe('blocked');
    expect(handoff.steps.value.copy.state).toBe('idle');
    expect(copyMock).not.toHaveBeenCalled();
    expect(openMock).not.toHaveBeenCalled();
    expect(revealMock).not.toHaveBeenCalled();
  });

  it('prepare throws: prepare=failed, later steps are not touched', async () => {
    prepareMock.mockRejectedValue(new Error('disk full'));
    const handoff = useAiTaskHandoff();
    await handoff.runAll(newTaskDraft(), AI_PROVIDERS[0]);
    expect(handoff.steps.value.prepare.state).toBe('failed');
    expect(handoff.steps.value.prepare.errorText).toBeTruthy();
    expect(copyMock).not.toHaveBeenCalled();
  });

  it('copy failed: open does not run; runCopy can be retried alone and succeed', async () => {
    prepareMock.mockResolvedValue(ready());
    copyMock.mockRejectedValueOnce(new Error('no clipboard'));
    const handoff = useAiTaskHandoff();
    await handoff.runAll(newTaskDraft(), AI_PROVIDERS[0]);
    expect(handoff.steps.value.copy.state).toBe('failed');
    expect(openMock).not.toHaveBeenCalled();
    copyMock.mockResolvedValueOnce(undefined);
    expect(await handoff.runCopy()).toBe(true);
    expect(handoff.steps.value.copy.state).toBe('done');
  });

  it('open failure is recorded as failed, the copy success stays', async () => {
    prepareMock.mockResolvedValue(ready());
    openMock.mockRejectedValue(new Error('no opener'));
    const handoff = useAiTaskHandoff();
    await handoff.runAll(newTaskDraft(), AI_PROVIDERS[0]);
    expect(handoff.steps.value.open.state).toBe('failed');
    expect(handoff.steps.value.open.errorText).toBeTruthy();
    expect(handoff.steps.value.copy.state).toBe('done');
  });

  it('web preview: open returns skipped, and the state shows it', async () => {
    prepareMock.mockResolvedValue(ready());
    openMock.mockResolvedValue('skipped');
    const handoff = useAiTaskHandoff();
    await handoff.runAll(newTaskDraft(), AI_PROVIDERS[0]);
    expect(handoff.steps.value.open.state).toBe('skipped');
  });

  it('exportOnly only exports and selects the folder, with no copy and no open', async () => {
    prepareMock.mockResolvedValue(ready());
    const handoff = useAiTaskHandoff();
    await handoff.exportOnly(newTaskDraft());
    expect(handoff.steps.value.prepare.state).toBe('done');
    expect(revealMock).toHaveBeenCalled();
    expect(copyMock).not.toHaveBeenCalled();
    expect(openMock).not.toHaveBeenCalled();
  });

  it('draft changed after prepare: isStale reports stale; a new prepare clears it', async () => {
    prepareMock.mockResolvedValue(ready());
    const handoff = useAiTaskHandoff();
    const task = newTaskDraft();
    await handoff.runAll(task, AI_PROVIDERS[0]);
    expect(handoff.isStale(task)).toBe(false);
    const edited = { ...task, prompt: 'edited afterwards' };
    expect(handoff.isStale(edited)).toBe(true);
    await handoff.runPrepare(edited);
    expect(handoff.isStale(edited)).toBe(false);
  });

  it('rerun prepare: the previous copy and open results are discarded', async () => {
    prepareMock.mockResolvedValue(ready());
    const handoff = useAiTaskHandoff();
    await handoff.runAll(newTaskDraft(), AI_PROVIDERS[0]);
    await handoff.runPrepare(newTaskDraft());
    expect(handoff.steps.value.prepare.state).toBe('done');
    expect(handoff.steps.value.copy.state).toBe('idle');
    expect(handoff.steps.value.open.state).toBe('idle');
  });

  it('lastProvider persists: the used provider is stored and a new instance starts with it', async () => {
    const store = new Map<string, string>();
    vi.stubGlobal('window', {
      localStorage: {
        getItem: (key: string) => store.get(key) ?? null,
        setItem: (key: string, value: string) => { store.set(key, String(value)); },
        removeItem: (key: string) => { store.delete(key); },
      },
    });
    const first = useAiTaskHandoff();
    expect(first.lastProvider.value).toBeNull();
    await first.runOpen(AI_PROVIDERS[2]); // gemini
    expect(store.get('zeppbridge.ai.handoff.provider')).toBe('gemini');
    expect(useAiTaskHandoff().lastProvider.value?.id).toBe('gemini');
  });

  it('an unknown provider id in localStorage is not restored', () => {
    vi.stubGlobal('window', {
      localStorage: { getItem: () => 'not-a-provider', setItem: vi.fn(), removeItem: vi.fn() },
    });
    expect(useAiTaskHandoff().lastProvider.value).toBeNull();
  });

  it('start: a second press while saving is ignored, save and export each run once (R11)', async () => {
    const handoff = useAiTaskHandoff();
    let release!: () => void;
    const save = vi.fn(() => new Promise<void>((resolve) => { release = resolve; }));
    const go = vi.fn(async () => {});
    const first = handoff.start(save, go);
    expect(handoff.inFlight.value).toBe(true);
    expect(await handoff.start(save, go)).toBe(false);
    release();
    expect(await first).toBe(true);
    expect(save).toHaveBeenCalledTimes(1);
    expect(go).toHaveBeenCalledTimes(1);
    expect(handoff.inFlight.value).toBe(false);
  });

  it('start: a save failure is not swallowed, it goes to saveError, the handoff continues; the next success clears it', async () => {
    const handoff = useAiTaskHandoff();
    const go = vi.fn(async () => {});
    await handoff.start(async () => { throw new Error('disk full'); }, go);
    expect(go).toHaveBeenCalledTimes(1);
    expect(handoff.saveError.value).toContain('The task was not saved');
    await handoff.start(async () => {}, go);
    expect(handoff.saveError.value).toBeNull();
  });

  it('start: a throwing handoff also releases inFlight', async () => {
    const handoff = useAiTaskHandoff();
    await expect(handoff.start(async () => {}, async () => { throw new Error('boom'); })).rejects.toThrow('boom');
    expect(handoff.inFlight.value).toBe(false);
  });
});
