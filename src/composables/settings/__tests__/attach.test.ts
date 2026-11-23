import { ref } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';

type Pending = { resolve: (off: () => void) => void };
const pending: Pending[] = [];
const offs: ReturnType<typeof vi.fn>[] = [];

vi.mock('../../../lib/bridge', () => ({
  backend: {
    listen: vi.fn(() => new Promise<() => void>((resolve) => { pending.push({ resolve }); })),
    getOfficialStatus: vi.fn(async () => ({ state: 'idle', message_code: null, message: null, user_id_masked: null, nickname: null, connected_at: null, authorize_url: null })),
    getLoginStatus: vi.fn(async () => ({ state: 'idle', message: '', page_url: '' })),
  },
  toUserMessage: (_error: unknown, fallback: string) => fallback,
  isDesktop: () => false,
}));

import { createOfficialAuth } from '../useOfficialAuth';
import { createAuthFlow } from '../useAuthFlow';
import type { SettingsFeedback } from '../useSettingsFeedback';

const feedback = () => ({ dataMessage: ref(null), dataError: ref(null) }) as unknown as SettingsFeedback;
const newOff = () => { const off = vi.fn(); offs.push(off); return off; };
const live = () => offs.filter((off) => off.mock.calls.length === 0).length;

describe.each([
  ['official', () => createOfficialAuth(feedback())],
  ['login', () => createAuthFlow(feedback())],
])('%s attach / detach', (_name, make) => {
  beforeEach(() => { pending.length = 0; offs.length = 0; });

  it('listen not back at unmount: it is removed when it returns, final subscriptions 0', async () => {
    const flow = make();
    const attaching = flow.attach();
    flow.detach();
    pending[0].resolve(newOff());
    await attaching;
    expect(live()).toBe(0);
  });

  it('repeated toggles: only the current listener remains', async () => {
    const flow = make();
    for (let i = 0; i < 3; i += 1) {
      const attaching = flow.attach();
      pending[pending.length - 1].resolve(newOff());
      await attaching;
      if (i < 2) flow.detach();
    }
    expect(live()).toBe(1);
    flow.detach();
    expect(live()).toBe(0);
  });

  it('two attach calls return out of order: the first one is discarded', async () => {
    const flow = make();
    const first = flow.attach();
    const second = flow.attach();
    pending[1].resolve(newOff());
    pending[0].resolve(newOff());
    await Promise.all([first, second]);
    expect(live()).toBe(1);
  });
});
