import { createSSRApp } from 'vue';
import { renderToString } from '@vue/server-renderer';
import { describe, expect, it } from 'vitest';
import HandoffSteps from '../HandoffSteps.vue';
import type { HandoffStep, HandoffStepId } from '../../../composables/useAiTaskHandoff';

const step = (state: HandoffStep['state'] = 'idle', errorText: string | null = null): HandoffStep => ({
  state,
  errorText,
});

const steps = (patch: Partial<Record<HandoffStepId, HandoffStep>> = {}): Record<HandoffStepId, HandoffStep> => ({
  prepare: patch.prepare ?? step(),
  copy: patch.copy ?? step(),
  open: patch.open ?? step(),
});

const render = (value: Record<HandoffStepId, HandoffStep>, providerLabel = 'Claude') =>
  renderToString(createSSRApp(HandoffSteps, { steps: value, providerLabel }));

const count = (html: string, re: RegExp): number => (html.match(re) ?? []).length;

describe('HandoffSteps', () => {
  it('all idle: three rows, no buttons, honest state', async () => {
    const html = await render(steps());
    expect(count(html, /<li[\s>]/g)).toBe(3);
    expect(count(html, /class="step is-idle"/g)).toBe(3);
    expect(count(html, /<button/g)).toBe(0);
    expect(html).toContain('Not started');
    expect(html).toContain('Claude');
  });

  it('copy failed: exactly one retry button, errorText replaces the state text', async () => {
    const html = await render(steps({ copy: step('failed', 'clipboard dead') }));
    expect(count(html, /<button/g)).toBe(1);
    expect(html).toContain('clipboard dead');
    expect(html).toContain('class="step is-failed"');
    expect(html).toContain('Retry');
  });

  it('a failed open can also be retried', async () => {
    const html = await render(steps({ open: step('failed', 'no opener') }));
    expect(count(html, /<button/g)).toBe(1);
    expect(html).toContain('no opener');
  });

  it('a failed prepare gets no retry button, the main CTA reruns it', async () => {
    const html = await render(steps({ prepare: step('failed', 'disk full') }));
    expect(count(html, /<button/g)).toBe(0);
    expect(html).toContain('disk full');
    expect(html).toContain('class="step is-failed"');
  });

  it('all three steps failed: only copy and open get retry buttons', async () => {
    const html = await render(steps({
      prepare: step('failed', 'p'),
      copy: step('failed', 'c'),
      open: step('failed', 'o'),
    }));
    expect(count(html, /<button/g)).toBe(2);
  });

  it('done steps get is-done; blocked/skipped go into the class, with no retry', async () => {
    const html = await render(steps({
      prepare: step('done'),
      copy: step('done'),
      open: step('skipped'),
    }));
    expect(count(html, /class="step is-done"/g)).toBe(2);
    expect(html).toContain('class="step is-skipped"');
    expect(count(html, /<button/g)).toBe(0);
  });
});
